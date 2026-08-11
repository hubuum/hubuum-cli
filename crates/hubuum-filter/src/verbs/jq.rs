use std::iter::once;

use jaq_all::data::{compile, run, Runner};
use jaq_all::jaq_core::unwrap_valr;
use jaq_all::json::read::parse_single;
use jaq_all::load::FileReportsDisp;
use serde_json::{from_str, to_vec, Value};

use crate::error::PipelineError;
use crate::model::{OutputEnvelope, OutputShape};
use crate::JqLimits;

pub(crate) fn validate_jq_expression(expression: &str) -> Result<(), PipelineError> {
    compile(expression).map(|_| ()).map_err(jq_compile_error)
}

pub(crate) fn validate_bounded_jq_expression(
    expression: &str,
    limits: JqLimits,
) -> Result<(), PipelineError> {
    if expression.len() > limits.max_expression_bytes {
        return Err(PipelineError::Jq(format!(
            "expression is {} bytes; limit is {} bytes",
            expression.len(),
            limits.max_expression_bytes
        )));
    }
    const UNBOUNDED_WORDS: &[&str] = &[
        "as",
        "combinations",
        "def",
        "foreach",
        "range",
        "recurse",
        "reduce",
        "repeat",
        "until",
        "walk",
        "while",
    ];
    if expression.contains("..") {
        return Err(PipelineError::Jq(
            "recursive descent '..' is not available in bounded workflow expressions".to_string(),
        ));
    }
    if let Some(word) = UNBOUNDED_WORDS
        .iter()
        .find(|word| contains_identifier(expression, word))
    {
        return Err(PipelineError::Jq(format!(
            "'{word}' is not available in bounded workflow expressions"
        )));
    }
    if expression.contains('*') {
        return Err(PipelineError::Jq(
            "the '*' operator is not available in bounded workflow expressions".to_string(),
        ));
    }
    if expression.match_indices("[]").count() > 1 {
        return Err(PipelineError::Jq(
            "bounded workflow expressions may contain at most one array generator '[]'".to_string(),
        ));
    }
    validate_jq_expression(expression)
}

pub(crate) fn evaluate_bounded_jq(
    value: &Value,
    expression: &str,
    limits: JqLimits,
) -> Result<Value, PipelineError> {
    validate_bounded_jq_expression(expression, limits)?;
    let filter = compile(expression).map_err(jq_compile_error)?;
    let input = to_vec(value)
        .map_err(|err| PipelineError::Jq(format!("serializing input failed: {err}")))?;
    if input.len() > limits.max_input_bytes {
        return Err(PipelineError::Jq(format!(
            "expression input is {} bytes; limit is {} bytes",
            input.len(),
            limits.max_input_bytes
        )));
    }
    let input = parse_single(&input)
        .map_err(|err| PipelineError::Jq(format!("reading input failed: {err}")))?;

    let mut outputs = Vec::new();
    let mut output_bytes = 0_usize;
    run(
        &Runner::default(),
        &filter,
        Default::default(),
        once(Ok::<_, String>(input)),
        PipelineError::Jq,
        |output| {
            if outputs.len() >= limits.max_outputs {
                return Err(PipelineError::Jq(format!(
                    "expression produced more than {} outputs",
                    limits.max_outputs
                )));
            }
            let output = unwrap_valr(output)
                .map_err(|err| PipelineError::Jq(err.to_string()))?
                .to_string();
            output_bytes = output_bytes.saturating_add(output.len());
            if output_bytes > limits.max_output_bytes {
                return Err(PipelineError::Jq(format!(
                    "expression output exceeds {} bytes",
                    limits.max_output_bytes
                )));
            }
            let output = from_str(&output).map_err(|err| {
                PipelineError::Jq(format!("transform produced unsupported JSON: {err}"))
            })?;
            outputs.push(output);
            Ok(())
        },
    )?;
    Ok(collapse_outputs(outputs, output_shape(value, OutputShape::Values)).1)
}

fn contains_identifier(expression: &str, candidate: &str) -> bool {
    expression.match_indices(candidate).any(|(index, _)| {
        let before = expression[..index].chars().next_back();
        let after = expression[index + candidate.len()..].chars().next();
        !before.is_some_and(is_identifier_character) && !after.is_some_and(is_identifier_character)
    })
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

pub(crate) fn jq_envelope(
    mut envelope: OutputEnvelope,
    expression: &str,
) -> Result<OutputEnvelope, PipelineError> {
    let filter = compile(expression).map_err(jq_compile_error)?;
    let input = to_vec(&envelope.value)
        .map_err(|err| PipelineError::Jq(format!("serializing input failed: {err}")))?;
    let input = parse_single(&input)
        .map_err(|err| PipelineError::Jq(format!("reading input failed: {err}")))?;

    let mut outputs = Vec::new();
    run(
        &Runner::default(),
        &filter,
        Default::default(),
        once(Ok::<_, String>(input)),
        PipelineError::Jq,
        |output| {
            let output = unwrap_valr(output)
                .map_err(|err| PipelineError::Jq(err.to_string()))?
                .to_string();
            let output = from_str(&output).map_err(|err| {
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

fn jq_compile_error(reports: impl AsRef<[jaq_all::load::FileReports]>) -> PipelineError {
    let message = reports
        .as_ref()
        .iter()
        .map(|report| FileReportsDisp::new(report).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    PipelineError::Jq(message.trim().to_string())
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

#[cfg(test)]
mod bounded_tests {
    use serde_json::json;

    use super::{evaluate_bounded_jq, validate_bounded_jq_expression};
    use crate::JqLimits;

    fn limits() -> JqLimits {
        JqLimits::new(128, 128, 4, 128)
    }

    #[test]
    fn rejects_unbounded_jq_constructs() {
        for expression in [
            "recurse(.next)",
            "range(0; 10)",
            "def f: f; f",
            ". as $x | $x",
            "[1] * 1000",
            "[.a[], .b[]]",
            "..",
        ] {
            assert!(validate_bounded_jq_expression(expression, limits()).is_err());
        }
    }

    #[test]
    fn caps_expression_input_outputs_and_output_bytes() {
        assert!(evaluate_bounded_jq(&json!("x".repeat(200)), ".", limits()).is_err());
        assert!(evaluate_bounded_jq(&json!([1, 2, 3, 4, 5]), ".[]", limits()).is_err());
        assert!(
            evaluate_bounded_jq(&json!("x".repeat(120)), ". + \"longsuffix\"", limits()).is_err()
        );
    }
}
