use std::collections::BTreeMap;

use hubuum_client::{BackupDocument, RestoreStageResponse};
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreReceipt {
    restore_id: i64,
    capability: String,
    sha256: String,
}

impl RestoreReceipt {
    pub(crate) fn new(restore_id: i64, capability: String, sha256: String) -> Self {
        Self {
            restore_id,
            capability,
            sha256,
        }
    }

    pub fn from_json(value: &str) -> Result<Self, AppError> {
        Ok(from_str(value)?)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RestoreRecord(Value);

impl RestoreRecord {
    pub fn from_response(mut response: RestoreStageResponse) -> Result<Self, AppError> {
        response.restore_capability = None;
        let mut value = to_value(response)?;
        if let Some(object) = value.as_object_mut() {
            object.remove("restore_capability");
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::RestoreReceipt;

    #[test]
    fn restore_receipts_round_trip_without_debugging_the_capability() {
        let receipt = RestoreReceipt::new(42, "one-time-secret".to_string(), "abc123".to_string());
        assert!(!format!("{receipt:?}").contains("one-time-secret"));

        let encoded = receipt.json_pretty().expect("serialize receipt");
        let decoded = RestoreReceipt::from_json(&encoded).expect("deserialize receipt");
        assert_eq!(decoded.restore_id(), 42);
        assert_eq!(decoded.capability(), "one-time-secret");
        assert_eq!(decoded.sha256(), "abc123");
    }
}
