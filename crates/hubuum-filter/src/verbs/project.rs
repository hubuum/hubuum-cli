use serde_json::{Map, Value};

use crate::error::PipelineError;
use crate::model::{OutputEnvelope, OutputShape, ProjectTerm};
use crate::selector::select_values;
use crate::verbs::array_values;

pub(crate) fn project_envelope(
    envelope: OutputEnvelope,
    terms: &[ProjectTerm],
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .map(|row| project_value(&row, terms))
                .collect::<Vec<_>>();
            Ok(OutputEnvelope::rows(rows, output_columns(terms)))
        }
        OutputShape::Detail | OutputShape::Message => Ok(OutputEnvelope::detail(
            project_value(&envelope.value, terms),
            output_columns(terms),
        )),
        OutputShape::Groups => project_group_rows(envelope, terms),
        OutputShape::Values | OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic projection"),
    }
}

pub(crate) fn value_envelope(
    envelope: OutputEnvelope,
    selector: &str,
) -> Result<OutputEnvelope, PipelineError> {
    let values = match envelope.shape {
        OutputShape::Rows | OutputShape::Values => array_values(&envelope.value)?
            .iter()
            .flat_map(|row| select_values(row, selector))
            .cloned()
            .collect(),
        OutputShape::Detail | OutputShape::Message => select_values(&envelope.value, selector)
            .into_iter()
            .cloned()
            .collect(),
        OutputShape::Groups => group_rows(&envelope)?
            .iter()
            .flat_map(|row| select_values(row, selector))
            .cloned()
            .collect(),
        OutputShape::Empty => Vec::new(),
        OutputShape::Lines => {
            unreachable!("line output is handled before semantic value extraction")
        }
    };
    Ok(OutputEnvelope::values(values))
}

pub(crate) fn project_value(value: &Value, terms: &[ProjectTerm]) -> Value {
    let keepers = terms.iter().filter(|term| !term.drop).collect::<Vec<_>>();
    let mut projected = if keepers.is_empty() {
        value.clone()
    } else {
        let mut object = Map::new();
        for term in keepers {
            let selected = select_values(value, &term.selector);
            let value = match selected.as_slice() {
                [] => Value::Null,
                [single] => (*single).clone(),
                many => Value::Array(many.iter().map(|value| (*value).clone()).collect()),
            };
            object.insert(term.selector.clone(), value);
        }
        Value::Object(object)
    };

    for term in terms.iter().filter(|term| term.drop) {
        drop_path(&mut projected, &term.selector);
    }

    projected
}

fn project_group_rows(
    envelope: OutputEnvelope,
    terms: &[ProjectTerm],
) -> Result<OutputEnvelope, PipelineError> {
    let groups = array_values(&envelope.value)?
        .into_iter()
        .map(|mut group| {
            let mut summary = Map::new();
            if let Some(values) = group.get("groups").and_then(Value::as_object) {
                summary.extend(values.clone());
            }
            if let Some(values) = group.get("aggregates").and_then(Value::as_object) {
                summary.extend(values.clone());
            }

            if let Some(group) = group.as_object_mut() {
                group.insert(
                    "groups".to_string(),
                    project_value(&Value::Object(summary), terms),
                );
                group.insert("aggregates".to_string(), Value::Object(Map::new()));
            }
            group
        })
        .collect::<Vec<_>>();
    Ok(OutputEnvelope::groups(groups, output_columns(terms)))
}

fn group_rows(envelope: &OutputEnvelope) -> Result<Vec<Value>, PipelineError> {
    Ok(array_values(&envelope.value)?
        .into_iter()
        .flat_map(|group| {
            group
                .get("rows")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect())
}

fn output_columns(terms: &[ProjectTerm]) -> Vec<String> {
    terms
        .iter()
        .filter(|term| !term.drop)
        .map(|term| term.selector.clone())
        .collect()
}

fn drop_path(value: &mut Value, selector: &str) {
    let mut parts = selector.split('.').collect::<Vec<_>>();
    if parts.is_empty() {
        return;
    }
    drop_path_parts(value, &mut parts);
}

fn drop_path_parts(value: &mut Value, parts: &mut [&str]) {
    if parts.is_empty() {
        return;
    }

    match value {
        Value::Object(object) if parts.len() == 1 => {
            object.remove(parts[0]);
        }
        Value::Object(object) => {
            if let Some(next) = object.get_mut(parts[0]) {
                drop_path_parts(next, &mut parts[1..]);
            }
        }
        Value::Array(values) => {
            for value in values {
                drop_path_parts(value, parts);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}
