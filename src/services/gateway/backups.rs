use std::time::Duration;

use hubuum_client::{
    blocking::Client, BackupRequest, RestoreCapability, RestoreConfirmRequest, RestoreId,
    Unauthenticated,
};

use crate::domain::{BackupArtifact, RestoreReceipt, RestoreRecord, TaskRecord};
use crate::errors::AppError;

use super::HubuumGateway;

pub struct RestoreMonitor {
    client: Client<Unauthenticated>,
}

impl RestoreMonitor {
    pub fn new(client: Client<Unauthenticated>) -> Self {
        Self { client }
    }

    pub fn status(&self, receipt: &RestoreReceipt) -> Result<RestoreRecord, AppError> {
        let response = self.client.restore_status(
            RestoreId::from(receipt.restore_id()),
            &RestoreCapability::new(receipt.capability()),
        )?;
        receipt.verify_response(&response)?;
        RestoreRecord::from_response(response)
    }
}

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
        let mut operation = self.client().backups().submit(input.request());
        if let Some(idempotency_key) = input.idempotency_key {
            operation = operation.idempotency_key(idempotency_key);
        }
        Ok(TaskRecord(operation.send()?))
    }

    pub fn backup_task(&self, task_id: i32) -> Result<TaskRecord, AppError> {
        Ok(TaskRecord(self.client().backups().get(task_id)?))
    }

    pub fn backup_output(&self, task_id: i32) -> Result<BackupArtifact, AppError> {
        BackupArtifact::from_document(self.client().backups().output(task_id)?)
    }

    pub fn run_backup(&self, input: RunBackupInput) -> Result<BackupArtifact, AppError> {
        let mut operation = self.client().backups().run(input.backup.request());
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
        let document = BackupArtifact::parse_document(backup_json)?;
        let mut response = self.client().restores().stage(&document)?;
        let capability = response.restore_capability.take().ok_or_else(|| {
            AppError::CommandExecutionError(
                "Restore stage did not return its one-time capability".to_string(),
            )
        })?;
        let receipt = RestoreReceipt::new(
            response.id.into(),
            capability.as_str().to_string(),
            response.sha256.clone(),
        )?;
        Ok((RestoreRecord::from_response(response)?, receipt))
    }

    pub fn restore_status(&self, receipt: &RestoreReceipt) -> Result<RestoreRecord, AppError> {
        let response = self.client().restores().status(
            RestoreId::from(receipt.restore_id()),
            &RestoreCapability::new(receipt.capability()),
        )?;
        receipt.verify_response(&response)?;
        RestoreRecord::from_response(response)
    }

    pub fn confirm_restore(&self, receipt: &RestoreReceipt) -> Result<RestoreRecord, AppError> {
        let request = RestoreConfirmRequest::new(
            RestoreCapability::new(receipt.capability()),
            receipt.sha256(),
        );
        let response = self
            .client()
            .restores()
            .confirm(RestoreId::from(receipt.restore_id()), request)?;
        receipt.verify_response(&response)?;
        RestoreRecord::from_response(response)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{Method, StatusCode};
    use serde_json::{from_slice, from_str, json, to_value, Value};

    use super::{HubuumGateway, RestoreMonitor};
    use crate::domain::RestoreReceipt;

    fn response(status: &str) -> Value {
        let mut value: Value =
            from_str(include_str!("../../../tests/fixtures/restore.json")).unwrap();
        value["status"] = json!(status);
        value
    }

    fn gateway(transport: &MockTransport) -> HubuumGateway {
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("old-token"));
        HubuumGateway::new(Arc::new(client))
    }

    #[test]
    fn backup_stage_confirm_and_status_preserve_the_contract_and_redact_secrets() {
        let transport = MockTransport::default();
        for status in ["validated", "confirmed", "succeeded"] {
            transport
                .push_response(TransportResponse::json(StatusCode::OK, &response(status)).unwrap());
        }
        let gateway = gateway(&transport);
        let (record, receipt) = gateway
            .stage_restore(include_str!("../../../tests/fixtures/backup.json"))
            .unwrap();
        assert!(to_value(record)
            .unwrap()
            .get("restore_capability")
            .is_none());
        assert_eq!(receipt.capability(), "one-time-secret");
        let accepted = gateway.confirm_restore(&receipt).unwrap();
        assert_eq!(to_value(accepted).unwrap()["status"], "confirmed");
        let completed = gateway.restore_status(&receipt).unwrap();
        assert_eq!(to_value(completed).unwrap()["status"], "succeeded");
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        let stage: Value = from_slice(requests[0].body()).unwrap();
        assert_eq!(stage["created_at"], "2026-09-09T13:02:03.456789+00:00");
        let confirm: Value = from_slice(requests[1].body()).unwrap();
        assert_eq!(requests[1].method, Method::POST);
        assert_eq!(confirm["confirmation"], "REPLACE ALL HUBUUM DATA");
        assert_eq!(confirm["sha256"], "a".repeat(64));
        assert_eq!(confirm["restore_capability"], "one-time-secret");
        assert_eq!(requests[2].method, Method::GET);
        assert_eq!(requests[2].url.path(), "/api/v1/restores/42/status");
        assert_eq!(
            requests[2].headers["x-hubuum-restore-capability"],
            "one-time-secret"
        );
        assert!(!requests[2].headers.contains_key("authorization"));
        assert!(requests[2].url.query().is_none());
    }

    #[test]
    fn monitoring_needs_no_authenticated_client_and_checks_the_receipt() {
        let transport = MockTransport::default();
        let mut mismatched = response("succeeded");
        mismatched["sha256"] = json!("b".repeat(64));
        transport.push_response(TransportResponse::json(StatusCode::OK, &mismatched).unwrap());
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap();
        let receipt =
            RestoreReceipt::new(42, "one-time-secret".to_string(), "a".repeat(64)).unwrap();
        let error = RestoreMonitor::new(client)
            .status(&receipt)
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not match"));
        assert!(!error.contains("one-time-secret"));
        assert!(!transport.requests()[0]
            .headers
            .contains_key("authorization"));
    }

    #[test]
    fn unsupported_backups_never_reach_the_server() {
        let transport = MockTransport::default();
        assert!(gateway(&transport)
            .stage_restore(r#"{"backup_version":4}"#)
            .is_err());
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn missing_stage_capabilities_cannot_produce_unusable_receipts() {
        let transport = MockTransport::default();
        let mut staged = response("validated");
        staged["restore_capability"] = Value::Null;
        transport.push_response(TransportResponse::json(StatusCode::OK, &staged).unwrap());
        assert!(gateway(&transport)
            .stage_restore(include_str!("../../../tests/fixtures/backup.json"))
            .unwrap_err()
            .to_string()
            .contains("one-time capability"));
    }
}
