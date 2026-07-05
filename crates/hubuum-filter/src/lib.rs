use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("Pipe error: {0}")]
    Pipe(String),

    #[error("Pipeline parse error: {0}")]
    Parse(String),

    #[error("Regular expression error: {0}")]
    Regex(#[from] regex::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeStage {
    Grep(String),
    Reject(String),
    Head(usize),
    Tail(usize),
    Count,
    SortLines { descending: bool },
    Columns(Vec<String>),
    SortColumn { column: String, descending: bool },
    Value(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputShape {
    Empty,
    Lines,
    Rows,
    Detail,
    Message,
    Values,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputEnvelope {
    pub shape: OutputShape,
    pub value: Value,
    pub columns: Vec<String>,
}

impl OutputEnvelope {
    pub fn empty() -> Self {
        Self {
            shape: OutputShape::Empty,
            value: Value::Array(Vec::new()),
            columns: Vec::new(),
        }
    }

    pub fn lines(lines: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Lines,
            value: Value::Array(lines.into_iter().map(Value::String).collect()),
            columns: Vec::new(),
        }
    }

    pub fn rows(rows: Vec<Value>, columns: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Rows,
            value: Value::Array(rows),
            columns,
        }
    }

    pub fn detail(value: Value, columns: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Detail,
            value,
            columns,
        }
    }

    pub fn message(value: Value) -> Self {
        Self {
            shape: OutputShape::Message,
            value,
            columns: Vec::new(),
        }
    }

    pub fn values(values: Vec<Value>) -> Self {
        Self {
            shape: OutputShape::Values,
            value: Value::Array(values),
            columns: vec!["value".to_string()],
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.value {
            Value::Array(items) => items.is_empty(),
            Value::Null => true,
            _ => false,
        }
    }
}

impl PipeStage {
    pub fn apply_all(
        stages: &[Self],
        mut lines: Vec<String>,
    ) -> Result<Vec<String>, PipelineError> {
        for stage in stages {
            lines = stage.apply(lines)?;
        }
        Ok(lines)
    }

    fn apply(&self, lines: Vec<String>) -> Result<Vec<String>, PipelineError> {
        match self {
            Self::Grep(pattern) => {
                let regex = Regex::new(pattern)?;
                Ok(lines
                    .into_iter()
                    .filter(|line| regex.is_match(line))
                    .collect())
            }
            Self::Reject(pattern) => {
                let regex = Regex::new(pattern)?;
                Ok(lines
                    .into_iter()
                    .filter(|line| !regex.is_match(line))
                    .collect())
            }
            Self::Head(count) => Ok(lines.into_iter().take(*count).collect()),
            Self::Tail(count) => {
                let keep_from = lines.len().saturating_sub(*count);
                Ok(lines.into_iter().skip(keep_from).collect())
            }
            Self::Count => Ok(vec![lines.len().to_string()]),
            Self::SortLines { descending } => {
                let mut sorted = lines;
                sorted.sort();
                if *descending {
                    sorted.reverse();
                }
                Ok(sorted)
            }
            Self::Columns(_) | Self::SortColumn { .. } | Self::Value(_) => Err(
                PipelineError::Pipe("Pipe stage requires structured table output".to_string()),
            ),
        }
    }
}

pub fn apply_pipeline(
    envelope: OutputEnvelope,
    stages: &[PipeStage],
) -> Result<OutputEnvelope, PipelineError> {
    let mut envelope = envelope;
    for stage in stages {
        envelope = apply_semantic_stage(envelope, stage)?;
    }
    Ok(envelope)
}

fn apply_semantic_stage(
    envelope: OutputEnvelope,
    stage: &PipeStage,
) -> Result<OutputEnvelope, PipelineError> {
    if envelope.shape == OutputShape::Lines {
        let lines = envelope
            .value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        return Ok(OutputEnvelope::lines(stage.apply(lines)?));
    }

    match stage {
        PipeStage::Grep(pattern) => filter_envelope(envelope, pattern, false),
        PipeStage::Reject(pattern) => filter_envelope(envelope, pattern, true),
        PipeStage::Head(count) => limit_envelope(envelope, *count, false),
        PipeStage::Tail(count) => limit_envelope(envelope, *count, true),
        PipeStage::Count => count_envelope(envelope),
        PipeStage::SortLines { descending } => sort_envelope(envelope, None, *descending),
        PipeStage::Columns(columns) => project_envelope(envelope, columns),
        PipeStage::SortColumn { column, descending } => {
            sort_envelope(envelope, Some(column), *descending)
        }
        PipeStage::Value(selector) => value_envelope(envelope, selector),
    }
}

fn filter_envelope(
    envelope: OutputEnvelope,
    expression: &str,
    invert: bool,
) -> Result<OutputEnvelope, PipelineError> {
    let filter = SemanticFilter::parse(expression)?;
    let keep = |value: &Value| filter.matches(value).map(|matched| matched != invert);

    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .filter_map(|value| match keep(&value) {
                    Ok(true) => Some(Ok(value)),
                    Ok(false) => None,
                    Err(err) => Some(Err(err)),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(OutputEnvelope {
                value: Value::Array(rows),
                ..envelope
            })
        }
        OutputShape::Detail | OutputShape::Message => {
            if keep(&envelope.value)? {
                Ok(envelope)
            } else {
                Ok(OutputEnvelope::empty())
            }
        }
        OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic filtering"),
    }
}

fn limit_envelope(
    envelope: OutputEnvelope,
    count: usize,
    from_end: bool,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let values = array_values(&envelope.value)?;
            let values = if from_end {
                let keep_from = values.len().saturating_sub(count);
                values.into_iter().skip(keep_from).collect()
            } else {
                values.into_iter().take(count).collect()
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

fn count_envelope(envelope: OutputEnvelope) -> Result<OutputEnvelope, PipelineError> {
    let count = match envelope.shape {
        OutputShape::Rows | OutputShape::Values => array_values(&envelope.value)?.len(),
        OutputShape::Detail | OutputShape::Message => usize::from(!envelope.is_empty()),
        OutputShape::Empty => 0,
        OutputShape::Lines => unreachable!("line output is handled before semantic counting"),
    };
    Ok(OutputEnvelope::values(vec![Value::Number(count.into())]))
}

fn sort_envelope(
    envelope: OutputEnvelope,
    selector: Option<&str>,
    descending: bool,
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows | OutputShape::Values => {
            let mut values = array_values(&envelope.value)?;
            values.sort_by(|left, right| compare_selected(left, right, selector));
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

fn project_envelope(
    envelope: OutputEnvelope,
    columns: &[String],
) -> Result<OutputEnvelope, PipelineError> {
    match envelope.shape {
        OutputShape::Rows => {
            let rows = array_values(&envelope.value)?
                .into_iter()
                .map(|row| project_value(&row, columns))
                .collect::<Vec<_>>();
            Ok(OutputEnvelope::rows(rows, columns.to_vec()))
        }
        OutputShape::Detail | OutputShape::Message => Ok(OutputEnvelope::detail(
            project_value(&envelope.value, columns),
            columns.to_vec(),
        )),
        OutputShape::Values | OutputShape::Empty => Ok(envelope),
        OutputShape::Lines => unreachable!("line output is handled before semantic projection"),
    }
}

fn value_envelope(
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
        OutputShape::Empty => Vec::new(),
        OutputShape::Lines => {
            unreachable!("line output is handled before semantic value extraction")
        }
    };
    Ok(OutputEnvelope::values(values))
}

#[derive(Debug)]
enum SemanticFilter {
    Quick(Regex),
    PathExists(String),
    PathEquals(String, String),
    PathContains(String, Regex),
}

impl SemanticFilter {
    fn parse(expression: &str) -> Result<Self, PipelineError> {
        let parts = shlex::split(expression).unwrap_or_else(|| {
            expression
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        });

        if parts.len() >= 2 && parts[1] == "exists" {
            return Ok(Self::PathExists(parts[0].clone()));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), "equals" | "=" | "==") {
            return Ok(Self::PathEquals(parts[0].clone(), parts[2..].join(" ")));
        }
        if parts.len() >= 3 && matches!(parts[1].as_str(), "contains" | "~") {
            return Ok(Self::PathContains(
                parts[0].clone(),
                Regex::new(&parts[2..].join(" "))?,
            ));
        }

        Ok(Self::Quick(Regex::new(expression)?))
    }

    fn matches(&self, value: &Value) -> Result<bool, PipelineError> {
        match self {
            Self::Quick(regex) => Ok(regex.is_match(&value.to_string())),
            Self::PathExists(path) => Ok(!select_values(value, path).is_empty()),
            Self::PathEquals(path, expected) => Ok(select_values(value, path)
                .into_iter()
                .any(|value| scalar_text(value).is_some_and(|actual| actual == *expected))),
            Self::PathContains(path, regex) => Ok(select_values(value, path)
                .into_iter()
                .any(|value| scalar_text(value).is_some_and(|actual| regex.is_match(&actual)))),
        }
    }
}

fn array_values(value: &Value) -> Result<Vec<Value>, PipelineError> {
    value.as_array().cloned().ok_or_else(|| {
        PipelineError::Pipe("Pipe stage expected an array-shaped semantic value".to_string())
    })
}

fn project_value(value: &Value, columns: &[String]) -> Value {
    let mut object = Map::new();
    for column in columns {
        let selected = select_values(value, column);
        let projected = match selected.as_slice() {
            [] => Value::Null,
            [single] => (*single).clone(),
            many => Value::Array(many.iter().map(|value| (*value).clone()).collect()),
        };
        object.insert(column.clone(), projected);
    }
    Value::Object(object)
}

fn compare_selected(left: &Value, right: &Value, selector: Option<&str>) -> Ordering {
    let left = selector
        .and_then(|selector| select_values(left, selector).first().copied())
        .unwrap_or(left);
    let right = selector
        .and_then(|selector| select_values(right, selector).first().copied())
        .unwrap_or(right);
    compare_values(left, right)
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .partial_cmp(&right.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        _ => scalar_text(left).cmp(&scalar_text(right)),
    }
}

fn select_values<'a>(value: &'a Value, selector: &str) -> Vec<&'a Value> {
    let mut current = vec![value];
    for token in selector_tokens(selector) {
        let mut next = Vec::new();
        for value in current {
            match token {
                SelectorToken::Field(field) => {
                    if let Value::Object(object) = value {
                        if let Some(value) = object.get(field) {
                            next.push(value);
                        }
                    }
                }
                SelectorToken::Index(index) => {
                    if let Value::Array(array) = value {
                        if let Some(value) = array.get(index) {
                            next.push(value);
                        }
                    }
                }
                SelectorToken::All => {
                    if let Value::Array(array) = value {
                        next.extend(array);
                    }
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

#[derive(Debug, Clone, Copy)]
enum SelectorToken<'a> {
    Field(&'a str),
    Index(usize),
    All,
}

fn selector_tokens(selector: &str) -> Vec<SelectorToken<'_>> {
    let mut tokens = Vec::new();
    for part in selector.split('.') {
        if part.is_empty() {
            continue;
        }

        let mut rest = part;
        if let Some(bracket) = rest.find('[') {
            let field = &rest[..bracket];
            if !field.is_empty() {
                tokens.push(SelectorToken::Field(field));
            }
            rest = &rest[bracket..];
        } else {
            tokens.push(SelectorToken::Field(rest));
            continue;
        }

        while let Some(inner) = rest.strip_prefix('[') {
            let Some(end) = inner.find(']') else {
                break;
            };
            let index = &inner[..end];
            if index == "*" {
                tokens.push(SelectorToken::All);
            } else if let Ok(index) = index.parse::<usize>() {
                tokens.push(SelectorToken::Index(index));
            }
            rest = &inner[end + 1..];
        }
    }
    tokens
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

pub fn split_pipeline(line: &str) -> Result<(String, Vec<PipeStage>), PipelineError> {
    let parts = split_unquoted_pipes(line);
    let Some(command) = parts.first() else {
        return Ok((String::new(), Vec::new()));
    };

    let stages = parts
        .iter()
        .skip(1)
        .map(|stage| parse_stage(stage.trim()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok((command.trim().to_string(), stages))
}

fn split_unquoted_pipes(line: &str) -> Vec<String> {
    let mut escaped = false;
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut start = 0;
    let mut parts = Vec::new();

    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !single_quoted => escaped = true,
            '\'' if !double_quoted => single_quoted = !single_quoted,
            '"' if !single_quoted => double_quoted = !double_quoted,
            '|' if !single_quoted && !double_quoted => {
                parts.push(line[start..index].to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(line[start..].to_string());
    parts
}

fn parse_stage(stage: &str) -> Result<PipeStage, PipelineError> {
    if stage.is_empty() {
        return Err(PipelineError::Pipe("Empty pipe stage".to_string()));
    }

    let Some(parts) = shlex::split(stage) else {
        return Err(PipelineError::Parse(
            "Parsing pipe stage failed".to_string(),
        ));
    };

    if parts.is_empty() {
        return Err(PipelineError::Pipe("Empty pipe stage".to_string()));
    }

    match parts[0].as_str() {
        "grep" | "F" => pattern_stage(parts[0].as_str(), &parts, PipeStage::Grep),
        "reject" => pattern_stage("reject", &parts, PipeStage::Reject),
        "head" | "L" => count_stage(parts[0].as_str(), &parts, PipeStage::Head),
        "tail" => count_stage("tail", &parts, PipeStage::Tail),
        "count" | "C" => {
            require_arg_count(parts[0].as_str(), &parts, 1)?;
            Ok(PipeStage::Count)
        }
        "columns" | "P" => parse_columns_stage(&parts),
        "sort" | "S" => parse_sort_stage(&parts),
        "VALUE" | "VAL" => pattern_stage(parts[0].as_str(), &parts, PipeStage::Value),
        _ => parse_legacy_stage(stage),
    }
}

fn parse_legacy_stage(stage: &str) -> Result<PipeStage, PipelineError> {
    if let Some(pattern) = stage.strip_prefix('!') {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Err(PipelineError::Pipe(
                "Legacy reject filter requires a regex".to_string(),
            ));
        }
        Ok(PipeStage::Reject(pattern.to_string()))
    } else {
        Ok(PipeStage::Grep(stage.to_string()))
    }
}

fn pattern_stage(
    name: &str,
    parts: &[String],
    build: fn(String) -> PipeStage,
) -> Result<PipeStage, PipelineError> {
    require_arg_count(name, parts, 2)?;
    Ok(build(parts[1].clone()))
}

fn count_stage(
    name: &str,
    parts: &[String],
    build: fn(usize) -> PipeStage,
) -> Result<PipeStage, PipelineError> {
    if parts.len() > 2 {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{name}' accepts at most one count"
        )));
    }
    let count = parts
        .get(1)
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                PipelineError::Pipe(format!(
                    "Pipe stage '{name}' count must be a positive integer"
                ))
            })
        })
        .transpose()?
        .unwrap_or(10);
    Ok(build(count))
}

fn parse_columns_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() < 2 {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            parts[0]
        )));
    }

    let columns = parts
        .iter()
        .skip(1)
        .flat_map(|part| part.split(','))
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if columns.is_empty() {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            parts[0]
        )));
    }

    Ok(PipeStage::Columns(columns))
}

fn parse_sort_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() > 3 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'sort' accepts: sort [line|column] [asc|desc]".to_string(),
        ));
    }

    let target = parts.get(1).map(String::as_str).unwrap_or("line");
    let (target, descending_prefix) = target
        .strip_prefix('!')
        .map(|target| (target, true))
        .unwrap_or((target, false));
    let descending = match parts.get(2).map(String::as_str).unwrap_or("asc") {
        "asc" => false,
        "desc" => true,
        other => {
            return Err(PipelineError::Pipe(format!(
                "Unknown sort direction '{other}'. Use asc or desc"
            )))
        }
    };

    let descending = descending || descending_prefix;

    if target == "line" {
        Ok(PipeStage::SortLines { descending })
    } else {
        Ok(PipeStage::SortColumn {
            column: target.to_string(),
            descending,
        })
    }
}

fn require_arg_count(name: &str, parts: &[String], expected: usize) -> Result<(), PipelineError> {
    if parts.len() != expected {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{name}' expects {} argument(s)",
            expected.saturating_sub(1)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply_pipeline, split_pipeline, OutputEnvelope, PipeStage};
    use serde_json::json;

    #[test]
    fn legacy_regex_pipe_still_parses() {
        let (command, stages) = split_pipeline("object list | alpha").expect("pipeline");
        assert_eq!(command, "object list");
        assert_eq!(stages, vec![PipeStage::Grep("alpha".to_string())]);
    }

    #[test]
    fn quoted_pipes_stay_in_command() {
        let (command, stages) =
            split_pipeline("object list --where name equals 'alpha|beta' | reject beta")
                .expect("pipeline");
        assert_eq!(command, "object list --where name equals 'alpha|beta'");
        assert_eq!(stages, vec![PipeStage::Reject("beta".to_string())]);
    }

    #[test]
    fn multiple_stages_apply_in_order() {
        let (_command, stages) =
            split_pipeline("object list | reject beta | sort line desc | head 2")
                .expect("pipeline");
        let lines = PipeStage::apply_all(
            &stages,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "delta".to_string(),
            ],
        )
        .expect("apply");
        assert_eq!(lines, vec!["gamma".to_string(), "delta".to_string()]);
    }

    #[test]
    fn count_replaces_lines_with_count() {
        let (_command, stages) = split_pipeline("object list | grep a | count").expect("pipeline");
        let lines = PipeStage::apply_all(&stages, vec!["alpha".to_string(), "beta".to_string()])
            .expect("apply");
        assert_eq!(lines, vec!["2".to_string()]);
    }

    #[test]
    fn head_and_tail_limit_lines() {
        let (_command, stages) = split_pipeline("object list | head 3 | tail 2").expect("pipeline");
        let lines = PipeStage::apply_all(
            &stages,
            vec![
                "one".to_string(),
                "two".to_string(),
                "three".to_string(),
                "four".to_string(),
            ],
        )
        .expect("apply");
        assert_eq!(lines, vec!["two".to_string(), "three".to_string()]);
    }

    #[test]
    fn structured_table_stages_parse_but_require_table_output() {
        let (_command, stages) =
            split_pipeline("object list | columns name,id | sort name desc").expect("pipeline");
        assert_eq!(
            stages,
            vec![
                PipeStage::Columns(vec!["name".to_string(), "id".to_string()]),
                PipeStage::SortColumn {
                    column: "name".to_string(),
                    descending: true
                }
            ]
        );
        assert!(PipeStage::apply_all(&stages, vec!["plain text".to_string()]).is_err());
    }

    #[test]
    fn dsl_shorthand_aliases_parse() {
        let (_command, stages) =
            split_pipeline("object list | F active | P name id | S !name | L 5 | C")
                .expect("pipeline");
        assert_eq!(
            stages,
            vec![
                PipeStage::Grep("active".to_string()),
                PipeStage::Columns(vec!["name".to_string(), "id".to_string()]),
                PipeStage::SortColumn {
                    column: "name".to_string(),
                    descending: true
                },
                PipeStage::Head(5),
                PipeStage::Count,
            ]
        );
    }

    #[test]
    fn projection_preserves_quoted_terms_with_spaces() {
        let (_command, stages) =
            split_pipeline("object list | P name 'team owner'").expect("pipeline");
        assert_eq!(
            stages,
            vec![PipeStage::Columns(vec![
                "name".to_string(),
                "team owner".to_string(),
            ])]
        );
    }

    #[test]
    fn semantic_pipeline_filters_projects_sorts_and_limits_rows() {
        let envelope = OutputEnvelope::rows(
            vec![
                json!({"name": "beta", "json_data": {"contact": "ops"}, "age": 2}),
                json!({"name": "alpha", "json_data": {"contact": "noc"}, "age": 3}),
                json!({"name": "retired", "json_data": {"contact": "old"}, "age": 1}),
            ],
            vec!["name".to_string(), "json_data.contact".to_string()],
        );
        let stages = vec![
            PipeStage::Reject("name equals retired".to_string()),
            PipeStage::SortColumn {
                column: "name".to_string(),
                descending: false,
            },
            PipeStage::Columns(vec!["name".to_string(), "json_data.contact".to_string()]),
            PipeStage::Head(1),
        ];

        let transformed = apply_pipeline(envelope, &stages).expect("semantic pipeline");
        assert_eq!(
            transformed.value,
            json!([{"name": "alpha", "json_data.contact": "noc"}])
        );
        assert_eq!(
            transformed.columns,
            vec!["name".to_string(), "json_data.contact".to_string()]
        );
    }

    #[test]
    fn semantic_value_extracts_nested_values_and_count_counts_rows() {
        let envelope = OutputEnvelope::rows(
            vec![
                json!({"name": "alpha", "json_data": {"contacts": [{"email": "a@example.com"}]}}),
                json!({"name": "beta", "json_data": {"contacts": [{"email": "b@example.com"}]}}),
            ],
            vec!["name".to_string()],
        );

        let values = apply_pipeline(
            envelope.clone(),
            &[PipeStage::Value("json_data.contacts[0].email".to_string())],
        )
        .expect("value pipeline");
        assert_eq!(values.value, json!(["a@example.com", "b@example.com"]));

        let count = apply_pipeline(envelope, &[PipeStage::Count]).expect("count pipeline");
        assert_eq!(count.value, json!([2]));
    }

    #[test]
    fn semantic_quick_filter_matches_json_keys_and_values() {
        let envelope = OutputEnvelope::rows(
            vec![
                json!({"name": "alpha", "json_data": {"eko_marker": "no visible value"}}),
                json!({"name": "beta", "json_data": {"marker": "other"}}),
            ],
            vec!["name".to_string()],
        );

        let transformed =
            apply_pipeline(envelope, &[PipeStage::Grep("eko".to_string())]).expect("grep");
        assert_eq!(
            transformed.value,
            json!([{"name": "alpha", "json_data": {"eko_marker": "no visible value"}}])
        );
    }
}
