use jaq_all::data::{self, Runner};
use jaq_all::jaq_core::unwrap_valr;
use serde_json::Value;

use crate::error::PipelineError;
use crate::model::{OutputEnvelope, OutputShape};

pub(crate) fn jq_envelope(
    mut envelope: OutputEnvelope,
    expression: &str,
) -> Result<OutputEnvelope, PipelineError> {
    let filter = data::compile(expression).map_err(|reports| {
        let message = reports
            .iter()
            .map(|report| jaq_all::load::FileReportsDisp::new(report).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        PipelineError::Jq(message.trim().to_string())
    })?;
    let input = serde_json::to_vec(&envelope.value)
        .map_err(|err| PipelineError::Jq(format!("serializing input failed: {err}")))?;
    let input = jaq_all::json::read::parse_single(&input)
        .map_err(|err| PipelineError::Jq(format!("reading input failed: {err}")))?;

    let mut outputs = Vec::new();
    data::run(
        &Runner::default(),
        &filter,
        Default::default(),
        std::iter::once(Ok::<_, String>(input)),
        PipelineError::Jq,
        |output| {
            let output = unwrap_valr(output)
                .map_err(|err| PipelineError::Jq(err.to_string()))?
                .to_string();
            let output = serde_json::from_str(&output).map_err(|err| {
                PipelineError::Jq(format!("transform produced unsupported JSON: {err}"))
            })?;
            outputs.push(output);
            Ok(())
        },
    )?;

    let previous_shape = envelope.shape;
    let (shape, value) = collapse_outputs(outputs, previous_shape);
    envelope.shape = shape;
    envelope.value = value;
    envelope.columns.clear();
    Ok(envelope)
}

fn collapse_outputs(outputs: Vec<Value>, previous_shape: OutputShape) -> (OutputShape, Value) {
    match outputs.len() {
        0 => (OutputShape::Empty, Value::Array(Vec::new())),
        1 => {
            let value = outputs.into_iter().next().expect("single jq output");
            (output_shape(&value, previous_shape), value)
        }
        _ => {
            let value = Value::Array(outputs);
            (output_shape(&value, previous_shape), value)
        }
    }
}

fn output_shape(value: &Value, previous_shape: OutputShape) -> OutputShape {
    match value {
        Value::Array(items) if items.is_empty() => match previous_shape {
            OutputShape::Rows => OutputShape::Rows,
            _ => OutputShape::Values,
        },
        Value::Array(items) if items.iter().all(Value::is_object) => OutputShape::Rows,
        Value::Array(_) => OutputShape::Values,
        Value::Object(_) => OutputShape::Detail,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => OutputShape::Message,
    }
}
