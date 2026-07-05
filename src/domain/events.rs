use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::formatting::{DetailRenderable, TableRenderable};

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
    pub fn from_serializable<T: Serialize>(value: T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            value: serde_json::to_value(value)?,
        })
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
        vec![
            self.get_string("id")
                .or_else_dash(self.get_string("event_id"))
                .or_else_dash(self.get_string("history_id")),
            self.get_string("name")
                .or_else_dash(self.get_string("entity_type"))
                .or_else_dash(self.get_string("kind")),
            self.get_string("status")
                .or_else_dash(self.get_string("action"))
                .or_else_dash(self.get_string("operation")),
            self.get_string("summary")
                .or_else_dash(self.get_string("description"))
                .or_else_dash(self.get_string("entity_name")),
            self.get_string("updated_at")
                .or_else_dash(self.get_string("occurred_at"))
                .or_else_dash(self.get_string("valid_from")),
        ]
    }
}

impl DetailRenderable for JsonRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        match &self.value {
            Value::Object(map) => map
                .iter()
                .map(|(key, value)| (leak_key(key), json_summary(value)))
                .collect(),
            _ => vec![("value", json_summary(&self.value))],
        }
    }
}

fn json_summary(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string()),
        Value::Object(value) => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
    }
}

fn leak_key(key: &str) -> &'static str {
    Box::leak(key.to_string().into_boxed_str())
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
