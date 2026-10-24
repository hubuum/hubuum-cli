use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

use crate::error::PipelineError;
use crate::model::{validate_projection_terms, OutputEnvelope, OutputShape, ProjectTerm};
use crate::selector::{
    remove_addressed, remove_matches, select_values, AddressStep, Selector, SelectorToken,
};
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
    let droppers = terms
        .iter()
        .filter(|term| term.is_drop())
        .map(ProjectTerm::selector)
        .collect::<Vec<_>>();
    if keepers.is_empty() {
        let mut projected = value.clone();
        remove_matches(&mut projected, &droppers);
        return projected;
    }
    let dropped = droppers
        .iter()
        .flat_map(|selector| selector.addressed(value))
        .map(|selected| selected.address)
        .collect::<HashSet<_>>();
    let mut object = Map::new();
    for term in keepers {
        let selected = term.selector().addressed(value);
        let had_matches = !selected.is_empty();
        let selected = selected
            .into_iter()
            .filter_map(|selected| {
                if dropped
                    .iter()
                    .any(|address| selected.address.starts_with(address))
                {
                    return None;
                }
                let relative_drops = dropped
                    .iter()
                    .filter_map(|address| {
                        address
                            .strip_prefix(selected.address.as_slice())
                            .map(<[AddressStep]>::to_vec)
                    })
                    .collect();
                let mut item = selected.value.clone();
                remove_addressed(&mut item, &relative_drops);
                Some(item)
            })
            .collect::<Vec<_>>();
        if had_matches && selected.is_empty() {
            continue;
        }
        let projected = match selected.len() {
            0 => Value::Null,
            1 => selected.into_iter().next().expect("one value"),
            _ => Value::Array(selected),
        };
        object.insert(term.output_name().to_string(), projected);
    }
    let mut projected = Value::Object(object);
    // Aliases may introduce paths that did not exist in the original document.
    // Apply only those remaining drop selectors to avoid removing shifted indexes twice.
    let aliases = droppers
        .into_iter()
        .filter(|selector| selector.addressed(value).is_empty())
        .collect::<Vec<_>>();
    remove_matches(&mut projected, &aliases);
    projected
}

fn project_group_rows(
    envelope: OutputEnvelope,
    terms: &[ProjectTerm],
) -> Result<OutputEnvelope, PipelineError> {
    let groups = array_values(&envelope.value)?;
    validate_group_aliases(&groups, terms)?;
    let groups = groups
        .into_iter()
        .filter_map(|group| {
            let summary = group_summary_row(&group)?;
            replace_group_summary(group, project_value(&summary, terms))
        })
        .collect::<Vec<_>>();
    Ok(OutputEnvelope::groups(groups, output_columns(terms)))
}

fn validate_group_aliases(groups: &[Value], terms: &[ProjectTerm]) -> Result<(), PipelineError> {
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
    Ok(())
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

/// Explicit row-producing projection; fanout columns join by source address.
pub(crate) fn project_rows_envelope(
    envelope: OutputEnvelope,
    terms: &[ProjectTerm],
) -> Result<OutputEnvelope, PipelineError> {
    validate_projection_terms(terms)?;
    if terms.iter().any(ProjectTerm::is_drop) {
        return Err(PipelineError::Pipe(
            "P ROWS does not accept drop terms; drop fields in a preceding P stage".into(),
        ));
    }
    let grouped = envelope.shape == OutputShape::Groups;
    let input = match envelope.shape {
        OutputShape::Empty => Vec::new(),
        OutputShape::Detail | OutputShape::Message => vec![envelope.value],
        _ => array_values(&envelope.value)?,
    };
    if grouped {
        validate_group_aliases(&input, terms)?;
    }
    let mut output = Vec::new();
    for value in input {
        if grouped {
            if let Some(summary) = group_summary_row(&value) {
                for row in project_rows(&summary, terms) {
                    if let Some(group) = replace_group_summary(value.clone(), row) {
                        output.push(group);
                    }
                }
            }
        } else {
            output.extend(project_rows(&value, terms));
        }
    }
    Ok(if grouped {
        OutputEnvelope::groups(output, output_columns(terms))
    } else {
        OutputEnvelope::rows(output, output_columns(terms))
    })
}

fn project_rows(value: &Value, terms: &[ProjectTerm]) -> Vec<Value> {
    let (dynamic, fixed): (Vec<_>, Vec<_>) = terms.iter().cloned().partition(|term| {
        term.selector()
            .tokens()
            .iter()
            .any(|token| matches!(token, SelectorToken::All | SelectorToken::Slice(_, _)))
    });
    if dynamic.is_empty() {
        return vec![project_value(value, terms)];
    }
    let static_fields = if fixed.is_empty() {
        Map::new()
    } else {
        project_value(value, &fixed)
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
    let mut rows = Vec::<Map<String, Value>>::new();
    let mut addresses = HashMap::new();
    for term in dynamic {
        for selected in term.selector().addressed(value) {
            let mut address = selected.address;
            if let Some(last_index) = address
                .iter()
                .rposition(|step| matches!(step, AddressStep::Index(_)))
            {
                address.truncate(last_index + 1);
            }
            let index = *addresses.entry(address).or_insert_with(|| {
                rows.push(static_fields.clone());
                rows.len() - 1
            });
            rows[index].insert(term.output_name().to_string(), selected.value.clone());
        }
    }
    rows.into_iter().map(Value::Object).collect()
}
