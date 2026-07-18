use std::time::Duration;

use hubuum_client::{
    BackupDocument, BackupRequest, RestoreCapability, RestoreConfirmRequest, RestoreId,
};

use crate::domain::{BackupArtifact, RestoreReceipt, RestoreRecord, TaskRecord};
use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct BackupInput {
    include_history: bool,
    idempotency_key: Option<String>,
}

impl BackupInput {
    pub fn new(include_history: bool) -> Self {
        Self {
            include_history,
            idempotency_key: None,
        }
    }

    pub fn idempotency_key(mut self, idempotency_key: Option<String>) -> Self {
        self.idempotency_key = idempotency_key;
        self
    }

    fn request(&self) -> BackupRequest {
        BackupRequest::new().include_history(self.include_history)
    }
}

#[derive(Debug, Clone)]
pub struct RunBackupInput {
    backup: BackupInput,
    timeout_secs: Option<u64>,
    poll_interval_secs: Option<u64>,
}

impl RunBackupInput {
    pub fn new(backup: BackupInput) -> Self {
        Self {
            backup,
            timeout_secs: Some(300),
            poll_interval_secs: Some(1),
        }
    }

    pub fn timeout_secs(mut self, timeout_secs: Option<u64>) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub fn poll_interval_secs(mut self, poll_interval_secs: Option<u64>) -> Self {
        self.poll_interval_secs = poll_interval_secs;
        self
    }
}

impl HubuumGateway {
    pub fn submit_backup(&self, input: BackupInput) -> Result<TaskRecord, AppError> {
        let mut operation = self.client.backups().submit(input.request());
        if let Some(idempotency_key) = input.idempotency_key {
            operation = operation.idempotency_key(idempotency_key);
        }
        Ok(TaskRecord(operation.send()?))
    }

    pub fn backup_task(&self, task_id: i32) -> Result<TaskRecord, AppError> {
        Ok(TaskRecord(self.client.backups().get(task_id)?))
    }

    pub fn backup_output(&self, task_id: i32) -> Result<BackupArtifact, AppError> {
        BackupArtifact::from_document(self.client.backups().output(task_id)?)
    }

    pub fn run_backup(&self, input: RunBackupInput) -> Result<BackupArtifact, AppError> {
        let mut operation = self.client.backups().run(input.backup.request());
        if let Some(idempotency_key) = input.backup.idempotency_key {
            operation = operation.idempotency_key(idempotency_key);
        }
        operation = operation.timeout(input.timeout_secs.map(Duration::from_secs));
        if let Some(poll_interval_secs) = input.poll_interval_secs {
            operation = operation.poll_interval(Duration::from_secs(poll_interval_secs));
        }
        BackupArtifact::from_document(operation.send()?)
    }

    pub fn stage_restore(
        &self,
        backup_json: &str,
    ) -> Result<(RestoreRecord, RestoreReceipt), AppError> {
        let document: BackupDocument = serde_json::from_str(backup_json)?;
        if !document.has_supported_version() {
            return Err(AppError::InvalidOption(format!(
                "Unsupported backup version {}",
                document.backup_version
            )));
        }
        let mut response = self.client.restores().stage(&document)?;
        let capability = response.restore_capability.take().ok_or_else(|| {
            AppError::CommandExecutionError(
                "Restore stage did not return its one-time capability".to_string(),
            )
        })?;
        let receipt = RestoreReceipt::new(
            response.id.into(),
            capability.as_str().to_string(),
            response.sha256.clone(),
        );
        Ok((RestoreRecord::from_response(response)?, receipt))
    }

    pub fn restore_status(&self, receipt: &RestoreReceipt) -> Result<RestoreRecord, AppError> {
        let response = self.client.restores().status(
            RestoreId::from(receipt.restore_id()),
            &RestoreCapability::new(receipt.capability()),
        )?;
        RestoreRecord::from_response(response)
    }

    pub fn confirm_restore(&self, receipt: &RestoreReceipt) -> Result<RestoreRecord, AppError> {
        let request = RestoreConfirmRequest::new(
            RestoreCapability::new(receipt.capability()),
            receipt.sha256(),
        );
        let response = self
            .client
            .restores()
            .confirm(RestoreId::from(receipt.restore_id()), request)?;
        RestoreRecord::from_response(response)
    }
}
