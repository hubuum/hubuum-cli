use serde::{Deserialize, Serialize};
use serde_json::{json, to_string, to_value, Error as JsonError, Map, Value};

use hubuum_filter::OutputEnvelope;
use json_patch::diff;

use crate::errors::AppError;
use crate::formatting::{OutputFormatter, TableRenderable};
use crate::output::set_semantic_output;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRecord {
    #[serde(flatten)]
    pub value: Value,
}

impl From<Value> for JsonRecord {
    fn from(value: Value) -> Self {
        Self { value }
    }
}

impl JsonRecord {
    pub fn from_serializable<T: Serialize>(value: T) -> Result<Self, JsonError> {
        Ok(Self {
            value: to_value(value)?,
        })
    }

    pub(crate) fn with_before_after_diff(mut self) -> Result<Self, JsonError> {
        let generated_diff = match &self.value {
            Value::Object(object) if !object.contains_key("diff") => {
                match (object.get("before"), object.get("after")) {
                    (Some(before), Some(after)) => Some(to_value(diff(before, after))?),
                    _ => None,
                }
            }
            _ => None,
        };

        if let (Some(object), Some(generated_diff)) = (self.value.as_object_mut(), generated_diff) {
            object.insert("diff".to_string(), generated_diff);
        }

        Ok(self)
    }

    fn get_string(&self, key: &str) -> String {
        self.value
            .get(key)
            .map(json_summary)
            .unwrap_or_else(|| "-".to_string())
    }
}

impl TableRenderable for JsonRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "name/type",
            "status/action",
            "summary",
            "updated/occurred",
        ]
    }

    fn row(&self) -> Vec<String> {
        if self.is_history_record() {
            return vec![
                self.get_string("history_id")
                    .or_else_dash(self.get_string("id")),
                self.get_string("name")
                    .or_else_dash(self.get_string("entity_type"))
                    .or_else_dash(self.get_string("kind")),
                self.get_string("op")
                    .or_else_dash(self.get_string("operation"))
                    .or_else_dash(self.get_string("status"))
                    .or_else_dash(self.get_string("action")),
                self.get_string("summary")
                    .or_else_dash(self.get_string("description"))
                    .or_else_dash(self.get_string("entity_name")),
                self.get_string("valid_from")
                    .or_else_dash(self.get_string("updated_at"))
                    .or_else_dash(self.get_string("occurred_at")),
            ];
        }

        vec![
            self.get_string("id")
                .or_else_dash(self.get_string("event_id"))
                .or_else_dash(self.get_string("history_id")),
            self.get_string("name")
                .or_else_dash(self.get_string("entity_type"))
                .or_else_dash(self.get_string("kind")),
            self.get_string("status")
                .or_else_dash(self.get_string("action"))
                .or_else_dash(self.get_string("operation"))
                .or_else_dash(self.get_string("op")),
            self.get_string("summary")
                .or_else_dash(self.get_string("description"))
                .or_else_dash(self.get_string("entity_name")),
            self.get_string("updated_at")
                .or_else_dash(self.get_string("occurred_at"))
                .or_else_dash(self.get_string("valid_from")),
        ]
    }
}

impl JsonRecord {
    fn is_history_record(&self) -> bool {
        self.value.get("history_id").is_some()
            || self.value.get("valid_from").is_some()
            || self.value.get("op").is_some()
    }
}

fn json_summary(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => to_string(values).unwrap_or_else(|_| "[]".to_string()),
        Value::Object(value) => to_string(value).unwrap_or_else(|_| "{}".to_string()),
    }
}

impl OutputFormatter for JsonRecord {
    fn format(&self) -> Result<Self, AppError> {
        let (value, columns) = match &self.value {
            Value::Object(map) => {
                let mut object = Map::new();
                let mut columns = Vec::with_capacity(map.len());
                for (key, value) in map {
                    columns.push(key.clone());
                    object.insert(key.clone(), Value::String(json_summary(value)));
                }
                (Value::Object(object), columns)
            }
            value => (
                json!({ "value": json_summary(value) }),
                vec!["value".to_string()],
            ),
        };

        set_semantic_output(OutputEnvelope::detail(value, columns))?;
        Ok(self.clone())
    }
}

trait DashFallback {
    fn or_else_dash(self, fallback: String) -> String;
}

impl DashFallback for String {
    fn or_else_dash(self, fallback: String) -> String {
        if self == "-" {
            fallback
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::JsonRecord;
    use crate::formatting::TableRenderable;

    #[test]
    fn history_rows_prefer_history_metadata_over_resource_fields() {
        let record = JsonRecord::from(json!({
            "id": 1526,
            "history_id": 5031,
            "op": "U",
            "valid_from": "2026-07-05T23:31:49.388144+00:00",
            "valid_to": null,
            "name": "host.example.org",
            "description": "host.example.org (192.0.2.10)",
            "updated_at": "2026-07-05T23:31:49.388144+00:00"
        }));

        assert_eq!(
            record.row(),
            vec![
                "5031".to_string(),
                "host.example.org".to_string(),
                "U".to_string(),
                "host.example.org (192.0.2.10)".to_string(),
                "2026-07-05T23:31:49.388144+00:00".to_string(),
            ]
        );
    }

    #[test]
    fn before_after_diff_is_an_rfc_6902_patch() {
        let record = JsonRecord::from(json!({
            "before": {
                "data": {
                    "hardware": {
                        "memory": {"total": "580 GB"}
                    }
                },
                "updated_at": "2026-07-11T08:48:45.312920"
            },
            "after": {
                "data": {
                    "hardware": {
                        "memory": {"total": "581 GB"}
                    }
                },
                "updated_at": "2026-07-21T20:17:03.617598"
            }
        }))
        .with_before_after_diff()
        .expect("diff should serialize");

        assert_eq!(
            record.value["diff"],
            json!([
                {
                    "op": "replace",
                    "path": "/data/hardware/memory/total",
                    "value": "581 GB"
                },
                {
                    "op": "replace",
                    "path": "/updated_at",
                    "value": "2026-07-21T20:17:03.617598"
                }
            ])
        );
    }

    #[test]
    fn before_after_diff_requires_both_snapshots() {
        let record = JsonRecord::from(json!({"before": {"name": "host.example.org"}}))
            .with_before_after_diff()
            .expect("record should remain serializable");

        assert!(record.value.get("diff").is_none());
    }

    #[test]
    fn before_after_diff_preserves_server_value() {
        let record = JsonRecord::from(json!({
            "before": {"name": "before.example.org"},
            "after": {"name": "after.example.org"},
            "diff": {"provided_by": "server"}
        }))
        .with_before_after_diff()
        .expect("record should remain serializable");

        assert_eq!(record.value["diff"], json!({"provided_by": "server"}));
    }
}
