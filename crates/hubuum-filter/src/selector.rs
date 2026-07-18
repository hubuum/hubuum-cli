use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectorToken {
    Field(String),
    Index(isize),
    All,
    Slice(Option<isize>, Option<isize>),
}

pub fn select_values<'a>(value: &'a Value, selector: &str) -> Vec<&'a Value> {
    let mut current = vec![value];
    for token in selector_tokens(selector) {
        let mut next = Vec::new();
        for value in current {
            match token {
                SelectorToken::Field(ref field) => {
                    if let Value::Object(object) = value {
                        if let Some(value) = object.get(field) {
                            next.push(value);
                        }
                    }
                }
                SelectorToken::Index(index) => {
                    if let Value::Array(array) = value {
                        if let Some(index) = resolve_index(array.len(), index) {
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
                        let (start, end) = resolve_slice(array.len(), start, end);
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

pub(crate) fn selector_tokens(selector: &str) -> Vec<SelectorToken> {
    let mut tokens = Vec::new();
    for part in selector.split('.') {
        if part.is_empty() {
            continue;
        }

        let mut rest = part;
        if let Some(bracket) = rest.find('[') {
            let field = &rest[..bracket];
            if !field.is_empty() {
                tokens.push(SelectorToken::Field(field.to_string()));
            }
            rest = &rest[bracket..];
        } else {
            tokens.push(SelectorToken::Field(rest.to_string()));
            continue;
        }

        while let Some(inner) = rest.strip_prefix('[') {
            let Some(end) = inner.find(']') else {
                break;
            };
            let index = &inner[..end];
            if index.is_empty() || index == "*" {
                tokens.push(SelectorToken::All);
            } else if let Some((start, end)) = index.split_once(':') {
                tokens.push(SelectorToken::Slice(parse_bound(start), parse_bound(end)));
            } else if let Ok(index) = index.parse::<isize>() {
                tokens.push(SelectorToken::Index(index));
            }
            rest = &inner[end + 1..];
        }
    }
    tokens
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

fn parse_bound(value: &str) -> Option<isize> {
    if value.is_empty() {
        None
    } else {
        value.parse::<isize>().ok()
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
    use super::select_values;
    use serde_json::json;

    #[test]
    fn selectors_support_array_forms() {
        let value =
            json!({"data": {"interfaces": [{"ipv4": "one"}, {"ipv4": "two"}, {"ipv4": "three"}]}});

        assert_eq!(
            select_values(&value, "data.interfaces[].ipv4")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("one"), json!("two"), json!("three")]
        );
        assert_eq!(
            select_values(&value, "data.interfaces[-1].ipv4")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("three")]
        );
        assert_eq!(
            select_values(&value, "data.interfaces[:2].ipv4")
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
            select_values(&value, "S:load")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!(1.5)]
        );
        assert_eq!(
            select_values(&value, "P:label")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![json!("mine")]
        );
    }
}
