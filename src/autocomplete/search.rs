use hubuum_filter::Predicate;
use hubuum_search::ResourceKind;
use serde_json::{from_value, Value};

use crate::services::CompletionContext;

use super::filters::FilterCompletion;

pub fn search_targets(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    [
        "object",
        "class",
        "collection",
        "audit_event",
        "user",
        "group",
        "service_account",
    ]
    .into_iter()
    .filter(|kind| kind.starts_with(prefix))
    .map(str::to_string)
    .collect()
}

pub(crate) fn search_target(parts: &[String]) -> Option<ResourceKind> {
    let value = parts
        .windows(2)
        .find(|pair| pair[0] == "--target")
        .map(|pair| pair[1].as_str())
        .or_else(|| parts.iter().find_map(|part| part.strip_prefix("--target=")))?;
    from_value(Value::String(value.into())).ok()
}

pub(crate) fn complete_search_predicate(
    ctx: &CompletionContext,
    parts: &[String],
    source: &str,
) -> (usize, Vec<FilterCompletion>) {
    let Some(kind) = search_target(parts) else {
        return (source.len(), Vec::new());
    };
    let (start, candidates) = predicate_candidates(kind, source);
    let prefix = &source[start..];
    let mut candidates = candidates;
    if candidates.iter().any(|value| value == "data.")
        || prefix.starts_with("data.")
        || prefix.starts_with("json_data.")
    {
        if let Some(class) = parts
            .windows(2)
            .find(|pair| pair[0] == "--class")
            .map(|pair| &pair[1])
        {
            candidates.extend(
                ctx.object_data_fields_for_class(class)
                    .into_iter()
                    .map(|field| {
                        if prefix.starts_with("json_data.") {
                            field.replacen("data.", "json_data.", 1)
                        } else {
                            field
                        }
                    }),
            );
        }
    }
    candidates.sort();
    candidates.dedup();
    (
        start,
        candidates
            .into_iter()
            .filter(|value| value.starts_with(prefix))
            .map(|value| FilterCompletion {
                append_whitespace: !value.ends_with('.') && value != "[",
                description: Some("server search predicate".into()),
                value,
            })
            .collect(),
    )
}

fn predicate_candidates(kind: ResourceKind, source: &str) -> (usize, Vec<String>) {
    let tokens = lexical_spans(source);
    let ends_space = source.ends_with(char::is_whitespace);
    let (start, completed) = if ends_space
        || tokens.is_empty()
        || tokens.last().is_some_and(|(a, b)| &source[*a..*b] == "(")
    {
        (source.len(), tokens.as_slice())
    } else {
        let last = tokens.last().unwrap();
        // A completed closing delimiter starts the next grammar position.
        if matches!(&source[last.0..last.1], ")" | "]") {
            return (source.len(), vec![" AND".into(), " OR".into()]);
        }
        (last.0, &tokens[..tokens.len() - 1])
    };
    let words = completed
        .iter()
        .map(|(a, b)| &source[*a..*b])
        .collect::<Vec<_>>();
    let last = words.last().copied();
    let values: Vec<String> = match last {
        None | Some("AND" | "OR" | "NOT" | "(") if !words.ends_with(&["IS", "NOT"]) => {
            let mut fields = kind
                .fields()
                .iter()
                .map(|field| match *field {
                    "json_data" => "data.".into(),
                    "json_schema" => "json_schema.".into(),
                    "metadata" => "metadata.".into(),
                    _ => field.to_string(),
                })
                .collect::<Vec<_>>();
            fields.extend(["NOT".into(), "(".into()]);
            fields
        }
        Some("IS") => vec!["NULL".into(), "NOT".into()],
        Some("NOT") if words.ends_with(&["IS", "NOT"]) => vec!["NULL".into()],
        Some("IN") => vec!["[".into()],
        Some("==" | "=" | "!=" | ">" | ">=" | "<" | "<=") => vec!["true".into(), "false".into()],
        _ => {
            let complete = source[..start].trim();
            // Close surrounding boolean groups solely to recognize a complete test.
            let opened = words.iter().filter(|word| **word == "(").count();
            let closed = words.iter().filter(|word| **word == ")").count();
            let probe = format!("{complete}{}", ")".repeat(opened.saturating_sub(closed)));
            if Predicate::parse(&probe).is_ok() {
                let mut values = vec!["AND".into(), "OR".into()];
                if opened > closed {
                    values.push(")".into());
                }
                values
            } else if last.is_some_and(|field| {
                kind.fields().contains(&field)
                    || field.starts_with("data.")
                    || field.starts_with("json_data.")
                    || field.starts_with("json_schema.")
                    || field.starts_with("metadata.")
            }) {
                vec!["==", "!=", ">", ">=", "<", "<=", "~", "!~", "IN", "IS"]
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            } else {
                Vec::new()
            }
        }
    };
    (start, values)
}

fn lexical_spans(source: &str) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let mut end = start + ch.len_utf8();
        if ch == '"' || ch == '\'' {
            let mut escaped = false;
            for (index, next) in chars.by_ref() {
                end = index + next.len_utf8();
                if next == ch && !escaped {
                    break;
                }
                escaped = next == '\\' && !escaped;
            }
        } else if !"()[],".contains(ch) {
            while let Some(&(index, next)) = chars.peek() {
                if next.is_whitespace() || "()[],".contains(next) {
                    break;
                }
                end = index + next.len_utf8();
                chars.next();
            }
        }
        result.push((start, end));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_follows_boolean_grammar_and_preserves_replacement_spans() {
        for (source, expected) in [
            ("", "name"),
            ("name ", "=="),
            ("name IS ", "NULL"),
            ("name IS NOT ", "NULL"),
            ("name == \"a room\" ", "AND"),
            ("name == \"a room\" AND (", "name"),
            ("NOT ", "name"),
            ("name IN ", "["),
        ] {
            let (start, values) = predicate_candidates(ResourceKind::Object, source);
            assert!(
                values
                    .iter()
                    .any(|value| value == expected && value.starts_with(&source[start..])),
                "{source}: {values:?}"
            );
        }
        let (start, _) = predicate_candidates(ResourceKind::Object, "name == \"røm\" AND data.ne");
        assert_eq!(&"name == \"røm\" AND data.ne"[start..], "data.ne");
        assert!(!predicate_candidates(ResourceKind::Collection, "")
            .1
            .contains(&"email".into()));
    }
}
