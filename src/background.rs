use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hubuum_client::TaskStatus;
use serde::Serialize;
use tokio::runtime::Handle;

use crate::domain::TaskRecord;
use crate::errors::AppError;
use crate::services::{HubuumGateway, TaskLookupInput};

type TaskFetcher = Arc<dyn Fn(i32) -> Result<TaskRecord, AppError> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct BackgroundJobRecord {
    pub id: u64,
    pub task_id: i32,
    pub label: String,
    pub state: String,
    pub status: String,
    pub summary: Option<String>,
    pub created_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BackgroundWatchRegistration {
    pub local_id: u64,
    pub task_id: i32,
    pub created: bool,
}

#[derive(Clone)]
pub struct BackgroundManager {
    inner: Arc<Mutex<BackgroundState>>,
    runtime: Handle,
    poll_interval: Duration,
    fetch_task: TaskFetcher,
}

#[derive(Debug, Default)]
struct BackgroundState {
    enabled: bool,
    next_id: u64,
    jobs: BTreeMap<u64, BackgroundJob>,
}

#[derive(Debug, Clone)]
struct BackgroundJob {
    id: u64,
    task_id: i32,
    label: String,
    task: Option<TaskRecord>,
    last_error: Option<String>,
    poller_running: bool,
    pending_notice: PendingNotice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PendingNotice {
    #[default]
    None,
    Done,
    Attention,
}

impl BackgroundManager {
    pub fn new(runtime: Handle, gateway: Arc<HubuumGateway>, poll_interval: Duration) -> Self {
        let fetch_task = Arc::new(move |task_id| gateway.task(TaskLookupInput { task_id }));
        Self::new_with_fetcher(runtime, poll_interval, fetch_task)
    }

    fn new_with_fetcher(runtime: Handle, poll_interval: Duration, fetch_task: TaskFetcher) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BackgroundState::default())),
            runtime,
            poll_interval,
            fetch_task,
        }
    }

    pub fn enable(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.enabled = true;
        }
    }

    pub fn disable(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.enabled = false;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.enabled)
            .unwrap_or(false)
    }

    pub fn require_enabled(&self) -> Result<(), AppError> {
        if self.is_enabled() {
            Ok(())
        } else {
            Err(AppError::CommandExecutionError(
                "Background jobs are only available in the interactive REPL".to_string(),
            ))
        }
    }

    pub fn watch_task(
        &self,
        task: TaskRecord,
        label: impl Into<String>,
    ) -> Option<BackgroundWatchRegistration> {
        if !self.is_enabled() {
            return None;
        }

        let (registration, should_spawn) = {
            let mut guard = self
                .inner
                .lock()
                .expect("background manager lock should not be poisoned");

            if let Some(job) = guard.jobs.values().find(|job| job.task_id == task.0.id) {
                (
                    BackgroundWatchRegistration {
                        local_id: job.id,
                        task_id: job.task_id,
                        created: false,
                    },
                    false,
                )
            } else {
                guard.next_id += 1;
                let id = guard.next_id;
                let should_spawn = !is_terminal_status(task.0.status);
                guard.jobs.insert(
                    id,
                    BackgroundJob {
                        id,
                        task_id: task.0.id,
                        label: label.into(),
                        task: Some(task.clone()),
                        last_error: None,
                        poller_running: should_spawn,
                        pending_notice: PendingNotice::None,
                    },
                );

                (
                    BackgroundWatchRegistration {
                        local_id: id,
                        task_id: task.0.id,
                        created: true,
                    },
                    should_spawn,
                )
            }
        };

        if should_spawn {
            self.spawn_poller(registration.local_id, registration.task_id);
        }

        Some(registration)
    }

    pub fn list_jobs(&self) -> Vec<BackgroundJobRecord> {
        let guard = self
            .inner
            .lock()
            .expect("background manager lock should not be poisoned");

        guard.jobs.values().map(BackgroundJob::record).collect()
    }

    pub fn job(&self, id: u64) -> Option<BackgroundJobRecord> {
        let guard = self
            .inner
            .lock()
            .expect("background manager lock should not be poisoned");

        guard.jobs.get(&id).map(BackgroundJob::record)
    }

    pub fn forget_job(&self, id: u64) -> bool {
        self.inner
            .lock()
            .expect("background manager lock should not be poisoned")
            .jobs
            .remove(&id)
            .is_some()
    }

    pub fn take_prompt_badge(&self) -> Option<String> {
        let mut guard = self
            .inner
            .lock()
            .expect("background manager lock should not be poisoned");
        if !guard.enabled {
            return None;
        }

        let active = guard.jobs.values().filter(|job| job.poller_running).count();
        let done = guard
            .jobs
            .values()
            .filter(|job| job.pending_notice == PendingNotice::Done)
            .count();
        let attention = guard
            .jobs
            .values()
            .filter(|job| job.pending_notice == PendingNotice::Attention)
            .count();

        for job in guard.jobs.values_mut() {
            job.pending_notice = PendingNotice::None;
        }

        let mut parts = Vec::new();
        if active > 0 {
            parts.push(format!("bg:{active}"));
        }
        if done > 0 {
            parts.push(format!("done:{done}"));
        }
        if attention > 0 {
            parts.push(format!("attention:{attention}"));
        }

        if parts.is_empty() {
            None
        } else {
            Some(format!("[{}]", parts.join(" ")))
        }
    }

    fn spawn_poller(&self, local_id: u64, task_id: i32) {
        let manager = self.clone();
        self.runtime.spawn(async move {
            loop {
                if !manager.should_poll(local_id) {
                    break;
                }

                let fetch_task = manager.fetch_task.clone();
                let result = tokio::task::spawn_blocking(move || fetch_task(task_id)).await;

                match result {
                    Ok(Ok(task)) => {
                        let terminal = is_terminal_status(task.0.status);
                        manager.record_task(local_id, task);
                        if terminal {
                            break;
                        }
                    }
                    Ok(Err(err)) => {
                        manager.record_error(local_id, err.to_string(), false);
                    }
                    Err(err) => {
                        manager.record_error(local_id, err.to_string(), true);
                        break;
                    }
                }

                tokio::time::sleep(manager.poll_interval).await;
            }
        });
    }

    fn should_poll(&self, local_id: u64) -> bool {
        let guard = self
            .inner
            .lock()
            .expect("background manager lock should not be poisoned");
        guard
            .jobs
            .get(&local_id)
            .map(|job| job.poller_running)
            .unwrap_or(false)
    }

    fn record_task(&self, local_id: u64, task: TaskRecord) {
        let mut guard = self
            .inner
            .lock()
            .expect("background manager lock should not be poisoned");
        let Some(job) = guard.jobs.get_mut(&local_id) else {
            return;
        };

        let was_terminal = job
            .task
            .as_ref()
            .map(|task| is_terminal_status(task.0.status))
            .unwrap_or(false);
        let is_terminal = is_terminal_status(task.0.status);

        job.last_error = None;
        job.task = Some(task.clone());
        job.poller_running = !is_terminal;
        if !was_terminal && is_terminal {
            job.pending_notice = if is_attention_status(task.0.status) {
                PendingNotice::Attention
            } else {
                PendingNotice::Done
            };
        }
    }

    fn record_error(&self, local_id: u64, message: String, fatal: bool) {
        let mut guard = self
            .inner
            .lock()
            .expect("background manager lock should not be poisoned");
        let Some(job) = guard.jobs.get_mut(&local_id) else {
            return;
        };

        job.last_error = Some(message);
        if fatal {
            job.poller_running = false;
            job.pending_notice = PendingNotice::Attention;
        }
    }
}

impl BackgroundJob {
    fn record(&self) -> BackgroundJobRecord {
        let task_status = self
            .task
            .as_ref()
            .map(|task| task.0.status.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let state = if self.poller_running {
            "watching".to_string()
        } else if self
            .task
            .as_ref()
            .map(|task| is_terminal_status(task.0.status))
            .unwrap_or(false)
        {
            "completed".to_string()
        } else if self.last_error.is_some() {
            "attention".to_string()
        } else {
            "idle".to_string()
        };

        BackgroundJobRecord {
            id: self.id,
            task_id: self.task_id,
            label: self.label.clone(),
            state,
            status: task_status,
            summary: self.task.as_ref().and_then(|task| task.0.summary.clone()),
            created_at: self.task.as_ref().map(|task| task.0.created_at.to_string()),
            started_at: self
                .task
                .as_ref()
                .and_then(|task| task.0.started_at.as_ref().map(ToString::to_string)),
            finished_at: self
                .task
                .as_ref()
                .and_then(|task| task.0.finished_at.as_ref().map(ToString::to_string)),
            last_error: self.last_error.clone(),
        }
    }
}

fn is_terminal_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Succeeded
            | TaskStatus::Failed
            | TaskStatus::PartiallySucceeded
            | TaskStatus::Cancelled
    )
}

fn is_attention_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Failed | TaskStatus::PartiallySucceeded | TaskStatus::Cancelled
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use hubuum_client::{TaskKind, TaskLinks, TaskProgress, TaskResponse, TaskStatus};
    use tokio::runtime::{Handle, Runtime};

    use super::{BackgroundJob, BackgroundManager, BackgroundState, PendingNotice};
    use crate::domain::TaskRecord;
    use crate::errors::AppError;

    #[test]
    fn prompt_badge_prefers_active_and_pending_counts() {
        let mut state = BackgroundState {
            enabled: true,
            ..BackgroundState::default()
        };
        state.jobs.insert(
            1,
            BackgroundJob {
                id: 1,
                task_id: 10,
                label: "import".to_string(),
                task: None,
                last_error: None,
                poller_running: true,
                pending_notice: PendingNotice::Done,
            },
        );

        let active = state.jobs.values().filter(|job| job.poller_running).count();
        let done = state
            .jobs
            .values()
            .filter(|job| job.pending_notice == PendingNotice::Done)
            .count();

        assert_eq!(active, 1);
        assert_eq!(done, 1);
    }

    #[test]
    fn background_manager_tracks_mock_task_through_phases() {
        let runtime = Runtime::new().expect("runtime should build");
        runtime.block_on(async {
            let phases = Arc::new(Mutex::new(VecDeque::from([
                Ok(task(42, TaskStatus::Running, Some("running"))),
                Ok(task(42, TaskStatus::Succeeded, Some("done"))),
            ])));
            let fetcher = {
                let phases = phases.clone();
                Arc::new(move |_task_id| -> Result<TaskRecord, AppError> {
                    let mut guard = phases.lock().expect("phase lock should not be poisoned");
                    if let Some(next) = guard.pop_front() {
                        next
                    } else {
                        Ok(task(42, TaskStatus::Succeeded, Some("done")))
                    }
                })
            };

            let manager = BackgroundManager::new_with_fetcher(
                Handle::current(),
                Duration::from_millis(10),
                fetcher,
            );
            manager.enable();

            manager.watch_task(task(42, TaskStatus::Queued, Some("queued")), "import 42");
            assert_eq!(manager.take_prompt_badge().as_deref(), Some("[bg:1]"));

            tokio::time::sleep(Duration::from_millis(40)).await;

            let job = manager.job(1).expect("job should exist");
            assert_eq!(job.status, "succeeded");
            assert_eq!(job.state, "completed");
            assert_eq!(manager.take_prompt_badge().as_deref(), Some("[done:1]"));
            assert_eq!(manager.take_prompt_badge(), None);
        });
    }

    fn task(task_id: i32, status: TaskStatus, summary: Option<&str>) -> TaskRecord {
        TaskRecord(TaskResponse {
            id: task_id,
            kind: TaskKind::Import,
            status,
            submitted_by: Some(1),
            created_at: Default::default(),
            started_at: None,
            finished_at: None,
            progress: TaskProgress {
                total_items: 1,
                processed_items: if matches!(status, TaskStatus::Queued) {
                    0
                } else {
                    1
                },
                success_items: if matches!(status, TaskStatus::Succeeded) {
                    1
                } else {
                    0
                },
                failed_items: 0,
            },
            summary: summary.map(str::to_string),
            request_redacted_at: None,
            links: TaskLinks {
                task: format!("/api/v1/tasks/{task_id}"),
                events: format!("/api/v1/tasks/{task_id}/events"),
                import_url: Some(format!("/api/v1/imports/{task_id}")),
                import_results: Some(format!("/api/v1/imports/{task_id}/results")),
            },
            details: None,
        })
    }
}
