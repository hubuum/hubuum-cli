use serde_json::{to_string, Map, Number, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use crate::equality::json_equality_key;
use crate::error::PipelineError;
use crate::model::{
    validate_group_keys, AggregateFunction, AggregateRequest, GroupKey, NullOrder, OutputEnvelope,
    OutputShape, SortCast, SortDirection, SortKey, SortReduction, SortSpec,
};
use crate::predicate::ValueCast;
use crate::selector::{scalar_text, select_values, Selector};
use crate::value_cast::{cast_value, CastValue};
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

pub(crate) fn sort_whole_envelope(
    envelope: OutputEnvelope,
    descending: bool,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values | OutputShape::Groups => {
            let mut values = array_values(&envelope.value)?;
            values.sort_by(|left, right| compare_values(left, right, SortCast::Auto));
            if descending {
                values.reverse();
            }
            Ok(OutputEnvelope {
                value: Value::Array(values),
                ..envelope
            })
        }
        OutputShape::Detail | OutputShape::Message | OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic sorting"),
    }
}

pub(crate) fn sort_columns_envelope(
    envelope: OutputEnvelope,
    spec: &SortSpec,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values | OutputShape::Groups => {
            let grouped = envelope.shape == OutputShape::Groups;
            let values = array_values(&envelope.value)?;
            let mut prepared = values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let surface = if grouped {
                        group_summary_row(&value).unwrap_or(Value::Null)
                    } else {
                        value.clone()
                    };
                    let keys = spec
                        .keys()
                        .iter()
                        .enumerate()
                        .map(|(key_index, key)| {
                            prepare_sort_key(&surface, key, key_index + 1, index + 1)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(PreparedRow { index, value, keys })
                })
                .collect::<Result<Vec<_>, PipelineError>>()?;
            prepared.sort_by(|left, right| {
                spec.keys()
                    .iter()
                    .zip(left.keys.iter().zip(&right.keys))
                    .map(|(key, (left, right))| compare_prepared(left, right, key))
                    .find(|ordering| *ordering != Ordering::Equal)
                    .unwrap_or_else(|| left.index.cmp(&right.index))
            });
            Ok(OutputEnvelope {
                value: Value::Array(prepared.into_iter().map(|row| row.value).collect()),
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
    validate_group_keys(keys)?;
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

    let columns = keys.iter().map(|key| key.alias().to_string()).collect();
    Ok(OutputEnvelope::groups(
        groups.into_values().collect(),
        columns,
    ))
}

pub(crate) fn aggregate_envelope(
    envelope: OutputEnvelope,
    request: &AggregateRequest,
) -> Result<OutputEnvelope, PipelineError> {
    if request.is_global() {
        return global_aggregate_envelope(envelope, request);
    }

    if envelope.shape != OutputShape::Groups {
        return Err(PipelineError::Pipe(
            "Pipe stage 'A' requires grouped output from G".to_string(),
        ));
    }
    let spec = request
        .specs()
        .first()
        .expect("grouped aggregate requests contain one spec");

    let alias_exists = envelope.columns.iter().any(|column| column == spec.alias())
        || array_values(&envelope.value)?.iter().any(|group| {
            ["groups", "aggregates"].iter().any(|namespace| {
                group
                    .get(namespace)
                    .and_then(Value::as_object)
                    .is_some_and(|values| values.contains_key(spec.alias()))
            })
        });
    if alias_exists {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage 'A' output name '{}' conflicts with a group key or earlier aggregate",
            spec.alias()
        )));
    }

    let groups = array_values(&envelope.value)?
        .into_iter()
        .map(|mut group| {
            let rows = group
                .get("rows")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let value = aggregate_rows(&rows, spec.function());
            group
                .get_mut("aggregates")
                .and_then(Value::as_object_mut)
                .expect("group aggregates should be an object")
                .insert(spec.alias().to_string(), value);
            group
        })
        .collect::<Vec<_>>();

    let mut columns = envelope.columns;
    columns.push(spec.alias().to_string());
    Ok(OutputEnvelope::groups(groups, columns))
}

fn global_aggregate_envelope(
    envelope: OutputEnvelope,
    request: &AggregateRequest,
) -> Result<OutputEnvelope, PipelineError> {
    let rows = match envelope.shape {
        OutputShape::Empty => Vec::new(),
        OutputShape::Rows | OutputShape::Values => array_values(&envelope.value)?,
        OutputShape::Lines | OutputShape::Detail | OutputShape::Message | OutputShape::Groups => {
            unreachable!("global aggregate input shape was validated")
        }
    };
    let mut record = Map::new();
    for spec in request.specs() {
        record.insert(
            spec.alias().to_string(),
            aggregate_rows(&rows, spec.function()),
        );
    }
    let columns = request
        .specs()
        .iter()
        .map(|spec| spec.alias().to_string())
        .collect();
    Ok(OutputEnvelope::rows(vec![Value::Object(record)], columns))
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
    selector: &Selector,
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
                .flat_map(|group| {
                    group_summary_row(&group)
                        .into_iter()
                        .flat_map(|summary| unroll_row(&summary, selector))
                        .filter_map(move |summary| replace_group_summary(group.clone(), summary))
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
        let mut selected = select_values(row, key.selector())
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
                    key.selector()
                )));
            }
        }

        let mut next = Vec::new();
        for combination in &combinations {
            for value in &selected {
                let mut combination = combination.clone();
                combination.insert(key.alias().to_string(), value.clone());
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
        AggregateFunction::CountSelected(selector) => Value::Number(
            selected_values(rows, selector)
                .filter(|value| !value.is_null())
                .count()
                .into(),
        ),
        AggregateFunction::CountDistinct(selector) => {
            let count = selected_values(rows, selector)
                .filter(|value| !value.is_null())
                .map(json_equality_key)
                .collect::<HashSet<_>>()
                .len();
            Value::Number(count.into())
        }
        AggregateFunction::Sum(selector) => {
            let values = numeric_values(rows, selector).collect::<Vec<_>>();
            if values.is_empty() {
                Value::Null
            } else {
                number_value(values.into_iter().sum())
            }
        }
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

fn selected_values<'a>(
    rows: &'a [Value],
    selector: &'a Selector,
) -> impl Iterator<Item = &'a Value> + 'a {
    rows.iter()
        .flat_map(move |row| select_values(row, selector))
}

fn numeric_values<'a>(rows: &'a [Value], selector: &'a Selector) -> impl Iterator<Item = f64> + 'a {
    selected_values(rows, selector).filter_map(Value::as_f64)
}

fn selected_min_max(rows: &[Value], selector: &Selector, max: bool) -> Value {
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

pub(crate) fn group_summary_row(group: &Value) -> Option<Value> {
    let mut object = Map::new();
    object.extend(group.get("groups")?.as_object()?.clone());
    if let Some(aggregates) = group.get("aggregates").and_then(Value::as_object) {
        object.extend(aggregates.clone());
    }
    Some(Value::Object(object))
}

pub(crate) fn replace_group_summary(mut group: Value, summary: Value) -> Option<Value> {
    let group = group.as_object_mut()?;
    let summary = summary.as_object()?.clone();
    group.insert("groups".to_string(), Value::Object(summary));
    group.insert("aggregates".to_string(), Value::Object(Map::new()));
    Some(Value::Object(group.clone()))
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

fn unroll_row(row: &Value, selector: &Selector) -> Vec<Value> {
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

struct PreparedRow {
    index: usize,
    value: Value,
    keys: Vec<PreparedSortValue>,
}

#[derive(Debug, Clone, PartialEq)]
enum PreparedSortValue {
    Null,
    Auto(Value),
    Cast(CastValue),
}

fn prepare_sort_key(
    value: &Value,
    key: &SortKey,
    key_number: usize,
    row: usize,
) -> Result<PreparedSortValue, PipelineError> {
    let selected = select_values(value, key.selector());
    match key.reduction() {
        SortReduction::First => selected
            .first()
            .map_or(Ok(PreparedSortValue::Null), |value| {
                prepare_sort_value(value, key, key_number, row)
            }),
        SortReduction::Min | SortReduction::Max => {
            let values = selected
                .into_iter()
                .map(|value| prepare_sort_value(value, key, key_number, row))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(values
                .into_iter()
                .filter(|value| !matches!(value, PreparedSortValue::Null))
                .reduce(|left, right| {
                    let ordering = compare_prepared_non_null(&left, &right);
                    let use_right = match key.reduction() {
                        SortReduction::Min => ordering == Ordering::Greater,
                        SortReduction::Max => ordering == Ordering::Less,
                        SortReduction::First => unreachable!("handled before reduction"),
                    };
                    if use_right {
                        right
                    } else {
                        left
                    }
                })
                .unwrap_or(PreparedSortValue::Null))
        }
    }
}

fn prepare_sort_value(
    value: &Value,
    key: &SortKey,
    key_number: usize,
    row: usize,
) -> Result<PreparedSortValue, PipelineError> {
    if value.is_null() {
        return Ok(PreparedSortValue::Null);
    }
    let prepared = match strict_sort_cast(key.cast()) {
        Some(cast) => cast_value(value, cast)
            .map_err(|reason| {
                PipelineError::Pipe(format!(
                    "Pipe stage 'S' sort key {key_number} selector '{}' could not cast AS {} at row {row}: {reason}; offending value {value}",
                    key.selector(), key.cast()
                ))
            })?
            .map_or(PreparedSortValue::Null, PreparedSortValue::Cast),
        None => PreparedSortValue::Auto(value.clone()),
    };
    Ok(prepared)
}

fn strict_sort_cast(cast: SortCast) -> Option<ValueCast> {
    match cast {
        SortCast::String => Some(ValueCast::String),
        SortCast::Number => Some(ValueCast::Number),
        SortCast::Boolean => Some(ValueCast::Boolean),
        SortCast::DateTime => Some(ValueCast::DateTime),
        SortCast::Version => Some(ValueCast::Version),
        SortCast::Natural => Some(ValueCast::Natural),
        SortCast::Ip => Some(ValueCast::Ip),
        SortCast::Auto => None,
    }
}

fn compare_prepared(
    left: &PreparedSortValue,
    right: &PreparedSortValue,
    key: &SortKey,
) -> Ordering {
    match (
        matches!(left, PreparedSortValue::Null),
        matches!(right, PreparedSortValue::Null),
    ) {
        (true, true) => Ordering::Equal,
        (true, false) => match key.null_order() {
            NullOrder::First => Ordering::Less,
            NullOrder::Last => Ordering::Greater,
        },
        (false, true) => match key.null_order() {
            NullOrder::First => Ordering::Greater,
            NullOrder::Last => Ordering::Less,
        },
        (false, false) => {
            let ordering = compare_prepared_non_null(left, right);
            match key.direction() {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            }
        }
    }
}

fn compare_prepared_non_null(left: &PreparedSortValue, right: &PreparedSortValue) -> Ordering {
    match (left, right) {
        (PreparedSortValue::Auto(left), PreparedSortValue::Auto(right)) => {
            compare_values(left, right, SortCast::Auto)
        }
        (PreparedSortValue::Cast(left), PreparedSortValue::Cast(right)) => {
            left.compare(right).unwrap_or(Ordering::Equal)
        }
        (PreparedSortValue::Null, _) | (_, PreparedSortValue::Null) => {
            unreachable!("null ordering is handled before value comparison")
        }
        _ => unreachable!("one sort key always prepares one value representation"),
    }
}

fn compare_values(left: &Value, right: &Value, cast: SortCast) -> Ordering {
    match cast {
        SortCast::Auto => match (left, right) {
            (Value::Number(left), Value::Number(right)) => left
                .as_f64()
                .partial_cmp(&right.as_f64())
                .unwrap_or(Ordering::Equal),
            (Value::String(left), Value::String(right)) => left.cmp(right),
            (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
            _ => sortable_text(left).cmp(&sortable_text(right)),
        },
        SortCast::String
        | SortCast::Number
        | SortCast::Boolean
        | SortCast::DateTime
        | SortCast::Version
        | SortCast::Natural
        | SortCast::Ip => unreachable!("strict casts are prepared before comparison"),
    }
}

fn sortable_text(value: &Value) -> String {
    scalar_text(value).unwrap_or_else(|| value.to_string())
}

fn number_value(value: f64) -> Value {
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}
