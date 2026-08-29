use serde_json::{Map, Value};

use crate::error::PipelineError;
use crate::model::{validate_projection_terms, OutputEnvelope, OutputShape, ProjectTerm};
use crate::selector::{select_values, Selector};
use crate::verbs::array_values;
use crate::verbs::collection::{group_summary_row, replace_group_summary};

pub(crate) fn project_envelope(
    envelope: OutputEnvelope,
    terms: &[ProjectTerm],
) -> Result<OutputEnvelope, PipelineError> {
    validate_projection_terms(terms)?;
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
    selector: &Selector,
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
        OutputShape::Groups => group_summary_rows(&envelope)?
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
    let keepers = terms
        .iter()
        .filter(|term| !term.is_drop())
        .collect::<Vec<_>>();
    let mut projected = if keepers.is_empty() {
        value.clone()
    } else {
        let mut object = Map::new();
        for term in keepers {
            let selected = select_values(value, term.selector());
            let value = match selected.as_slice() {
                [] => Value::Null,
                [single] => (*single).clone(),
                many => Value::Array(many.iter().map(|value| (*value).clone()).collect()),
            };
            object.insert(term.output_name().to_string(), value);
        }
        Value::Object(object)
    };

    for term in terms.iter().filter(|term| term.is_drop()) {
        term.selector().remove_matches(&mut projected);
    }

    projected
}

fn project_group_rows(
    envelope: OutputEnvelope,
    terms: &[ProjectTerm],
) -> Result<OutputEnvelope, PipelineError> {
    let groups = array_values(&envelope.value)?;
    for summary in groups.iter().filter_map(group_summary_row) {
        let Some(summary) = summary.as_object() else {
            continue;
        };
        for alias in terms.iter().filter_map(ProjectTerm::alias) {
            if summary.contains_key(alias) {
                return Err(PipelineError::Pipe(format!(
                    "Pipe stage 'P' alias '{alias}' conflicts with a group or aggregate output name"
                )));
            }
        }
    }
    let groups = groups
        .into_iter()
        .filter_map(|group| {
            let summary = group_summary_row(&group)?;
            replace_group_summary(group, project_value(&summary, terms))
        })
        .collect::<Vec<_>>();
    Ok(OutputEnvelope::groups(groups, output_columns(terms)))
}

fn group_summary_rows(envelope: &OutputEnvelope) -> Result<Vec<Value>, PipelineError> {
    Ok(array_values(&envelope.value)?
        .into_iter()
        .filter_map(|group| group_summary_row(&group))
        .collect())
}

fn output_columns(terms: &[ProjectTerm]) -> Vec<String> {
    terms
        .iter()
        .filter(|term| !term.is_drop())
        .map(|term| term.output_name().to_string())
        .collect()
}
