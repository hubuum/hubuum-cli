use std::time::Duration;

use hubuum_client::{
    types::SortDirection, TaskCancelRequest, TaskCancellationReason, TaskKind, TaskStatus,
};

use crate::domain::{
    ImportResultRecord, TaskEventRecord, TaskOutput, TaskQueueStateRecord, TaskRecord,
};
use crate::errors::AppError;
use crate::list_query::{
    fetch_cursor_results, validate_sort_clauses, ListQuery, PageSelection, PagedResult,
    SortFieldSpec,
};
use crate::services::WaitTaskInput;

use super::{HubuumGateway, TaskDiscovery};

#[derive(Debug, Clone)]
pub struct TaskLookupInput {
    pub task_id: i32,
}

#[derive(Debug, Clone, Default)]
pub struct ListTasksInput {
    pub discovery: TaskDiscovery,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub include_total: bool,
    pub page_selection: PageSelection,
}

impl HubuumGateway {
    pub fn cancel_task(
        &self,
        input: TaskLookupInput,
        reason: Option<String>,
        expected_status: Option<String>,
    ) -> Result<TaskRecord, AppError> {
        let request = TaskCancelRequest {
            reason: reason.map(TaskCancellationReason::new).transpose()?,
            expected_status: expected_status
                .as_deref()
                .map(parse_task_status)
                .transpose()?,
        };
        Ok(self.client().tasks().cancel(input.task_id, request)?.into())
    }

    pub fn recent_schema_tasks(&self) -> Result<Vec<TaskRecord>, AppError> {
        Ok(self
            .client()
            .tasks()
            .query()
            .kind(TaskKind::SchemaValidation)
            .sort("id", SortDirection::Desc)
            .limit(50)
            .page()?
            .items
            .into_iter()
            .map(TaskRecord::from)
            .collect())
    }

    pub fn task_queue_state(&self) -> Result<TaskQueueStateRecord, AppError> {
        Ok(TaskQueueStateRecord::from(self.client().meta_tasks()?))
    }

    pub fn task(&self, input: TaskLookupInput) -> Result<TaskRecord, AppError> {
        Ok(TaskRecord::from(self.client().tasks().get(input.task_id)?))
    }

    pub fn task_events(
        &self,
        input: TaskLookupInput,
        query: &ListQuery,
    ) -> Result<PagedResult<TaskEventRecord>, AppError> {
        let validated_sorts = validate_sort_clauses(&query.sorts, TASK_EVENT_SORT_SPECS)?;
        let page = fetch_cursor_results(
            self.client().tasks().events(input.task_id),
            query,
            &validated_sorts,
        )?;
        Ok(page.map(TaskEventRecord::from))
    }

    pub fn task_output(&self, task_id: i32) -> Result<TaskOutput, AppError> {
        let task = self.client().tasks().get(task_id)?;
        Ok(match task.kind {
            TaskKind::Export => TaskOutput::Export(self.client().exports().output(task_id)?.into()),
            TaskKind::Import => {
                let results: Vec<ImportResultRecord> = self
                    .client()
                    .imports()
                    .results(task_id)
                    .list()?
                    .into_iter()
                    .map(ImportResultRecord::from)
                    .collect();
                TaskOutput::ImportResults(results)
            }
            // RemoteCall task output is not exposed by the current client API.
            _ => TaskOutput::None,
        })
    }

    pub fn wait_task(&self, input: WaitTaskInput) -> Result<TaskRecord, AppError> {
        let mut op = self.client().tasks().wait(input.task_id);
        if let Some(p) = input.poll_interval_secs {
            op = op.poll_interval(Duration::from_secs(p));
        }
        op = op.timeout(input.timeout_secs.map(Duration::from_secs));
        Ok(TaskRecord(op.send()?))
    }

    pub fn list_tasks(&self, input: ListTasksInput) -> Result<PagedResult<TaskRecord>, AppError> {
        let mut q = self.client().tasks().query();
        if let Some(k) = input.kind.as_deref() {
            q = q.kinds(
                k.split(',')
                    .map(|kind| parse_task_kind(kind.trim()))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        if let Some(s) = input.status.as_deref() {
            q = q.statuses(
                s.split(',')
                    .map(|status| parse_task_status(status.trim()))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        if let Some(l) = input.limit {
            q = q.limit(l);
        }
        if let Some(c) = input.cursor {
            q = q.cursor(c);
        }
        q = input.discovery.apply(q).include_total(input.include_total);
        let page = if matches!(input.page_selection, PageSelection::All) {
            PagedResult::from_pages(q.pages())?
        } else {
            PagedResult::from_page(q.page()?, |task| task)
        };
        Ok(page.map(TaskRecord::from))
    }
}

fn parse_task_kind(s: &str) -> Result<TaskKind, AppError> {
    match s.to_lowercase().as_str() {
        "import" => Ok(TaskKind::Import),
        "export" => Ok(TaskKind::Export),
        "backup" => Ok(TaskKind::Backup),
        "reindex" => Ok(TaskKind::Reindex),
        "remote_call" | "remotecall" => Ok(TaskKind::RemoteCall),
        "schemavalidation" | "schema_validation" => Ok(TaskKind::SchemaValidation),
        _ => Err(AppError::InvalidOption(format!(
            "Invalid task kind '{}'. Valid values: import, export, backup, reindex, remotecall, schema_validation",
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
        "partially_succeeded" | "partiallysucceeded" => Ok(TaskStatus::PartiallySucceeded),
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

    use crate::formatting::DetailRenderable;
    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{Method, StatusCode};
    use serde_json::{from_slice, json, Value};
    use std::sync::Arc;

    fn gateway(transport: &MockTransport) -> HubuumGateway {
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("test-token"));
        HubuumGateway::new(Arc::new(client))
    }

    #[test]
    fn cancellation_preserves_running_cleanup_state_and_sends_precondition() {
        let transport = MockTransport::default();
        transport.push_response(TransportResponse::json(StatusCode::OK, &json!({
            "id": 71, "kind": "schema_validation", "status": "running",
            "created_at": "2026-09-15T00:00:00Z",
            "progress": {"total_items": 10, "processed_items": 2, "success_items": 2, "failed_items": 0},
            "links": {"task": "/api/v1/tasks/71", "events": "/api/v1/tasks/71/events"},
            "cancel_requested_at": "2026-09-15T00:01:00Z", "cancel_requested_by": 1,
            "cancel_reason": "Withdraw work", "unattempted_items": 8,
            "remote_side_effect_state": "possibly_sent",
            "execution_deadline_at": "2026-09-15T01:00:00Z"
        })).unwrap());
        let task = gateway(&transport)
            .cancel_task(
                TaskLookupInput { task_id: 71 },
                Some("Withdraw work".into()),
                Some("running".into()),
            )
            .unwrap();
        assert_eq!(task.0.status, TaskStatus::Running);
        assert_eq!(task.0.kind, TaskKind::SchemaValidation);
        let rows = task.detail_rows();
        assert!(rows.contains(&("Unattempted", "8".into())));
        assert!(rows.contains(&("Remote Side Effects", "PossiblySent".into())));
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::POST);
        assert_eq!(requests[0].url.path(), "/api/v1/tasks/71/cancel");
        assert_eq!(
            from_slice::<Value>(requests[0].body()).unwrap(),
            json!({"reason": "Withdraw work", "expected_status": "running"})
        );
    }

    #[test]
    fn invalid_cancellation_options_do_not_send_requests() {
        let transport = MockTransport::default();
        let gateway = gateway(&transport);
        for reason in ["", "  ", "two\nlines"] {
            assert!(gateway
                .cancel_task(TaskLookupInput { task_id: 71 }, Some(reason.into()), None)
                .is_err());
        }
        assert!(gateway
            .cancel_task(
                TaskLookupInput { task_id: 71 },
                None,
                Some("invalid".into())
            )
            .is_err());
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn parse_task_kind_accepts_valid_lowercase() {
        assert!(matches!(parse_task_kind("import"), Ok(TaskKind::Import)));
        assert!(matches!(
            parse_task_kind("schema_validation"),
            Ok(TaskKind::SchemaValidation)
        ));
        assert!(matches!(parse_task_kind("export"), Ok(TaskKind::Export)));
        assert!(matches!(parse_task_kind("backup"), Ok(TaskKind::Backup)));
        assert!(matches!(parse_task_kind("reindex"), Ok(TaskKind::Reindex)));
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
