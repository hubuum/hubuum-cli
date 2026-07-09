use regex::Regex;
use serde_json::{Map, Value};
use std::cmp::Ordering;

use crate::error::PipelineError;
use crate::model::{OutputEnvelope, OutputShape};
use crate::selector::{
    compact_empty, is_bookkeeping_key, key_paths, scalar_text, select_values, truthy,
};
use crate::verbs::array_values;

#[derive(Debug)]
enum SemanticFilter {
    Quick(Regex),
    PathExists(String),
    PathEquals(String, String),
    PathContains(String, Regex),
    PathCompare(String, CompareOp, String),
    PathNotEquals(String, String),
    PathNotContains(String, Regex),
}

#[derive(Debug, Clone, Copy)]
enum CompareOp {
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

#[derive(Debug, Default)]
struct FilterMatch {
    matched: bool,
    hits: Vec<String>,
}

pub(crate) fn filter_envelope(
    envelope: OutputEnvelope,
    expression: &str,
    invert: bool,
) -> Result<OutputEnvelope, PipelineError> {
    let filter = SemanticFilter::parse(expression)?;

    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let columns = envelope.columns.clone();
            let add_match_column = !invert && filter.is_quick() && !columns.is_empty();
            let match_column = add_match_column.then(|| match_column_name(&columns));
            let rows = array_values(&envelope.value)?
                .into_iter()
                .filter_map(|value| match filter.match_result(&value, &columns) {
                    Ok(result) if result.matched != invert => Some(Ok(match &match_column {
                        Some(column) => with_match_column(value, column, &result.hits),
                        None => value,
                    })),
                    Ok(_) => None,
                    Err(err) => Some(Err(err)),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let output_columns = match match_column {
                Some(column) => {
                    let mut output_columns = columns;
                    output_columns.push(column);
                    output_columns
                }
                None => columns,
            };
            Ok(OutputEnvelope {
                value: Value::Array(rows),
                columns: output_columns,
                ..envelope
            })
        }
        OutputShape::Detail | OutputShape::Message => {
            if filter
                .match_result(&envelope.value, &envelope.columns)?
                .matched
                != invert
            {
                Ok(envelope)
            } else {
                Ok(OutputEnvelope::empty())
            }
        }
        OutputShape::Groups => filter_group_rows(envelope, expression, invert),
        OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic filtering"),
    }
}

pub(crate) fn value_search_envelope(
    envelope: OutputEnvelope,
    pattern: &str,
) -> Result<OutputEnvelope, PipelineError> {
    let regex = Regex::new(pattern)?;
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .filter(|value| value_search_match(value, &regex))
                .collect::<Vec<_>>();
            Ok(OutputEnvelope {
                value: Value::Array(rows),
                ..envelope
            })
        }
        OutputShape::Detail | OutputShape::Message => {
            if value_search_match(&envelope.value, &regex) {
                Ok(envelope)
            } else {
                Ok(OutputEnvelope::empty())
            }
        }
        OutputShape::Groups => filter_group_rows(envelope, pattern, false),
        OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic filtering"),
    }
}

pub(crate) fn key_search_envelope(
    envelope: OutputEnvelope,
    pattern: &str,
) -> Result<OutputEnvelope, PipelineError> {
    let regex = Regex::new(pattern)?;
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .filter_map(|value| key_projection(&value, &regex))
                .collect::<Vec<_>>();
            Ok(OutputEnvelope::rows(rows, Vec::new()))
        }
        OutputShape::Detail | OutputShape::Message => Ok(key_projection(&envelope.value, &regex)
            .map(|value| OutputEnvelope::detail(value, Vec::new()))
            .unwrap_or_else(OutputEnvelope::empty)),
        OutputShape::Groups => filter_group_rows(envelope, pattern, false),
        OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic filtering"),
    }
}

pub(crate) fn truthy_envelope(
    envelope: OutputEnvelope,
    selector: Option<&str>,
) -> Result<OutputEnvelope, PipelineError> {
    match (envelope.shape, selector) {
        (OutputShape::Rows | OutputShape::Values, Some(selector)) => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .filter(|value| select_values(value, selector).into_iter().any(truthy))
                .collect::<Vec<_>>();
            Ok(OutputEnvelope {
                value: Value::Array(rows),
                ..envelope
            })
        }
        (OutputShape::Rows | OutputShape::Values, None) => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .filter_map(compact_empty)
                .collect::<Vec<_>>();
            Ok(OutputEnvelope {
                value: Value::Array(rows),
                ..envelope
            })
        }
        (OutputShape::Detail | OutputShape::Message, Some(selector)) => {
            if select_values(&envelope.value, selector)
                .into_iter()
                .any(truthy)
            {
                Ok(envelope)
            } else {
                Ok(OutputEnvelope::empty())
            }
        }
        (OutputShape::Detail | OutputShape::Message, None) => Ok(compact_empty(envelope.value)
            .map(|value| OutputEnvelope::detail(value, envelope.columns))
            .unwrap_or_else(OutputEnvelope::empty)),
        (OutputShape::Groups, _) => Ok(envelope),
        (OutputShape::Empty, _) => Ok(envelope),
        (OutputShape::Lines, _) => unreachable!("line output is handled before semantic filtering"),
    }
}

impl SemanticFilter {
    fn parse(expression: &str) -> Result<Self, PipelineError> {
        let parts = shlex::split(expression).unwrap_or_else(|| {
            expression
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        });

        if parts.len() == 1 {
            if let Some((field, op, value)) = split_embedded_comparison(&parts[0]) {
                return parse_comparison(field, op, value);
            }
        }

        if parts.len() >= 2 && parts[1] == "exists" {
            return Ok(Self::PathExists(parts[0].clone()));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), "equals" | "=" | "==") {
            return Ok(Self::PathEquals(parts[0].clone(), parts[2..].join(" ")));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), "!=" | "<>") {
            return Ok(Self::PathNotEquals(parts[0].clone(), parts[2..].join(" ")));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), ">" | ">=" | "<" | "<=") {
            return Ok(Self::PathCompare(
                parts[0].clone(),
                parse_compare_op(&parts[1])?,
                parts[2..].join(" "),
            ));
        }
        if parts.len() >= 4
            && parts[1] == "not"
            && matches!(parts[2].as_str(), "equals" | "=" | "==")
        {
            return Ok(Self::PathNotEquals(parts[0].clone(), parts[3..].join(" ")));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), "contains" | "~" | "matches" | "match") {
            return Ok(Self::PathContains(
                parts[0].clone(),
                Regex::new(&parts[2..].join(" "))?,
            ));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), "!~") {
            return Ok(Self::PathNotContains(
                parts[0].clone(),
                Regex::new(&parts[2..].join(" "))?,
            ));
        }
        if parts.len() >= 4
            && parts[1] == "not"
            && matches!(parts[2].as_str(), "contains" | "~" | "matches" | "match")
        {
            return Ok(Self::PathNotContains(
                parts[0].clone(),
                Regex::new(&parts[3..].join(" "))?,
            ));
        }

        Ok(Self::Quick(Regex::new(expression)?))
    }

    fn is_quick(&self) -> bool {
        matches!(self, Self::Quick(_))
    }

    fn match_result(
        &self,
        value: &Value,
        columns: &[String],
    ) -> Result<FilterMatch, PipelineError> {
        match self {
            Self::Quick(regex) => Ok(quick_filter_match(value, columns, regex)),
            Self::PathExists(path) => Ok(FilterMatch {
                matched: !select_values(value, path).is_empty(),
                hits: Vec::new(),
            }),
            Self::PathEquals(path, expected) => Ok(FilterMatch {
                matched: select_values(value, path)
                    .into_iter()
                    .any(|value| scalar_text(value).is_some_and(|actual| actual == *expected)),
                hits: Vec::new(),
            }),
            Self::PathContains(path, regex) => Ok(FilterMatch {
                matched: select_values(value, path)
                    .into_iter()
                    .any(|value| scalar_text(value).is_some_and(|actual| regex.is_match(&actual))),
                hits: Vec::new(),
            }),
            Self::PathCompare(path, op, expected) => Ok(FilterMatch {
                matched: select_values(value, path)
                    .into_iter()
                    .any(|value| compare_expected(value, *op, expected)),
                hits: Vec::new(),
            }),
            Self::PathNotEquals(path, expected) => Ok(FilterMatch {
                matched: {
                    let values = select_values(value, path);
                    !values.is_empty()
                        && values.into_iter().all(|value| {
                            scalar_text(value).is_none_or(|actual| actual != *expected)
                        })
                },
                hits: Vec::new(),
            }),
            Self::PathNotContains(path, regex) => Ok(FilterMatch {
                matched: {
                    let values = select_values(value, path);
                    !values.is_empty()
                        && values.into_iter().all(|value| {
                            scalar_text(value).is_none_or(|actual| !regex.is_match(&actual))
                        })
                },
                hits: Vec::new(),
            }),
        }
    }
}

fn filter_group_rows(
    envelope: OutputEnvelope,
    expression: &str,
    invert: bool,
) -> Result<OutputEnvelope, PipelineError> {
    let groups = array_values(&envelope.value)?
        .into_iter()
        .map(|mut group| {
            let Some(rows) = group.get_mut("rows").and_then(Value::as_array_mut) else {
                return Ok(group);
            };
            let row_envelope = OutputEnvelope::rows(std::mem::take(rows), Vec::new());
            let row_envelope = filter_envelope(row_envelope, expression, invert)?;
            *rows = array_values(&row_envelope.value)?;
            Ok(group)
        })
        .collect::<Result<Vec<_>, PipelineError>>()?;
    Ok(OutputEnvelope::groups(groups, envelope.columns))
}

fn split_embedded_comparison(value: &str) -> Option<(&str, &str, &str)> {
    for op in [">=", "<=", "!=", "==", "=", ">", "<", "~"] {
        if let Some((field, expected)) = value.split_once(op) {
            if !field.is_empty() && !expected.is_empty() {
                return Some((field, op, expected));
            }
        }
    }
    None
}

fn parse_comparison(
    field: &str,
    op: &str,
    expected: &str,
) -> Result<SemanticFilter, PipelineError> {
    match op {
        "=" | "==" => Ok(SemanticFilter::PathEquals(
            field.to_string(),
            expected.to_string(),
        )),
        "!=" => Ok(SemanticFilter::PathNotEquals(
            field.to_string(),
            expected.to_string(),
        )),
        "~" => Ok(SemanticFilter::PathContains(
            field.to_string(),
            Regex::new(expected)?,
        )),
        ">" | ">=" | "<" | "<=" => Ok(SemanticFilter::PathCompare(
            field.to_string(),
            parse_compare_op(op)?,
            expected.to_string(),
        )),
        _ => unreachable!("embedded comparison only supplies known operators"),
    }
}

fn parse_compare_op(value: &str) -> Result<CompareOp, PipelineError> {
    match value {
        ">" => Ok(CompareOp::Greater),
        ">=" => Ok(CompareOp::GreaterOrEqual),
        "<" => Ok(CompareOp::Less),
        "<=" => Ok(CompareOp::LessOrEqual),
        _ => Err(PipelineError::Pipe(format!(
            "Unknown comparison operator '{value}'"
        ))),
    }
}

fn compare_expected(value: &Value, op: CompareOp, expected: &str) -> bool {
    let ordering = match (value.as_f64(), expected.parse::<f64>().ok()) {
        (Some(actual), Some(expected)) => actual.partial_cmp(&expected).unwrap_or(Ordering::Equal),
        _ => scalar_text(value)
            .unwrap_or_default()
            .as_str()
            .cmp(expected),
    };
    match op {
        CompareOp::Greater => ordering == Ordering::Greater,
        CompareOp::GreaterOrEqual => matches!(ordering, Ordering::Greater | Ordering::Equal),
        CompareOp::Less => ordering == Ordering::Less,
        CompareOp::LessOrEqual => matches!(ordering, Ordering::Less | Ordering::Equal),
    }
}

fn quick_filter_match(value: &Value, columns: &[String], regex: &Regex) -> FilterMatch {
    if columns.is_empty() {
        let mut text = String::new();
        collect_key_text(value, &mut text);
        collect_value_text(value, &mut text);
        return FilterMatch {
            matched: regex.is_match(&text),
            hits: Vec::new(),
        };
    }

    let mut hits = Vec::new();
    for column in columns {
        let mut text = String::new();
        for value in select_values(value, column) {
            collect_value_text(value, &mut text);
        }
        if regex.is_match(&text) {
            hits.push(column.clone());
        }
    }

    for (path, _value) in key_paths(value) {
        if regex.is_match(&path) {
            hits.push(format!("key:{path}"));
        }
    }

    FilterMatch {
        matched: !hits.is_empty() || value_search_match(value, regex),
        hits,
    }
}

fn value_search_match(value: &Value, regex: &Regex) -> bool {
    let mut text = String::new();
    collect_value_text(value, &mut text);
    regex.is_match(&text)
}

fn key_projection(value: &Value, regex: &Regex) -> Option<Value> {
    let mut object = Map::new();
    for (path, value) in key_paths(value) {
        let Some(last) = path.rsplit('.').next() else {
            continue;
        };
        if regex.is_match(&path) || regex.is_match(last) {
            object.insert(path, value.clone());
        }
    }
    (!object.is_empty()).then_some(Value::Object(object))
}

fn collect_key_text(value: &Value, text: &mut String) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_bookkeeping_key(key) {
                    continue;
                }
                text.push(' ');
                text.push_str(key);
                collect_key_text(value, text);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_key_text(value, text);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn collect_value_text(value: &Value, text: &mut String) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_bookkeeping_key(key) {
                    continue;
                }
                collect_value_text(value, text);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_value_text(value, text);
            }
        }
        Value::Null => text.push_str(" null"),
        Value::Bool(value) => {
            text.push(' ');
            text.push_str(if *value { "true" } else { "false" });
        }
        Value::Number(value) => {
            text.push(' ');
            text.push_str(&value.to_string());
        }
        Value::String(value) => {
            text.push(' ');
            text.push_str(value);
        }
    }
}

fn match_column_name(columns: &[String]) -> String {
    let base = "Match";
    if !columns.iter().any(|column| column == base) {
        return base.to_string();
    }

    let mut suffix = 2;
    loop {
        let candidate = format!("{base} {suffix}");
        if !columns.iter().any(|column| column == &candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn with_match_column(mut value: Value, column: &str, hits: &[String]) -> Value {
    let text = if hits.is_empty() {
        "value".to_string()
    } else {
        hits.join(", ")
    };

    match &mut value {
        Value::Object(object) => {
            object.insert(column.to_string(), Value::String(text));
            value
        }
        _ => {
            let mut object = Map::new();
            object.insert("value".to_string(), value);
            object.insert(column.to_string(), Value::String(text));
            Value::Object(object)
        }
    }
}
