use std::collections::BTreeMap;
use std::thread::sleep;
use std::time::{Duration, Instant};

use hubuum_client::{
    BackupDocument, RestoreJobStatus, RestoreStageResponse, CURRENT_BACKUP_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string_pretty, to_value, Value};

use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSummary {
    pub backup_version: i32,
    pub created_at: String,
    pub source_version: String,
    pub includes_history: bool,
    pub item_counts: BTreeMap<String, i64>,
    pub exclusions: Vec<String>,
}

#[derive(Clone)]
pub struct BackupArtifact {
    document: Value,
    summary: BackupSummary,
}

impl BackupArtifact {
    pub fn parse_document(json: &str) -> Result<BackupDocument, AppError> {
        let value: Value = from_str(json)?;
        let version = value.get("backup_version").and_then(Value::as_i64);
        if version != Some(i64::from(CURRENT_BACKUP_VERSION)) {
            return Err(AppError::InvalidOption(format!(
                "Unsupported backup version {}; this client requires format {CURRENT_BACKUP_VERSION}. Restore older artifacts with a compatible older server, then upgrade and create a new backup. Editing the version field does not convert a backup.",
                version.map_or_else(|| "(missing or invalid)".to_string(), |v| v.to_string())
            )));
        }
        Ok(serde_json::from_value(value)?)
    }

    pub fn from_document(document: BackupDocument) -> Result<Self, AppError> {
        let summary = BackupSummary {
            backup_version: document.backup_version,
            created_at: document.created_at.to_string(),
            source_version: document.source_version.clone(),
            includes_history: document.history.is_some(),
            item_counts: document.manifest.item_counts.clone(),
            exclusions: document.manifest.exclusions.clone(),
        };
        Ok(Self {
            document: to_value(document)?,
            summary,
        })
    }

    pub fn json_pretty(&self) -> Result<String, AppError> {
        Ok(to_string_pretty(&self.document)?)
    }

    pub fn summary(&self) -> &BackupSummary {
        &self.summary
    }
}

impl std::fmt::Debug for BackupArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupArtifact")
            .field("summary", &self.summary)
            .field("document", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Serialize)]
pub struct RestoreReceipt {
    restore_id: i64,
    capability: String,
    sha256: String,
}

impl RestoreReceipt {
    pub(crate) fn new(
        restore_id: i64,
        capability: String,
        sha256: String,
    ) -> Result<Self, AppError> {
        let receipt = Self {
            restore_id,
            capability,
            sha256,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn from_json(value: &str) -> Result<Self, AppError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ReceiptFields {
            restore_id: i64,
            capability: String,
            sha256: String,
        }
        let fields: ReceiptFields = from_str(value)?;
        Self::new(fields.restore_id, fields.capability, fields.sha256)
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.restore_id <= 0 {
            return Err(AppError::InvalidOption(
                "Restore receipt ID must be positive".to_string(),
            ));
        }
        if self.capability.is_empty()
            || !self.capability.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(AppError::InvalidOption(
                "Restore receipt capability must contain nonempty printable ASCII without spaces"
                    .to_string(),
            ));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AppError::InvalidOption(
                "Restore receipt SHA-256 must contain exactly 64 hexadecimal characters"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn verify_response(&self, response: &RestoreStageResponse) -> Result<(), AppError> {
        if i64::from(response.id) != self.restore_id
            || !response.sha256.eq_ignore_ascii_case(&self.sha256)
        {
            return Err(AppError::CommandExecutionError(
                "Restore response does not match the receipt ID and SHA-256".to_string(),
            ));
        }
        Ok(())
    }

    pub fn json_pretty(&self) -> Result<String, AppError> {
        Ok(to_string_pretty(self)?)
    }

    pub(crate) fn restore_id(&self) -> i64 {
        self.restore_id
    }

    pub(crate) fn capability(&self) -> &str {
        &self.capability
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl std::fmt::Debug for RestoreReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreReceipt")
            .field("restore_id", &self.restore_id)
            .field("capability", &"[REDACTED]")
            .field("sha256", &self.sha256)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct RestoreRecord {
    value: Value,
    #[serde(skip)]
    status: RestoreJobStatus,
}

impl RestoreRecord {
    pub fn from_response(mut response: RestoreStageResponse) -> Result<Self, AppError> {
        response.restore_capability = None;
        let status = response.status;
        let mut value = to_value(response)?;
        if let Some(object) = value.as_object_mut() {
            object.remove("restore_capability");
        }
        Ok(Self { value, status })
    }

    fn completed(&self) -> Result<bool, AppError> {
        match self.status {
            RestoreJobStatus::Succeeded => Ok(true),
            RestoreJobStatus::Failed | RestoreJobStatus::Expired => {
                Err(AppError::CommandExecutionError(format!(
                    "Restore {} {}: {}",
                    self.value["id"],
                    self.value["status"].as_str().unwrap_or("failed"),
                    self.value["error"]
                        .as_str()
                        .unwrap_or("inspect restore status for details")
                )))
            }
            _ => Ok(false),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RestoreWaitOptions {
    timeout: Duration,
    poll_interval: Duration,
}

impl RestoreWaitOptions {
    pub fn new(
        timeout_secs: Option<u64>,
        poll_interval_secs: Option<u64>,
    ) -> Result<Self, AppError> {
        let timeout = Duration::from_secs(timeout_secs.unwrap_or(300));
        let poll_interval = Duration::from_secs(poll_interval_secs.unwrap_or(1));
        if poll_interval.is_zero() {
            return Err(AppError::InvalidOption(
                "--poll-interval must be at least 1 second".to_string(),
            ));
        }
        Ok(Self {
            timeout,
            poll_interval,
        })
    }

    pub fn wait(
        self,
        mut poll: impl FnMut() -> Result<RestoreRecord, AppError>,
    ) -> Result<RestoreRecord, AppError> {
        let started = Instant::now();
        loop {
            let record = poll().map_err(|error| AppError::CommandExecutionError(format!(
                "Restore monitoring stopped: {error}. Keep the receipt and use 'restore status' or 'restore wait' with --receipt to resume monitoring."
            )))?;
            if record.completed()? {
                return Ok(record);
            }
            let remaining = self.timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(AppError::CommandExecutionError(format!(
                    "Timed out waiting for restore {} (status: {}). The restore was not cancelled. Keep the receipt and use 'restore status' or 'restore wait' with --receipt to resume monitoring; check that the matching hubuum-admin --restore-executor is running.",
                    record.value["id"], record.value["status"].as_str().unwrap_or("unknown")
                )));
            }
            sleep(self.poll_interval.min(remaining));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{from_str, json, to_value, Value};

    use super::{BackupArtifact, RestoreReceipt, RestoreRecord, RestoreWaitOptions};

    fn record(status: &str) -> RestoreRecord {
        let mut value: Value = from_str(include_str!("../../tests/fixtures/restore.json")).unwrap();
        value["status"] = json!(status);
        RestoreRecord::from_response(serde_json::from_value(value).unwrap()).unwrap()
    }

    #[test]
    fn backup_round_trip_preserves_the_creation_instant_and_json_null() {
        let document =
            BackupArtifact::parse_document(include_str!("../../tests/fixtures/backup.json"))
                .unwrap();
        let instant = document.created_at.clone();
        let artifact = BackupArtifact::from_document(document).unwrap();
        let saved = artifact.json_pretty().unwrap();
        assert!(saved.contains("2026-09-09T13:02:03.456789+00:00"));
        let staged = BackupArtifact::parse_document(&saved).unwrap();
        assert_eq!(staged.created_at, instant);
        assert!(staged.state.sections["objects"][0]["data"].is_null());
    }

    #[test]
    fn legacy_backup_errors_explain_migration_before_decoding_rows() {
        let error = BackupArtifact::parse_document(r#"{"backup_version":4}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("format 5"));
        assert!(error.contains("compatible older server"));
        assert!(error.contains("does not convert"));
    }

    #[test]
    fn malformed_receipts_are_rejected_without_disclosing_the_capability() {
        for (id, capability, hash) in [
            (0, "one-time-secret", "a".repeat(64)),
            (42, "secret\nvalue", "a".repeat(64)),
            (42, "", "a".repeat(64)),
            (42, "one-time-secret", "bad".to_string()),
        ] {
            let error = RestoreReceipt::from_json(
                &json!({"restore_id":id,"capability":capability,"sha256":hash}).to_string(),
            )
            .unwrap_err()
            .to_string();
            assert!(!error.contains("one-time-secret"));
            assert!(!error.contains("secret\nvalue"));
        }
    }

    #[test]
    fn restore_output_never_contains_the_capability_field() {
        let record = record("validated");
        assert!(!format!("{record:?}").contains("one-time-secret"));
        assert!(to_value(record)
            .unwrap()
            .get("restore_capability")
            .is_none());
    }

    #[test]
    fn waiting_does_not_treat_confirmation_or_unknown_states_as_completion() {
        let mut statuses = ["validated", "confirmed", "future_state", "succeeded"].into_iter();
        let options = RestoreWaitOptions {
            timeout: Duration::from_secs(1),
            poll_interval: Duration::ZERO,
        };
        let result = options
            .wait(|| Ok(record(statuses.next().expect("unexpected extra poll"))))
            .unwrap();
        assert_eq!(to_value(result).unwrap()["status"], "succeeded");
        assert!(statuses.next().is_none());
    }

    #[test]
    fn wait_reports_failure_expiry_and_timeout() {
        for status in ["failed", "expired"] {
            let error = RestoreWaitOptions::new(Some(0), None)
                .unwrap()
                .wait(|| Ok(record(status)))
                .unwrap_err()
                .to_string();
            assert!(error.contains(status));
        }
        let error = RestoreWaitOptions::new(Some(0), None)
            .unwrap()
            .wait(|| Ok(record("confirmed")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not cancelled"));
        assert!(error.contains("restore wait"));
        assert!(RestoreWaitOptions::new(None, Some(0)).is_err());
    }

    #[test]
    fn restore_receipts_round_trip_without_debugging_the_capability() {
        let receipt = RestoreReceipt::new(42, "one-time-secret".to_string(), "a".repeat(64))
            .expect("valid receipt");
        assert!(!format!("{receipt:?}").contains("one-time-secret"));

        let encoded = receipt.json_pretty().expect("serialize receipt");
        let decoded = RestoreReceipt::from_json(&encoded).expect("deserialize receipt");
        assert_eq!(decoded.restore_id(), 42);
        assert_eq!(decoded.capability(), "one-time-secret");
        assert_eq!(decoded.sha256(), "a".repeat(64));
    }
}
