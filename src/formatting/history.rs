use super::TableRenderable;
use crate::domain::{HistoricalRelationRecord, HistoryRecord};
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
