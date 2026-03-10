use crate::domain::{ImportResultRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_cursor_request_paging, validate_sort_clauses, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct SubmitImportInput {
    pub request_json: String,
    pub idempotency_key: Option<String>,
}

impl HubuumGateway {
    pub fn submit_import(&self, input: SubmitImportInput) -> Result<TaskRecord, AppError> {
        let request: hubuum_client::ImportRequest = serde_json::from_str(&input.request_json)?;
        let submit = self.client.imports().submit(request);
        let task = match input.idempotency_key {
            Some(key) => submit.idempotency_key(key).send()?,
            None => submit.send()?,
        };

        Ok(TaskRecord::from(task))
    }

    pub fn import_task(&self, task_id: i32) -> Result<TaskRecord, AppError> {
        Ok(TaskRecord::from(self.client.imports().get(task_id)?))
    }

    pub fn import_results(
        &self,
        task_id: i32,
        query: &ListQuery,
    ) -> Result<PagedResult<ImportResultRecord>, AppError> {
        let validated_sorts = validate_sort_clauses(&query.sorts, IMPORT_RESULT_SORT_SPECS)?;
        let page = apply_cursor_request_paging(
            self.client.imports().results(task_id),
            query,
            &validated_sorts,
        )
        .page()?;
        Ok(PagedResult::from_page(
            page,
            query.limit,
            ImportResultRecord::from,
        ))
    }
}

pub(crate) const IMPORT_RESULT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("task_id", "task_id"),
    SortFieldSpec::new("item_ref", "item_ref"),
    SortFieldSpec::new("entity_kind", "entity_kind"),
    SortFieldSpec::new("action", "action"),
    SortFieldSpec::new("identifier", "identifier"),
    SortFieldSpec::new("outcome", "outcome"),
    SortFieldSpec::new("created_at", "created_at"),
];
