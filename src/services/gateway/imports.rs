use super::credential_approvals::send_approved;
use hubuum_client::{CredentialOperation, FullImportRequest};

use crate::domain::{ImportResultRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    fetch_cursor_results, validate_sort_clauses, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct SubmitImportInput {
    pub request: FullImportRequest,
    pub idempotency_key: Option<String>,
}

impl HubuumGateway {
    pub fn submit_import(&self, input: SubmitImportInput) -> Result<TaskRecord, AppError> {
        let client = self.client();
        let submit = client.imports().submit_full(input.request.clone());
        let result = match input.idempotency_key.as_ref() {
            Some(key) => submit.idempotency_key(key).send(),
            None => submit.send(),
        };
        let task = match result {
            Ok(task) => task,
            Err(error) if error.is_reauthentication_required() => {
                let description = format!(
                    "Import {} items, dry run: {}",
                    input.request.total_items(),
                    input.request.dry_run.unwrap_or(false)
                );
                let mut approved = self.approval_source().approve(
                    &client,
                    CredentialOperation::import_credentials(input.request),
                    &description,
                )?;
                if let Some(key) = input.idempotency_key {
                    approved = approved.idempotency_key(key);
                }
                send_approved(approved)?
            }
            Err(error) => return Err(error.into()),
        };

        Ok(TaskRecord::from(task))
    }

    pub fn import_task(&self, task_id: i32) -> Result<TaskRecord, AppError> {
        Ok(TaskRecord::from(self.client().imports().get(task_id)?))
    }

    pub fn import_results(
        &self,
        task_id: i32,
        query: &ListQuery,
    ) -> Result<PagedResult<ImportResultRecord>, AppError> {
        let validated_sorts = validate_sort_clauses(&query.sorts, IMPORT_RESULT_SORT_SPECS)?;
        let page = fetch_cursor_results(
            self.client().imports().results(task_id),
            query,
            &validated_sorts,
        )?;
        Ok(page.map(ImportResultRecord::from))
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
