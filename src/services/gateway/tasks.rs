use crate::domain::{TaskEventRecord, TaskQueueStateRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{apply_cursor_request_paging, ListQuery, PagedResult};

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
        let page =
            apply_cursor_request_paging(self.client.tasks().events(input.task_id), query).page()?;
        Ok(PagedResult::from_page(
            page,
            query.limit,
            TaskEventRecord::from,
        ))
    }
}
