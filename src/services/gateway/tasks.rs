use std::time::Duration;

use hubuum_client::{TaskKind, TaskStatus};

use crate::domain::{ImportResultRecord, TaskEventRecord, TaskOutput, TaskQueueStateRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_cursor_request_paging, validate_sort_clauses, ListQuery, PagedResult, SortFieldSpec,
};
use crate::services::WaitTaskInput;

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct TaskLookupInput {
    pub task_id: i32,
}

#[derive(Debug, Clone, Default)]
pub struct ListTasksInput {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

impl HubuumGateway {
    pub fn task_queue_state(&self) -> Result<TaskQueueStateRecord, AppError> {
        Ok(TaskQueueStateRecord::from(self.client.meta_tasks()?))
    }

    pub fn task(&self, input: TaskLookupInput) -> Result<TaskRecord, AppError> {
        Ok(TaskRecord::from(self.client.tasks().get(input.task_id)?))
    }

    pub fn task_events(
        &self,
        input: TaskLookupInput,
        query: &ListQuery,
    ) -> Result<PagedResult<TaskEventRecord>, AppError> {
        let validated_sorts = validate_sort_clauses(&query.sorts, TASK_EVENT_SORT_SPECS)?;
        let page = apply_cursor_request_paging(
            self.client.tasks().events(input.task_id),
            query,
            &validated_sorts,
        )
        .page()?;
        Ok(PagedResult::from_page(
            page,
            query.limit,
            TaskEventRecord::from,
        ))
    }

    pub fn task_output(&self, task_id: i32) -> Result<TaskOutput, AppError> {
        let task = self.client.tasks().get(task_id)?;
        Ok(match task.kind {
            TaskKind::Report => {
                TaskOutput::Report(self.client.reports().output(task_id)?.into())
            }
            TaskKind::Import => {
                let results: Vec<ImportResultRecord> = self
                    .client
                    .imports()
                    .results(task_id)
                    .list()?
                    .into_iter()
                    .map(ImportResultRecord::from)
                    .collect();
                TaskOutput::ImportResults(results)
            }
            // RemoteCall results are not fetchable in 0.0.3; see Task 3.4
            _ => TaskOutput::None,
        })
    }

    pub fn wait_task(&self, input: WaitTaskInput) -> Result<TaskRecord, AppError> {
        let mut op = self.client.tasks().wait(input.task_id);
        if let Some(p) = input.poll_interval_secs {
            op = op.poll_interval(Duration::from_secs(p));
        }
        op = op.timeout(input.timeout_secs.map(Duration::from_secs));
        Ok(TaskRecord(op.send()?))
    }

    pub fn list_tasks(&self, input: ListTasksInput) -> Result<PagedResult<TaskRecord>, AppError> {
        let mut q = self.client.tasks().query();
        if let Some(k) = input.kind.as_deref() {
            q = q.kind(parse_task_kind(k)?);
        }
        if let Some(s) = input.status.as_deref() {
            q = q.status(parse_task_status(s)?);
        }
        if let Some(l) = input.limit {
            q = q.limit(l);
        }
        if let Some(c) = input.cursor {
            q = q.cursor(c);
        }
        let page = q.page()?;
        Ok(PagedResult::from_page(
            page,
            input.limit,
            TaskRecord::from,
        ))
    }
}

fn parse_task_kind(s: &str) -> Result<TaskKind, AppError> {
    match s.to_lowercase().as_str() {
        "import" => Ok(TaskKind::Import),
        "report" => Ok(TaskKind::Report),
        "export" => Ok(TaskKind::Export),
        "reindex" => Ok(TaskKind::Reindex),
        "remotecall" => Ok(TaskKind::RemoteCall),
        _ => Err(AppError::InvalidOption(format!(
            "Invalid task kind '{}'. Valid values: import, report, export, reindex, remotecall",
            s
        ))),
    }
}

fn parse_task_status(s: &str) -> Result<TaskStatus, AppError> {
    match s.to_lowercase().as_str() {
        "queued" => Ok(TaskStatus::Queued),
        "validating" => Ok(TaskStatus::Validating),
        "running" => Ok(TaskStatus::Running),
        "succeeded" => Ok(TaskStatus::Succeeded),
        "failed" => Ok(TaskStatus::Failed),
        "partiallysucceeded" => Ok(TaskStatus::PartiallySucceeded),
        "cancelled" => Ok(TaskStatus::Cancelled),
        _ => Err(AppError::InvalidOption(format!(
            "Invalid task status '{}'. Valid values: queued, validating, running, succeeded, failed, partiallysucceeded, cancelled",
            s
        ))),
    }
}

pub(crate) const TASK_EVENT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("task_id", "task_id"),
    SortFieldSpec::new("event_type", "event_type"),
    SortFieldSpec::new("message", "message"),
    SortFieldSpec::new("created_at", "created_at"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_task_kind_accepts_valid_lowercase() {
        assert!(matches!(parse_task_kind("import"), Ok(TaskKind::Import)));
        assert!(matches!(parse_task_kind("report"), Ok(TaskKind::Report)));
        assert!(matches!(parse_task_kind("export"), Ok(TaskKind::Export)));
        assert!(matches!(
            parse_task_kind("reindex"),
            Ok(TaskKind::Reindex)
        ));
        assert!(matches!(
            parse_task_kind("remotecall"),
            Ok(TaskKind::RemoteCall)
        ));
    }

    #[test]
    fn parse_task_kind_accepts_mixed_case() {
        assert!(matches!(parse_task_kind("Import"), Ok(TaskKind::Import)));
        assert!(matches!(
            parse_task_kind("RemoteCall"),
            Ok(TaskKind::RemoteCall)
        ));
    }

    #[test]
    fn parse_task_kind_rejects_invalid() {
        assert!(parse_task_kind("invalid").is_err());
        assert!(parse_task_kind("").is_err());
    }

    #[test]
    fn parse_task_status_accepts_valid_lowercase() {
        assert!(matches!(
            parse_task_status("queued"),
            Ok(TaskStatus::Queued)
        ));
        assert!(matches!(
            parse_task_status("validating"),
            Ok(TaskStatus::Validating)
        ));
        assert!(matches!(
            parse_task_status("running"),
            Ok(TaskStatus::Running)
        ));
        assert!(matches!(
            parse_task_status("succeeded"),
            Ok(TaskStatus::Succeeded)
        ));
        assert!(matches!(
            parse_task_status("failed"),
            Ok(TaskStatus::Failed)
        ));
        assert!(matches!(
            parse_task_status("partiallysucceeded"),
            Ok(TaskStatus::PartiallySucceeded)
        ));
        assert!(matches!(
            parse_task_status("cancelled"),
            Ok(TaskStatus::Cancelled)
        ));
    }

    #[test]
    fn parse_task_status_accepts_mixed_case() {
        assert!(matches!(
            parse_task_status("Queued"),
            Ok(TaskStatus::Queued)
        ));
        assert!(matches!(
            parse_task_status("PartiallySucceeded"),
            Ok(TaskStatus::PartiallySucceeded)
        ));
    }

    #[test]
    fn parse_task_status_rejects_invalid() {
        assert!(parse_task_status("invalid").is_err());
        assert!(parse_task_status("").is_err());
    }
}
