use std::collections::HashSet;

use serde_json::Value;

use crate::equality::{json_equality_key, JsonEqualityKey};
use crate::error::PipelineError;
use crate::model::{DistinctKey, DistinctSpec, OutputEnvelope, OutputShape};
use crate::selector::select_values;
use crate::value_cast::{cast_value, CastValue};
use crate::verbs::array_values;
use crate::verbs::collection::group_summary_row;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum DistinctIdentity {
    WholeValue(JsonEqualityKey),
    KeyTuple(Vec<SelectedIdentity>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SelectedIdentity {
    Missing,
    Values(Vec<SelectedValue>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SelectedValue {
    Json(JsonEqualityKey),
    Cast(CastValue),
    Null,
}

pub(crate) fn distinct_envelope(
    mut envelope: OutputEnvelope,
    spec: &DistinctSpec,
) -> Result<OutputEnvelope, PipelineError> {
    if envelope.shape == OutputShape::Empty {
        return Ok(envelope);
    }

    let mut seen = HashSet::new();
    let mut retained = Vec::new();
    for (index, value) in array_values(&envelope.value)?.into_iter().enumerate() {
        let visible = if envelope.shape == OutputShape::Groups {
            group_summary_row(&value).ok_or_else(|| {
                PipelineError::Pipe(
                    "Pipe stage 'D' expected each group to contain a visible summary".to_string(),
                )
            })?
        } else {
            value.clone()
        };
        let identity = distinct_identity(&visible, spec, index + 1)?;
        if seen.insert(identity) {
            retained.push(value);
        }
    }
    envelope.value = Value::Array(retained);
    Ok(envelope)
}

fn distinct_identity(
    value: &Value,
    spec: &DistinctSpec,
    row: usize,
) -> Result<DistinctIdentity, PipelineError> {
    if spec.is_whole_value() {
        return Ok(DistinctIdentity::WholeValue(json_equality_key(value)));
    }

    spec.keys()
        .iter()
        .enumerate()
        .map(|(index, key)| selected_identity(value, key, index + 1, row))
        .collect::<Result<Vec<_>, _>>()
        .map(DistinctIdentity::KeyTuple)
}

fn selected_identity(
    value: &Value,
    key: &DistinctKey,
    key_number: usize,
    row: usize,
) -> Result<SelectedIdentity, PipelineError> {
    let selected = select_values(value, key.selector());
    if selected.is_empty() {
        return Ok(SelectedIdentity::Missing);
    }

    let values = selected
        .into_iter()
        .map(|value| match key.cast() {
            None => Ok(SelectedValue::Json(json_equality_key(value))),
            Some(cast) => cast_value(value, cast)
                .map_err(|reason| {
                    PipelineError::Pipe(format!(
                        "Pipe stage 'D' distinct key {key_number} selector '{}' could not cast AS {cast} at row {row}: {reason}; offending value {value}",
                        key.selector()
                    ))
                })
                .map(|value| value.map_or(SelectedValue::Null, SelectedValue::Cast)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SelectedIdentity::Values(values))
}

pub(crate) fn distinct_lines(lines: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    lines
        .into_iter()
        .filter(|line| seen.insert(line.clone()))
        .collect()
}
