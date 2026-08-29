use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::error::PipelineError;
use crate::settings::PipelineSettings;

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

    pub fn remove_matches(&self, value: &mut Value) {
        remove_selected(value, &self.tokens);
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

fn remove_selected(value: &mut Value, tokens: &[SelectorToken]) {
    let Some((token, remaining)) = tokens.split_first() else {
        return;
    };

    match token {
        SelectorToken::Field(field) => {
            let Value::Object(object) = value else {
                return;
            };
            if remaining.is_empty() {
                object.remove(field);
            } else if let Some(value) = object.get_mut(field) {
                remove_selected(value, remaining);
            }
        }
        SelectorToken::Index(index) => {
            let Value::Array(values) = value else {
                return;
            };
            let Some(index) = resolve_index(values.len(), *index) else {
                return;
            };
            if remaining.is_empty() {
                values.remove(index);
            } else if let Some(value) = values.get_mut(index) {
                remove_selected(value, remaining);
            }
        }
        SelectorToken::All => {
            let Value::Array(values) = value else {
                return;
            };
            if remaining.is_empty() {
                values.clear();
            } else {
                for value in values {
                    remove_selected(value, remaining);
                }
            }
        }
        SelectorToken::Slice(start, end) => {
            let Value::Array(values) = value else {
                return;
            };
            let (start, end) = resolve_slice(values.len(), *start, *end);
            if remaining.is_empty() {
                values.drain(start..end);
            } else {
                for value in &mut values[start..end] {
                    remove_selected(value, remaining);
                }
            }
        }
    }
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

pub(crate) fn key_paths<'a>(
    value: &'a Value,
    settings: &PipelineSettings,
) -> Vec<(String, &'a Value)> {
    let mut paths = Vec::new();
    collect_key_paths(value, "", settings, &mut paths);
    paths
}

fn collect_key_paths<'a>(
    value: &'a Value,
    prefix: &str,
    settings: &PipelineSettings,
    paths: &mut Vec<(String, &'a Value)>,
) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if settings.ignores_search_key(key) {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                paths.push((path.clone(), value));
                collect_key_paths(value, &path, settings, paths);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_key_paths(value, prefix, settings, paths);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
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

    #[test]
    fn selectors_remove_fields_through_fanout_indexes_and_slices() {
        let original = json!({
            "items": [
                {"name": "first", "secret": 1},
                {"name": "second", "secret": 2},
                {"name": "third", "secret": 3}
            ]
        });

        let mut fanout = original.clone();
        selector("items[].secret").remove_matches(&mut fanout);
        assert_eq!(
            fanout,
            json!({"items": [{"name": "first"}, {"name": "second"}, {"name": "third"}]})
        );

        let mut index = original.clone();
        selector("items[-1].secret").remove_matches(&mut index);
        assert_eq!(index["items"][0]["secret"], json!(1));
        assert_eq!(index["items"][1]["secret"], json!(2));
        assert!(index["items"][2].get("secret").is_none());

        let mut slice = original;
        selector("items[:2].secret").remove_matches(&mut slice);
        assert!(slice["items"][0].get("secret").is_none());
        assert!(slice["items"][1].get("secret").is_none());
        assert_eq!(slice["items"][2]["secret"], json!(3));
    }

    #[test]
    fn terminal_array_selectors_remove_only_selected_elements() {
        let mut value = json!({"items": ["zero", "one", "two", "three"]});
        selector("items[1:3]").remove_matches(&mut value);
        assert_eq!(value, json!({"items": ["zero", "three"]}));

        selector("items[-1]").remove_matches(&mut value);
        assert_eq!(value, json!({"items": ["zero"]}));

        selector("items[*]").remove_matches(&mut value);
        assert_eq!(value, json!({"items": []}));
    }

    #[test]
    fn removing_missing_selector_matches_is_harmless() {
        let mut value = json!({"items": [{"name": "first"}]});
        let original = value.clone();

        selector("missing[].secret").remove_matches(&mut value);

        assert_eq!(value, original);
    }
}
