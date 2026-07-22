use serde::{Deserialize, Serialize};
use serde_json::{json, to_string, to_value, Error as JsonError, Map, Value};

use hubuum_filter::OutputEnvelope;

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

    pub(crate) fn into_audit_detail(mut self, include_snapshots: bool) -> Self {
        let generated_diff = match &self.value {
            Value::Object(object) if !object.contains_key("diff") => {
                match (object.get("before"), object.get("after")) {
                    (Some(before), Some(after)) => {
                        Some(nested_json_diff(before, after).unwrap_or_else(empty_json_object))
                    }
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(object) = self.value.as_object_mut() {
            if let Some(generated_diff) = generated_diff {
                object.insert("diff".to_string(), generated_diff);
            }
            if !include_snapshots {
                object.remove("before");
                object.remove("after");
            }
        }

        self
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

fn detail_json_summary(key: &str, value: &Value) -> String {
    if key == "diff" && matches!(value, Value::Array(_) | Value::Object(_)) {
        pretty_json(value)
    } else {
        json_summary(value)
    }
}

fn nested_json_diff(before: &Value, after: &Value) -> Option<Value> {
    if before == after {
        return None;
    }

    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            let mut keys = before.keys().chain(after.keys()).collect::<Vec<_>>();
            keys.sort_unstable();
            keys.dedup();

            let mut changes = Map::new();
            for key in keys {
                let change = match (before.get(key), after.get(key)) {
                    (Some(before), Some(after)) => nested_json_diff(before, after),
                    (Some(before), None) => Some(change_pair(before.clone(), Value::Null)),
                    (None, Some(after)) => Some(change_pair(Value::Null, after.clone())),
                    (None, None) => None,
                };
                if let Some(change) = change {
                    changes.insert(key.clone(), change);
                }
            }
            Some(Value::Object(changes))
        }
        _ => Some(change_pair(before.clone(), after.clone())),
    }
}

fn change_pair(before: Value, after: Value) -> Value {
    json!({"before": before, "after": after})
}

fn empty_json_object() -> Value {
    Value::Object(Map::new())
}

fn pretty_json(value: &Value) -> String {
    let mut output = String::new();
    write_pretty_json(value, 0, &mut output);
    output
}

fn write_pretty_json(value: &Value, indent: usize, output: &mut String) {
    match value {
        Value::Object(object) if object.is_empty() => output.push_str("{}"),
        Value::Object(object) => {
            output.push_str("{\n");
            let mut entries = object.iter().collect::<Vec<_>>();
            if object.len() == 2 && object.contains_key("before") && object.contains_key("after") {
                entries.sort_by_key(|(key, _)| if key.as_str() == "before" { 0 } else { 1 });
            }
            for (index, (key, value)) in entries.iter().enumerate() {
                output.push_str(&" ".repeat(indent + 2));
                output
                    .push_str(&to_string(key).expect("a JSON object key should always serialize"));
                output.push_str(": ");
                write_pretty_json(value, indent + 2, output);
                if index + 1 != entries.len() {
                    output.push(',');
                }
                output.push('\n');
            }
            output.push_str(&" ".repeat(indent));
            output.push('}');
        }
        Value::Array(values) if values.is_empty() => output.push_str("[]"),
        Value::Array(values) => {
            output.push_str("[\n");
            for (index, value) in values.iter().enumerate() {
                output.push_str(&" ".repeat(indent + 2));
                write_pretty_json(value, indent + 2, output);
                if index + 1 != values.len() {
                    output.push(',');
                }
                output.push('\n');
            }
            output.push_str(&" ".repeat(indent));
            output.push(']');
        }
        value => output.push_str(&to_string(value).expect("a JSON value should always serialize")),
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
                    object.insert(key.clone(), Value::String(detail_json_summary(key, value)));
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

    use super::{detail_json_summary, JsonRecord};
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
    fn audit_detail_contains_a_nested_before_after_diff() {
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
        .into_audit_detail(false);

        assert_eq!(
            record.value["diff"],
            json!({
                "data": {
                    "hardware": {
                        "memory": {
                            "total": {
                                "before": "580 GB",
                                "after": "581 GB"
                            }
                        }
                    }
                },
                "updated_at": {
                    "before": "2026-07-11T08:48:45.312920",
                    "after": "2026-07-21T20:17:03.617598"
                }
            })
        );
        assert!(record.value.get("before").is_none());
        assert!(record.value.get("after").is_none());

        let formatted = detail_json_summary("diff", &record.value["diff"]);
        let before = formatted
            .find(r#""before": "580 GB""#)
            .expect("formatted diff should include before");
        let after = formatted
            .find(r#""after": "581 GB""#)
            .expect("formatted diff should include after");
        assert!(before < after);
    }

    #[test]
    fn before_after_diff_requires_both_snapshots() {
        let record = JsonRecord::from(json!({"before": {"name": "host.example.org"}}))
            .into_audit_detail(true);

        assert!(record.value.get("diff").is_none());
        assert!(record.value.get("before").is_some());
    }

    #[test]
    fn complete_audit_detail_preserves_snapshots() {
        let record = JsonRecord::from(json!({
            "before": {"name": "before.example.org"},
            "after": {"name": "after.example.org"}
        }))
        .into_audit_detail(true);

        assert!(record.value.get("before").is_some());
        assert!(record.value.get("after").is_some());
        assert!(record.value.get("diff").is_some());
    }

    #[test]
    fn before_after_diff_preserves_server_value() {
        let record = JsonRecord::from(json!({
            "before": {"name": "before.example.org"},
            "after": {"name": "after.example.org"},
            "diff": {"provided_by": "server"}
        }))
        .into_audit_detail(false);

        assert_eq!(record.value["diff"], json!({"provided_by": "server"}));
        assert!(record.value.get("before").is_none());
        assert!(record.value.get("after").is_none());
    }
}
