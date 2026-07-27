use hubuum_client::{
    ObjectAggregateDimensionValue, ObjectAggregateMeasureOperation, ObjectAggregateMeasureState,
    ObjectAggregateMeasureValue, ObjectAggregateRow, ObjectAggregateValueState,
};
use serde::{Deserialize, Serialize};
use serde_json::{to_value, Value};

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectAggregateDimensionState {
    Value,
    Null,
    Missing,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectAggregateDimensionRecord {
    pub field: String,
    pub state: ObjectAggregateDimensionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectAggregateMeasureStateRecord {
    Value,
    Empty,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectAggregateMeasureRecord {
    pub field: String,
    pub operation: String,
    pub state: ObjectAggregateMeasureStateRecord,
    pub value_count: i64,
    pub skipped_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectAggregateRecord {
    dimensions: Vec<ObjectAggregateDimensionRecord>,
    #[serde(default)]
    measures: Vec<ObjectAggregateMeasureRecord>,
    object_count: i64,
}

impl ObjectAggregateRecord {
    pub fn semantic_value(&self) -> Result<Value, AppError> {
        let mut object = to_value(self)?
            .as_object()
            .cloned()
            .expect("object aggregate records serialize as objects");

        for dimension in &self.dimensions {
            object.insert(
                display_selector(&dimension.field),
                dimension.display_value(),
            );
        }
        for measure in &self.measures {
            object.insert(measure.display_selector(), measure.display_value());
        }

        Ok(Value::Object(object))
    }
}

impl ObjectAggregateDimensionRecord {
    fn display_value(&self) -> Value {
        match self.state {
            ObjectAggregateDimensionState::Value => self.value.clone().unwrap_or(Value::Null),
            ObjectAggregateDimensionState::Null => Value::String("<null>".to_string()),
            ObjectAggregateDimensionState::Missing => Value::String("<missing>".to_string()),
            ObjectAggregateDimensionState::Unavailable => {
                Value::String("<unavailable>".to_string())
            }
            ObjectAggregateDimensionState::Unknown => Value::String("<unknown>".to_string()),
        }
    }
}

impl ObjectAggregateMeasureRecord {
    fn display_selector(&self) -> String {
        format!("{}:{}", self.operation, display_selector(&self.field))
    }

    fn display_value(&self) -> Value {
        match self.state {
            ObjectAggregateMeasureStateRecord::Value => self.value.clone().unwrap_or(Value::Null),
            ObjectAggregateMeasureStateRecord::Empty => Value::String("<empty>".to_string()),
            ObjectAggregateMeasureStateRecord::Unknown => Value::String("<unknown>".to_string()),
        }
    }
}

impl From<ObjectAggregateRow> for ObjectAggregateRecord {
    fn from(value: ObjectAggregateRow) -> Self {
        Self {
            dimensions: value.dimensions.into_iter().map(Into::into).collect(),
            measures: value.measures.into_iter().map(Into::into).collect(),
            object_count: value.object_count,
        }
    }
}

impl From<ObjectAggregateDimensionValue> for ObjectAggregateDimensionRecord {
    fn from(value: ObjectAggregateDimensionValue) -> Self {
        Self {
            field: value.field,
            state: match value.state {
                ObjectAggregateValueState::Value => ObjectAggregateDimensionState::Value,
                ObjectAggregateValueState::Null => ObjectAggregateDimensionState::Null,
                ObjectAggregateValueState::Missing => ObjectAggregateDimensionState::Missing,
                ObjectAggregateValueState::Unavailable => {
                    ObjectAggregateDimensionState::Unavailable
                }
                ObjectAggregateValueState::Unknown => ObjectAggregateDimensionState::Unknown,
                _ => ObjectAggregateDimensionState::Unknown,
            },
            value: value.value,
        }
    }
}

impl From<ObjectAggregateMeasureValue> for ObjectAggregateMeasureRecord {
    fn from(value: ObjectAggregateMeasureValue) -> Self {
        Self {
            field: value.field,
            operation: measure_operation_name(value.operation).to_string(),
            state: match value.state {
                ObjectAggregateMeasureState::Value => ObjectAggregateMeasureStateRecord::Value,
                ObjectAggregateMeasureState::Empty => ObjectAggregateMeasureStateRecord::Empty,
                ObjectAggregateMeasureState::Unknown => ObjectAggregateMeasureStateRecord::Unknown,
                _ => ObjectAggregateMeasureStateRecord::Unknown,
            },
            value_count: value.value_count,
            skipped_count: value.skipped_count,
            value: value.value,
        }
    }
}

fn measure_operation_name(operation: ObjectAggregateMeasureOperation) -> &'static str {
    match operation {
        ObjectAggregateMeasureOperation::Sum => "sum",
        ObjectAggregateMeasureOperation::Average => "average",
        ObjectAggregateMeasureOperation::Min => "min",
        ObjectAggregateMeasureOperation::Max => "max",
        _ => "unknown",
    }
}

fn display_selector(field: &str) -> String {
    if let Some(path) = field.strip_prefix("json_data.") {
        return format!("data.{}", path.replace(',', "."));
    }
    if let Some(key) = field.strip_prefix("computed.shared.") {
        return format!("S:{key}");
    }
    if let Some(key) = field.strip_prefix("computed.personal.") {
        return format!("P:{key}");
    }
    field.to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ObjectAggregateDimensionRecord, ObjectAggregateDimensionState,
        ObjectAggregateMeasureRecord, ObjectAggregateMeasureStateRecord, ObjectAggregateRecord,
    };

    #[test]
    fn semantic_value_flattens_typed_dimensions_and_measures() {
        let record = ObjectAggregateRecord {
            dimensions: vec![
                ObjectAggregateDimensionRecord {
                    field: "json_data.region,zone".to_string(),
                    state: ObjectAggregateDimensionState::Value,
                    value: Some(json!("eu-west")),
                },
                ObjectAggregateDimensionRecord {
                    field: "computed.shared.risk".to_string(),
                    state: ObjectAggregateDimensionState::Unavailable,
                    value: None,
                },
            ],
            measures: vec![ObjectAggregateMeasureRecord {
                field: "json_data.metrics,latency_ms".to_string(),
                operation: "average".to_string(),
                state: ObjectAggregateMeasureStateRecord::Value,
                value_count: 3,
                skipped_count: 1,
                value: Some(json!(12.5)),
            }],
            object_count: 4,
        };

        let semantic = record.semantic_value().expect("semantic value");

        assert_eq!(semantic["data.region.zone"], json!("eu-west"));
        assert_eq!(semantic["S:risk"], json!("<unavailable>"));
        assert_eq!(semantic["average:data.metrics.latency_ms"], json!(12.5));
        assert_eq!(semantic["object_count"], json!(4));
        assert_eq!(semantic["measures"][0]["skipped_count"], json!(1));
    }

    #[test]
    fn semantic_value_makes_empty_and_missing_states_visible() {
        let record = ObjectAggregateRecord {
            dimensions: vec![ObjectAggregateDimensionRecord {
                field: "json_data.owner".to_string(),
                state: ObjectAggregateDimensionState::Missing,
                value: None,
            }],
            measures: vec![ObjectAggregateMeasureRecord {
                field: "json_data.load".to_string(),
                operation: "max".to_string(),
                state: ObjectAggregateMeasureStateRecord::Empty,
                value_count: 0,
                skipped_count: 2,
                value: None,
            }],
            object_count: 2,
        };

        let semantic = record.semantic_value().expect("semantic value");

        assert_eq!(semantic["data.owner"], json!("<missing>"));
        assert_eq!(semantic["max:data.load"], json!("<empty>"));
    }
}
