use serde::{Deserialize, Serialize};
use serde_json::{to_value, Error as JsonError, Value};

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
}
