use crate::domain::{TaskEventRecord, TaskQueueStateRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_cursor_request_paging, validate_sort_clauses, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct TaskLookupInput {
    pub task_id: i32,
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
}

pub(crate) const TASK_EVENT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("task_id", "task_id"),
    SortFieldSpec::new("event_type", "event_type"),
    SortFieldSpec::new("message", "message"),
    SortFieldSpec::new("created_at", "created_at"),
];
