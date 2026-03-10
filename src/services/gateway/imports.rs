use crate::domain::{ImportResultRecord, TaskRecord};
use crate::errors::AppError;

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

    pub fn import_results(&self, task_id: i32) -> Result<Vec<ImportResultRecord>, AppError> {
        Ok(self
            .client
            .imports()
            .results(task_id)
            .list()?
            .into_iter()
            .map(ImportResultRecord::from)
            .collect())
    }
}
