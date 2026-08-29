use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::error::PipelineError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectorToken {
    Field(String),
    Index(isize),
    All,
    Slice(Option<isize>, Option<isize>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    source: String,
    tokens: Vec<SelectorToken>,
}

impl Selector {
    pub fn new(source: impl Into<String>) -> Result<Self, PipelineError> {
        let source = source.into();
        let tokens = parse_selector_tokens(&source)?;
        Ok(Self { source, tokens })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub(crate) fn tokens(&self) -> &[SelectorToken] {
        &self.tokens
    }
}

impl Display for Selector {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.source)
    }
}

impl FromStr for Selector {
    type Err = PipelineError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::new(source)
    }
}

impl AsRef<str> for Selector {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

pub fn select_values<'a>(value: &'a Value, selector: &Selector) -> Vec<&'a Value> {
    let mut current = vec![value];
    for token in selector.tokens() {
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
                        if let Some(index) = resolve_index(array.len(), *index) {
                            if let Some(value) = array.get(index) {
                                next.push(value);
                            }
                        }
                    }
                }
                SelectorToken::All => {
                    if let Value::Array(array) = value {
                        next.extend(array);
                    }
                }
                SelectorToken::Slice(start, end) => {
                    if let Value::Array(array) = value {
                        let (start, end) = resolve_slice(array.len(), *start, *end);
                        next.extend(&array[start..end]);
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

fn parse_selector_tokens(selector: &str) -> Result<Vec<SelectorToken>, PipelineError> {
    if selector.is_empty() {
        return Err(invalid_selector(selector, "selector cannot be empty"));
    }

    let mut tokens = Vec::new();
    let mut position = 0;

    while position < selector.len() {
        let component_start = position;
        if position > 0 && selector[position..].starts_with('[') {
            return Err(invalid_selector(
                selector,
                &format!("empty path component at byte {position}"),
            ));
        }
        while position < selector.len() {
            let ch = selector[position..]
                .chars()
                .next()
                .expect("position is inside selector");
            if matches!(ch, '.' | '[' | ']') {
                break;
            }
            position += ch.len_utf8();
        }

        if position > component_start {
            tokens.push(SelectorToken::Field(
                selector[component_start..position].to_string(),
            ));
        }

        let mut bracket_count = 0;
        while selector[position..].starts_with('[') {
            bracket_count += 1;
            let bracket_start = position;
            position += 1;
            let Some(relative_end) = selector[position..].find(']') else {
                return Err(invalid_selector(
                    selector,
                    &format!("unmatched '[' at byte {bracket_start}"),
                ));
            };
            let bracket_end = position + relative_end;
            let contents = &selector[position..bracket_end];
            tokens.push(parse_bracket(selector, contents, bracket_start)?);
            position = bracket_end + 1;
        }

        if position == component_start && bracket_count == 0 {
            let component = selector[position..]
                .chars()
                .next()
                .expect("position is inside selector");
            let detail = if component == '.' {
                format!("empty path component at byte {position}")
            } else {
                format!("unexpected '{component}' at byte {position}")
            };
            return Err(invalid_selector(selector, &detail));
        }

        if position == selector.len() {
            break;
        }
        if !selector[position..].starts_with('.') {
            return Err(invalid_selector(
                selector,
                &format!("unexpected trailing characters after bracket at byte {position}"),
            ));
        }

        position += 1;
        if position == selector.len() || selector[position..].starts_with('.') {
            return Err(invalid_selector(
                selector,
                &format!("empty path component at byte {position}"),
            ));
        }
    }

    Ok(tokens)
}

fn parse_bracket(
    selector: &str,
    contents: &str,
    position: usize,
) -> Result<SelectorToken, PipelineError> {
    if contents.is_empty() || contents == "*" {
        return Ok(SelectorToken::All);
    }

    if contents.contains(':') {
        if contents.matches(':').count() != 1 {
            return Err(invalid_selector(
                selector,
                &format!("slice at byte {position} must contain exactly one ':'"),
            ));
        }
        let (start, end) = contents
            .split_once(':')
            .expect("slice delimiter was checked");
        return Ok(SelectorToken::Slice(
            parse_bound(selector, start, position)?,
            parse_bound(selector, end, position)?,
        ));
    }

    contents
        .parse::<isize>()
        .map(SelectorToken::Index)
        .map_err(|_| {
            invalid_selector(
                selector,
                &format!(
                    "bracket component '[{contents}]' at byte {position} must be an integer, '*', empty, or a slice"
                ),
            )
        })
}

fn invalid_selector(selector: &str, detail: &str) -> PipelineError {
    PipelineError::Pipe(format!("Invalid selector '{selector}': {detail}"))
}

pub fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
    }
}

pub(crate) fn compact_empty(value: Value) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let object = object
                .into_iter()
                .filter_map(|(key, value)| compact_empty(value).map(|value| (key, value)))
                .collect::<Map<_, _>>();
            (!object.is_empty()).then_some(Value::Object(object))
        }
        Value::Array(values) => {
            let values = values
                .into_iter()
                .filter_map(compact_empty)
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some(Value::Array(values))
        }
        Value::Null => None,
        Value::Bool(false) => None,
        Value::Number(number) if number.as_f64() == Some(0.0) => None,
        Value::String(value) if value.is_empty() => None,
        other => Some(other),
    }
}

pub(crate) fn key_paths(value: &Value) -> Vec<(String, &Value)> {
    let mut paths = Vec::new();
    collect_key_paths(value, "", &mut paths);
    paths
}

fn collect_key_paths<'a>(value: &'a Value, prefix: &str, paths: &mut Vec<(String, &'a Value)>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_bookkeeping_key(key) {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                paths.push((path.clone(), value));
                collect_key_paths(value, &path, paths);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_key_paths(value, prefix, paths);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub(crate) fn is_bookkeeping_key(key: &str) -> bool {
    matches!(key, "created_at" | "updated_at" | "Created" | "Updated")
}

fn parse_bound(
    selector: &str,
    value: &str,
    position: usize,
) -> Result<Option<isize>, PipelineError> {
    if value.is_empty() {
        Ok(None)
    } else {
        value.parse::<isize>().map(Some).map_err(|_| {
            invalid_selector(
                selector,
                &format!("slice bound '{value}' at byte {position} must be an integer"),
            )
        })
    }
}

fn resolve_index(len: usize, index: isize) -> Option<usize> {
    if index >= 0 {
        usize::try_from(index).ok().filter(|index| *index < len)
    } else {
        len.checked_sub(index.unsigned_abs())
    }
}

fn resolve_slice(len: usize, start: Option<isize>, end: Option<isize>) -> (usize, usize) {
    let start = start
        .and_then(|index| resolve_slice_bound(len, index))
        .unwrap_or(0)
        .min(len);
    let end = end
        .and_then(|index| resolve_slice_bound(len, index))
        .unwrap_or(len)
        .min(len);
    if end < start {
        (start, start)
    } else {
        (start, end)
    }
}

fn resolve_slice_bound(len: usize, index: isize) -> Option<usize> {
    if index >= 0 {
        usize::try_from(index).ok()
    } else {
        len.checked_sub(index.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::{select_values, Selector};
    use serde_json::json;

    fn selector(value: &str) -> Selector {
        Selector::new(value).expect("valid selector")
    }

    #[test]
    fn selectors_support_array_forms() {
        let value =
            json!({"data": {"interfaces": [{"ipv4": "one"}, {"ipv4": "two"}, {"ipv4": "three"}]}});

        assert_eq!(
            select_values(&value, &selector("data.interfaces[].ipv4"))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("one"), json!("two"), json!("three")]
        );
        assert_eq!(
            select_values(&value, &selector("data.interfaces[-1].ipv4"))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("three")]
        );
        assert_eq!(
            select_values(&value, &selector("data.interfaces[:2].ipv4"))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("one"), json!("two")]
        );
    }

    #[test]
    fn selectors_treat_computed_scope_colons_as_field_characters() {
        let value = json!({"S:load": 1.5, "P:label": "mine"});

        assert_eq!(
            select_values(&value, &selector("S:load"))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!(1.5)]
        );
        assert_eq!(
            select_values(&value, &selector("P:label"))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("mine")]
        );
    }

    #[test]
    fn selectors_reject_malformed_components() {
        for invalid in [
            "",
            ".a",
            "a.",
            "a..b",
            "a.[0]",
            "a[",
            "a]",
            "a[bogus]",
            "a[:bogus]",
            "a[bogus:]",
            "a[1:2:3]",
            "a[0]tail",
        ] {
            let error = Selector::new(invalid).expect_err("selector should be rejected");
            assert!(error.to_string().contains(invalid));
        }
    }

    #[test]
    fn root_and_chained_array_selectors_are_valid() {
        let value = json!([[{"name": "first"}], [{"name": "second"}]]);
        let selector = selector("[][0].name");

        assert_eq!(
            select_values(&value, &selector)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("first"), json!("second")]
        );
    }
}
