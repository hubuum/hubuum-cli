use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::mem::take;

use serde_json::{to_string, Map, Number, Value};

use crate::error::PipelineError;
use crate::model::{
    AggregateFunction, AggregateSpec, GroupKey, OutputEnvelope, OutputShape, SortCast,
};
use crate::selector::{scalar_text, select_values};
use crate::verbs::array_values;

pub fn group_summary_rows(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(group_summary_row)
        .collect()
}

pub(crate) fn limit_envelope(
    envelope: OutputEnvelope,
    count: usize,
    offset: usize,
    from_end: bool,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values | OutputShape::Groups => {
            let values = array_values(&envelope.value)?;
            let values = if from_end {
                let keep_from = values.len().saturating_sub(count);
                values.into_iter().skip(keep_from).collect()
            } else {
                values.into_iter().skip(offset).take(count).collect()
            };
            Ok(OutputEnvelope {
                value: Value::Array(values),
                ..envelope
            })
        }
        OutputShape::Detail | OutputShape::Message => Ok(envelope),
        OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic limiting"),
    }
}

pub(crate) fn count_envelope(envelope: OutputEnvelope) -> Result<OutputEnvelope, PipelineError> {
    if envelope.shape == OutputShape::Groups {
        return Ok(OutputEnvelope::rows(
            group_count_rows(&envelope.value),
            Vec::new(),
        ));
    }

    let count = match envelope.shape {
        OutputShape::Rows | OutputShape::Values => array_values(&envelope.value)?.len(),
        OutputShape::Detail | OutputShape::Message => usize::from(!envelope.is_empty()),
        OutputShape::Empty => 0,
        OutputShape::Lines | OutputShape::Groups => unreachable!("handled above"),
    };
    Ok(OutputEnvelope::values(vec![Value::Number(count.into())]))
}

pub(crate) fn sort_envelope(
    envelope: OutputEnvelope,
    selector: Option<&str>,
    descending: bool,
    cast: SortCast,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values | OutputShape::Groups => {
            let mut values = array_values(&envelope.value)?;
            values.sort_by(|left, right| compare_selected(left, right, selector, descending, cast));
            Ok(OutputEnvelope {
                value: Value::Array(values),
                ..envelope
            })
        }
        OutputShape::Detail | OutputShape::Message | OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic sorting"),
    }
}

pub(crate) fn group_envelope(
    envelope: OutputEnvelope,
    keys: &[GroupKey],
) -> Result<OutputEnvelope, PipelineError> {
    let rows = match envelope.shape {
        OutputShape::Rows | OutputShape::Values => array_values(&envelope.value)?,
        OutputShape::Detail | OutputShape::Message => vec![envelope.value],
        OutputShape::Empty => Vec::new(),
        OutputShape::Groups => return Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic grouping"),
    };

    let mut groups = BTreeMap::<String, Value>::new();
    for row in rows {
        for group_values in group_value_combinations(&row, keys)? {
            let key = to_string(&group_values).unwrap_or_default();
            let group = groups.entry(key).or_insert_with(|| {
                let mut object = Map::new();
                object.insert("groups".to_string(), Value::Object(group_values.clone()));
                object.insert("aggregates".to_string(), Value::Object(Map::new()));
                object.insert("rows".to_string(), Value::Array(Vec::new()));
                Value::Object(object)
            });
            group
                .get_mut("rows")
                .and_then(Value::as_array_mut)
                .expect("group rows should be an array")
                .push(row.clone());
        }
    }

    let columns = keys.iter().map(|key| key.alias.clone()).collect();
    Ok(OutputEnvelope::groups(
        groups.into_values().collect(),
        columns,
    ))
}

pub(crate) fn aggregate_envelope(
    envelope: OutputEnvelope,
    spec: &AggregateSpec,
) -> Result<OutputEnvelope, PipelineError> {
    if envelope.shape != OutputShape::Groups {
        return Err(PipelineError::Pipe(
            "Pipe stage 'A' requires grouped output from G".to_string(),
        ));
    }

    let groups = array_values(&envelope.value)?
        .into_iter()
        .map(|mut group| {
            let rows = group
                .get("rows")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let value = aggregate_rows(&rows, &spec.function);
            group
                .get_mut("aggregates")
                .and_then(Value::as_object_mut)
                .expect("group aggregates should be an object")
                .insert(spec.alias.clone(), value);
            group
        })
        .collect::<Vec<_>>();

    let mut columns = envelope.columns;
    if !columns.iter().any(|column| column == &spec.alias) {
        columns.push(spec.alias.clone());
    }
    Ok(OutputEnvelope::groups(groups, columns))
}

pub(crate) fn collapse_groups(envelope: OutputEnvelope) -> Result<OutputEnvelope, PipelineError> {
    if envelope.shape != OutputShape::Groups {
        return Err(PipelineError::Pipe(
            "Pipe stage 'Z' requires grouped output from G".to_string(),
        ));
    }
    Ok(OutputEnvelope::rows(
        group_summary_rows(&envelope.value),
        Vec::new(),
    ))
}

pub(crate) fn unroll_envelope(
    envelope: OutputEnvelope,
    selector: &str,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .flat_map(|row| unroll_row(&row, selector))
                .collect::<Vec<_>>();
            Ok(OutputEnvelope {
                value: Value::Array(rows),
                ..envelope
            })
        }
        OutputShape::Groups => {
            let groups = array_values(&envelope.value)?
                .into_iter()
                .map(|mut group| {
                    if let Some(rows) = group.get_mut("rows").and_then(Value::as_array_mut) {
                        *rows = take(rows)
                            .into_iter()
                            .flat_map(|row| unroll_row(&row, selector))
                            .collect();
                    }
                    group
                })
                .collect();
            Ok(OutputEnvelope::groups(groups, envelope.columns))
        }
        OutputShape::Detail | OutputShape::Message | OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic unroll"),
    }
}

fn group_value_combinations(
    row: &Value,
    keys: &[GroupKey],
) -> Result<Vec<Map<String, Value>>, PipelineError> {
    let mut combinations = vec![Map::new()];
    for key in keys {
        let mut selected = select_values(row, &key.selector)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if selected.is_empty() {
            selected.push(Value::Null);
        }

        for value in &selected {
            if matches!(value, Value::Array(_) | Value::Object(_)) {
                return Err(PipelineError::Pipe(format!(
                    "Group selector '{}' resolved to a non-scalar value; use [] or [*] to fan out arrays",
                    key.selector
                )));
            }
        }

        let mut next = Vec::new();
        for combination in &combinations {
            for value in &selected {
                let mut combination = combination.clone();
                combination.insert(key.alias.clone(), value.clone());
                next.push(combination);
            }
        }
        combinations = next;
    }
    Ok(combinations)
}

fn aggregate_rows(rows: &[Value], function: &AggregateFunction) -> Value {
    match function {
        AggregateFunction::Count => Value::Number(rows.len().into()),
        AggregateFunction::Sum(selector) => number_value(numeric_values(rows, selector).sum()),
        AggregateFunction::Avg(selector) => {
            let values = numeric_values(rows, selector).collect::<Vec<_>>();
            if values.is_empty() {
                Value::Null
            } else {
                number_value(values.iter().sum::<f64>() / values.len() as f64)
            }
        }
        AggregateFunction::Min(selector) => selected_min_max(rows, selector, false),
        AggregateFunction::Max(selector) => selected_min_max(rows, selector, true),
    }
}

fn numeric_values<'a>(rows: &'a [Value], selector: &'a str) -> impl Iterator<Item = f64> + 'a {
    rows.iter()
        .flat_map(move |row| select_values(row, selector))
        .filter_map(Value::as_f64)
}

fn selected_min_max(rows: &[Value], selector: &str, max: bool) -> Value {
    let mut values = rows
        .iter()
        .flat_map(|row| select_values(row, selector))
        .cloned()
        .collect::<Vec<_>>();
    values.sort_by(|left, right| compare_values(left, right, SortCast::Auto));
    if max {
        values.pop().unwrap_or(Value::Null)
    } else {
        values.into_iter().next().unwrap_or(Value::Null)
    }
}

fn group_summary_row(group: &Value) -> Option<Value> {
    let mut object = Map::new();
    object.extend(group.get("groups")?.as_object()?.clone());
    if let Some(aggregates) = group.get("aggregates").and_then(Value::as_object) {
        object.extend(aggregates.clone());
    }
    Some(Value::Object(object))
}

fn group_count_rows(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|group| {
            let mut row = group_summary_row(group)?;
            if let Value::Object(object) = &mut row {
                let row_count = group
                    .get("rows")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or_default();
                object.insert("count".to_string(), Value::Number(row_count.into()));
            }
            Some(row)
        })
        .collect()
}

fn unroll_row(row: &Value, selector: &str) -> Vec<Value> {
    let selected = select_values(row, selector);
    let items = selected
        .into_iter()
        .flat_map(|value| match value {
            Value::Array(values) => values.iter().collect::<Vec<_>>(),
            value => vec![value],
        })
        .cloned()
        .collect::<Vec<_>>();

    if items.is_empty() {
        return Vec::new();
    }

    items
        .into_iter()
        .map(|item| {
            let mut row = row.clone();
            if let (Value::Object(row), Value::Object(item)) = (&mut row, &item) {
                for (key, value) in item {
                    row.insert(key.clone(), value.clone());
                }
            }
            if let Value::Object(row) = &mut row {
                row.insert(selector.to_string(), item);
            }
            row
        })
        .collect()
}

fn compare_selected(
    left: &Value,
    right: &Value,
    selector: Option<&str>,
    descending: bool,
    cast: SortCast,
) -> Ordering {
    let left = selected_sort_value(left, selector);
    let right = selected_sort_value(right, selector);
    compare_sort_values(left.as_ref(), right.as_ref(), descending, cast)
}

fn selected_sort_value(value: &Value, selector: Option<&str>) -> Option<Value> {
    match selector {
        Some(selector) => select_values(value, selector)
            .first()
            .map(|value| (*value).clone())
            .or_else(|| {
                group_summary_row(value).and_then(|summary| {
                    select_values(&summary, selector)
                        .first()
                        .map(|value| (*value).clone())
                })
            }),
        None => Some(value.clone()),
    }
}

fn compare_sort_values(
    left: Option<&Value>,
    right: Option<&Value>,
    descending: bool,
    cast: SortCast,
) -> Ordering {
    match (is_nullish(left), is_nullish(right)) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let ordering =
                compare_values(left.expect("left value"), right.expect("right value"), cast);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
    }
}

fn is_nullish(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn compare_values(left: &Value, right: &Value, cast: SortCast) -> Ordering {
    match cast {
        SortCast::String => scalar_text(left).cmp(&scalar_text(right)),
        SortCast::Number => number_for_sort(left)
            .partial_cmp(&number_for_sort(right))
            .unwrap_or(Ordering::Equal),
        SortCast::Ip => ip_for_sort(left).cmp(&ip_for_sort(right)),
        SortCast::Auto => match (left, right) {
            (Value::Number(left), Value::Number(right)) => left
                .as_f64()
                .partial_cmp(&right.as_f64())
                .unwrap_or(Ordering::Equal),
            (Value::String(left), Value::String(right)) => left.cmp(right),
            (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
            _ => scalar_text(left).cmp(&scalar_text(right)),
        },
    }
}

fn number_for_sort(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
}

fn ip_for_sort(value: &Value) -> Option<Vec<u8>> {
    value.as_str().map(|value| {
        value
            .split('.')
            .map(|part| part.parse::<u8>().unwrap_or_default())
            .collect()
    })
}

fn number_value(value: f64) -> Value {
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}
