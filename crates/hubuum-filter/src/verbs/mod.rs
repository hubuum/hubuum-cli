pub(crate) mod collection;
pub(crate) mod jq;
pub(crate) mod project;
pub(crate) mod search;

use crate::error::PipelineError;
use serde_json::Value;

pub(crate) fn array_values(value: &Value) -> Result<Vec<Value>, PipelineError> {
    value.as_array().cloned().ok_or_else(|| {
        PipelineError::Pipe("Pipe stage expected an array-shaped semantic value".to_string())
    })
}
