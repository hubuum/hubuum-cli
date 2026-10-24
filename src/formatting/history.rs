use super::TableRenderable;
use crate::domain::{HistoricalRelationRecord, HistoryRecord, HistoryTimelineRecord};
use crate::errors::AppError;
use crate::output::append_line;
use chrono::{DateTime, Utc};
use serde_json::Value;

fn snapshot_field(snapshot: &Value, key: &str) -> String {
    match &snapshot[key] {
        Value::Null => "-".into(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

impl TableRenderable for HistoryRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "history_id",
            "id",
            "name",
            "revision",
            "operation",
            "valid_from",
            "valid_to",
        ]
    }

    fn row(&self) -> Vec<String> {
        Self::headers()
            .iter()
            .map(|field| {
                if *field == "operation" && self.snapshot().get("operation").is_none() {
                    snapshot_field(self.snapshot(), "op")
                } else {
                    snapshot_field(self.snapshot(), field)
                }
            })
            .collect()
    }
}

impl TableRenderable for HistoryTimelineRecord {
    fn headers() -> Vec<&'static str> {
        vec!["history_id", "revision", "valid_from", "changes"]
    }

    fn row(&self) -> Vec<String> {
        let timestamp = snapshot_field(self.snapshot(), "valid_from");
        let timestamp = DateTime::parse_from_rfc3339(&timestamp)
            .map(|time| {
                time.with_timezone(&Utc)
                    .format("%Y-%m-%d\n%H:%M:%S%.f")
                    .to_string()
            })
            .unwrap_or(timestamp);
        vec![
            snapshot_field(self.snapshot(), "history_id"),
            snapshot_field(self.snapshot(), "revision"),
            timestamp,
            self.snapshot()["changes"]["summary"]
                .as_str()
                .unwrap_or("Unavailable")
                .to_owned(),
        ]
    }
}

pub(crate) fn append_history_changes(records: &[HistoryRecord]) -> Result<(), AppError> {
    for record in records {
        let Some(changes) = record.changes() else {
            continue;
        };
        append_line(format!(
            "\nChanges for history #{} (revision {}):",
            snapshot_field(record.snapshot(), "history_id"),
            snapshot_field(record.snapshot(), "revision")
        ))?;
        for line in changes.lines() {
            append_line(format!("  {line}"))?;
        }
    }
    Ok(())
}
impl TableRenderable for HistoricalRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "history_id",
            "revision",
            "operation",
            "from",
            "to",
            "forward_label",
            "reverse_label",
            "context_at",
        ]
    }
    fn row(&self) -> Vec<String> {
        Self::headers()
            .iter()
            .map(|field| match *field {
                "operation" => snapshot_field(self.snapshot(), "op"),
                "from" | "to" => {
                    let endpoint = &self.snapshot()[field];
                    if endpoint["status"] == "available" {
                        format!(
                            "{} (#{})",
                            endpoint["snapshot"]["name"].as_str().unwrap_or("-"),
                            endpoint["snapshot"]["id"]
                        )
                    } else {
                        format!(
                            "unavailable: {}",
                            endpoint["reason"].as_str().unwrap_or("history_unavailable")
                        )
                    }
                }
                "forward_label" | "reverse_label" => self.snapshot()["relation"][field]
                    .as_str()
                    .unwrap_or("-")
                    .to_owned(),
                field => snapshot_field(self.snapshot(), field),
            })
            .collect()
    }
}
