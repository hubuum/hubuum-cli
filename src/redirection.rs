use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anstream::{AutoStream, ColorChoice};
use dirs::home_dir;
use hubuum_filter::{
    group_summary_rows, scalar_text, select_values, split_pipeline, OutputShape, Selector,
};
use serde_json::Value;
use shlex::split;

use crate::errors::AppError;
use crate::output::{render_semantic_item, OutputSnapshot};
use crate::theme::color_choice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRedirect {
    pub target: RedirectTarget,
    pub append: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectTarget {
    File(PathBuf),
    Each(EachTemplate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EachTemplate {
    template: String,
    selectors: Vec<Option<Selector>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedirectCandidate {
    pub line: String,
    pub redirect: OutputRedirect,
}

pub(crate) fn split_redirect_candidate(line: &str) -> Result<Option<RedirectCandidate>, AppError> {
    let Some((operator_start, operator_len)) = final_redirect_operator(line) else {
        return Ok(None);
    };

    let command = line[..operator_start].trim_end();
    let target = line[operator_start + operator_len..].trim();
    if command.is_empty() {
        return Ok(None);
    }
    if target.is_empty() {
        return Err(AppError::ParseError(
            "Redirect requires a file path".to_string(),
        ));
    }

    let target_parts = split(target)
        .ok_or_else(|| AppError::ParseError("Parsing redirect path failed".to_string()))?;
    if target_parts.len() != 1 {
        return Err(AppError::ParseError(
            "Redirect accepts exactly one file path".to_string(),
        ));
    }

    Ok(Some(RedirectCandidate {
        line: command.to_string(),
        redirect: OutputRedirect {
            target: parse_redirect_target(&target_parts[0])?,
            append: operator_len == 2,
        },
    }))
}

pub(crate) fn redirect_completion_context(line: &str, pos: usize) -> Option<(&str, usize)> {
    let prefix = line.get(..pos)?;
    let (operator_start, operator_len) = final_redirect_operator(prefix)?;
    let command = prefix[..operator_start].trim_end();
    if command.is_empty() {
        return None;
    }

    let target_start = operator_start + operator_len;
    let target = &prefix[target_start..];
    let leading_whitespace = target.len() - target.trim_start().len();
    let replacement_start = target_start + leading_whitespace;
    let completion_prefix = &prefix[replacement_start..];
    if let Some(path_prefix) = completion_prefix.strip_prefix("each:") {
        Some((path_prefix, replacement_start + "each:".len()))
    } else {
        Some((completion_prefix, replacement_start))
    }
}

pub fn write_output(snapshot: &OutputSnapshot, redirect: &OutputRedirect) -> Result<(), AppError> {
    match &redirect.target {
        RedirectTarget::File(path) => write_file(&snapshot.render(), path, redirect.append),
        RedirectTarget::Each(template) => write_each_output(snapshot, template, redirect.append),
    }
}

fn write_file(content: &str, path: &Path, append: bool) -> Result<(), AppError> {
    write_file_with_color_choice(content, path, append, color_choice())
}

fn write_file_with_color_choice(
    content: &str,
    path: &Path,
    append: bool,
    color_choice: ColorChoice,
) -> Result<(), AppError> {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }

    let file = options.open(path)?;
    let mut stream = AutoStream::new(file, color_choice);
    stream.write_all(content.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn write_each_output(
    snapshot: &OutputSnapshot,
    template: &EachTemplate,
    append: bool,
) -> Result<(), AppError> {
    if snapshot.semantic.is_empty() {
        return Err(AppError::ParseError(
            "each: redirects require structured semantic output".to_string(),
        ));
    }

    let items = semantic_items(snapshot)?;
    let mut seen = HashSet::new();
    let mut writes = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let path = template.path_for(&item.value, index + 1)?;
        if !seen.insert(path.clone()) {
            return Err(AppError::ParseError(format!(
                "each: redirect generated duplicate path '{}'",
                path.display()
            )));
        }
        let content = render_semantic_item(
            &item.value,
            item.source_shape,
            item.columns,
            snapshot.render_format,
        )?;
        writes.push((path, content));
    }

    for (path, content) in writes {
        write_file(&content, &path, append)?;
    }

    Ok(())
}

struct SemanticItem<'a> {
    value: Value,
    source_shape: OutputShape,
    columns: &'a [String],
}

fn semantic_items(snapshot: &OutputSnapshot) -> Result<Vec<SemanticItem<'_>>, AppError> {
    let mut items = Vec::new();
    for envelope in &snapshot.semantic {
        match envelope.shape() {
            OutputShape::Rows | OutputShape::Values | OutputShape::Lines => {
                let values = envelope.value().as_array().ok_or_else(|| {
                    AppError::ParseError("each: semantic output is not an array".to_string())
                })?;
                items.extend(values.iter().map(|value| SemanticItem {
                    value: value.clone(),
                    source_shape: envelope.shape(),
                    columns: envelope.columns(),
                }));
            }
            OutputShape::Detail | OutputShape::Message => {
                items.push(SemanticItem {
                    value: envelope.value().clone(),
                    source_shape: envelope.shape(),
                    columns: envelope.columns(),
                });
            }
            OutputShape::Groups => {
                // Store grouped summaries for per-item redirects so templates can use
                // group and aggregate field names without exposing member rows.
                items.extend(
                    group_summary_rows(envelope.value())
                        .into_iter()
                        .map(|value| SemanticItem {
                            value,
                            source_shape: OutputShape::Rows,
                            columns: envelope.columns(),
                        }),
                );
            }
            OutputShape::Empty => {}
        }
    }
    Ok(items)
}

fn parse_redirect_target(target: &str) -> Result<RedirectTarget, AppError> {
    if let Some(template) = target.strip_prefix("each:") {
        return Ok(RedirectTarget::Each(EachTemplate::parse(template)?));
    }
    Ok(RedirectTarget::File(expand_user_path(target)))
}

impl EachTemplate {
    fn parse(template: &str) -> Result<Self, AppError> {
        if template.is_empty() {
            return Err(AppError::ParseError(
                "each: redirect requires a filename template".to_string(),
            ));
        }

        let template = expand_user_template(template);
        let placeholders = placeholders(&template)?;
        if placeholders.is_empty() {
            return Err(AppError::ParseError(
                "each: redirect template requires at least one placeholder".to_string(),
            ));
        }

        let selectors = placeholders
            .iter()
            .map(|placeholder| {
                if *placeholder == "n" {
                    Ok(None)
                } else {
                    Selector::new(*placeholder)
                        .map(Some)
                        .map_err(AppError::from)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            template,
            selectors,
        })
    }

    fn path_for(&self, value: &Value, number: usize) -> Result<PathBuf, AppError> {
        let mut path = String::new();
        let mut rest = self.template.as_str();
        let mut selectors = self.selectors.iter();
        while let Some(start) = rest.find('{') {
            path.push_str(&rest[..start]);
            let after_start = &rest[start + 1..];
            let Some(end) = after_start.find('}') else {
                return Err(AppError::ParseError(
                    "each: redirect template has an unclosed placeholder".to_string(),
                ));
            };
            let selector = selectors
                .next()
                .expect("validated template has one selector entry per placeholder");
            let replacement = match selector {
                None => number.to_string(),
                Some(selector) => field_placeholder(value, selector)?,
            };
            path.push_str(&sanitize_path_value(&replacement));
            rest = &after_start[end + 1..];
        }
        path.push_str(rest);
        Ok(PathBuf::from(path))
    }
}

fn placeholders(template: &str) -> Result<Vec<&str>, AppError> {
    let mut placeholders = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            return Err(AppError::ParseError(
                "each: redirect template has an unclosed placeholder".to_string(),
            ));
        };
        let placeholder = &after_start[..end];
        if placeholder.is_empty() {
            return Err(AppError::ParseError(
                "each: redirect template has an empty placeholder".to_string(),
            ));
        }
        placeholders.push(placeholder);
        rest = &after_start[end + 1..];
    }

    if rest.contains('}') {
        return Err(AppError::ParseError(
            "each: redirect template has an unopened placeholder".to_string(),
        ));
    }

    Ok(placeholders)
}

fn field_placeholder(value: &Value, selector: &Selector) -> Result<String, AppError> {
    let selected = select_placeholder_values(value, selector);
    match selected.as_slice() {
        [value] => scalar_text(value).ok_or_else(|| {
            AppError::ParseError(format!(
                "each: placeholder '{{{selector}}}' resolved to a non-scalar value"
            ))
        }),
        [] => Err(AppError::ParseError(format!(
            "each: placeholder '{{{selector}}}' did not match output item"
        ))),
        _ => Err(AppError::ParseError(format!(
            "each: placeholder '{{{selector}}}' matched multiple values"
        ))),
    }
}

fn select_placeholder_values<'a>(value: &'a Value, selector: &Selector) -> Vec<&'a Value> {
    if selector.as_str() == "value" && !value.is_object() {
        return vec![value];
    }

    if let Value::Object(object) = value {
        if let Some(value) = object.get(selector.as_str()) {
            return vec![value];
        }
    }

    select_values(value, selector)
}

fn sanitize_path_value(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    let sanitized = sanitized.trim();

    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "_".to_string()
    } else {
        sanitized.to_string()
    }
}

fn final_redirect_operator(line: &str) -> Option<(usize, usize)> {
    let mut quote = None;
    let mut escaped = false;
    let mut candidates = Vec::new();
    let mut iter = line.char_indices().peekable();

    while let Some((index, ch)) = iter.next() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if quote != Some('\'') => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '>' if quote.is_none() => {
                if iter.peek().is_some_and(|(_, next)| *next == '=') {
                    continue;
                }
                let operator_len = if iter.peek().is_some_and(|(_, next)| *next == '>') {
                    iter.next();
                    2
                } else {
                    1
                };
                if has_token_boundaries(line, index, operator_len) {
                    candidates.push((index, operator_len));
                }
            }
            _ => {}
        }
    }

    candidates.into_iter().rev().find(|(start, _)| {
        let command = line[..*start].trim_end();
        !command.is_empty() && split_pipeline(command).is_ok()
    })
}

fn has_token_boundaries(line: &str, start: usize, len: usize) -> bool {
    line[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
        && line[start + len..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
}

fn expand_user_template(template: &str) -> String {
    expand_user_path(template).to_string_lossy().to_string()
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }

    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use std::fs::{read_to_string, write};
    use std::path::PathBuf;

    use super::{
        redirect_completion_context, split_redirect_candidate, write_file_with_color_choice,
        write_output, RedirectTarget,
    };
    use crate::output::{OutputSnapshot, RenderFormat};
    use anstream::ColorChoice;
    use hubuum_filter::{OutputEnvelope, Pipeline};
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn splits_trailing_redirects() {
        let candidate = split_redirect_candidate("object list | P Name > out.json")
            .expect("redirect should parse")
            .expect("redirect should exist");

        assert_eq!(candidate.line, "object list | P Name");
        assert_eq!(
            candidate.redirect.target,
            RedirectTarget::File(PathBuf::from("out.json"))
        );
        assert!(!candidate.redirect.append);
    }

    #[test]
    fn splits_append_redirects() {
        let candidate = split_redirect_candidate("object list >> out.json")
            .expect("redirect should parse")
            .expect("redirect should exist");

        assert_eq!(candidate.line, "object list");
        assert!(candidate.redirect.append);
    }

    #[test]
    fn splits_each_redirects() {
        let candidate = split_redirect_candidate("object list > each:hosts/{Name}.json")
            .expect("redirect should parse")
            .expect("redirect should exist");

        assert_eq!(candidate.line, "object list");
        assert!(matches!(candidate.redirect.target, RedirectTarget::Each(_)));
    }

    #[test]
    fn rejects_each_templates_without_placeholders() {
        let err = split_redirect_candidate("object list > each:hosts/output.json")
            .expect_err("placeholder-free each template should fail");

        assert!(err
            .to_string()
            .contains("requires at least one placeholder"));
    }

    #[test]
    fn rejects_malformed_each_placeholder_selectors_during_parsing() {
        let err = split_redirect_candidate("object list > each:hosts/{a[bogus]}.json")
            .expect_err("malformed placeholder selector should fail");

        assert!(err.to_string().contains("Invalid selector 'a[bogus]'"));
    }

    #[test]
    fn ignores_quoted_redirects() {
        assert!(
            split_redirect_candidate("object list --where name equals 'a > b'")
                .expect("redirect parse should succeed")
                .is_none()
        );
    }

    #[test]
    fn ignores_embedded_pipeline_comparisons() {
        for line in [
            "object list | F age>3",
            "object list | F age> 3",
            "object list | F age >3",
        ] {
            assert!(
                split_redirect_candidate(line)
                    .expect("redirect parse should succeed")
                    .is_none(),
                "comparison was treated as a redirect: {line}"
            );
        }
    }

    #[test]
    fn typed_comparisons_are_disambiguated_from_redirects() {
        for line in [
            "object list | F WHERE age > 3",
            "object list | F WHERE age >= 3",
            "object list | reject WHERE age > 3 OR state == \"retired\"",
            "object list | F WHERE age >",
        ] {
            assert!(
                split_redirect_candidate(line)
                    .expect("redirect discovery should succeed")
                    .is_none(),
                "typed comparison was treated as a redirect: {line}"
            );
        }
    }

    #[test]
    fn redirect_after_spaced_typed_comparison_uses_the_complete_predicate_prefix() {
        let candidate = split_redirect_candidate(
            "object list | F WHERE age > 3 AND state == \"active\" > adults.json",
        )
        .expect("redirect discovery should succeed")
        .expect("redirect should be found");

        assert_eq!(
            candidate.line,
            "object list | F WHERE age > 3 AND state == \"active\""
        );
        assert_eq!(
            candidate.redirect.target,
            RedirectTarget::File(PathBuf::from("adults.json"))
        );
    }

    #[test]
    fn each_redirect_after_typed_predicate_remains_supported() {
        let candidate = split_redirect_candidate(
            "object list | F WHERE state == \"active\" > each:hosts/{Name}.json",
        )
        .expect("redirect discovery should succeed")
        .expect("redirect should be found");

        assert!(matches!(candidate.redirect.target, RedirectTarget::Each(_)));
    }

    #[test]
    fn each_redirect_writes_the_rows_retained_by_a_typed_predicate() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("{Name}.json");
        let command = format!(
            "object list | F WHERE age AS num > 3 > each:{}",
            template.display()
        );
        let candidate = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist");
        let (_, pipeline) = hubuum_filter::split_pipeline(&candidate.line)
            .expect("typed pipeline should remain complete");
        let output = Pipeline::from_stages(pipeline)
            .expect("pipeline should validate")
            .apply(OutputEnvelope::rows(
                vec![
                    json!({"Name": "alpha", "age": 2}),
                    json!({"Name": "beta", "age": 4}),
                ],
                vec!["Name".to_string(), "age".to_string()],
            ))
            .expect("typed predicate should apply");
        let snapshot = OutputSnapshot {
            semantic: vec![output],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &candidate.redirect).expect("each redirect should write");

        assert!(!dir.path().join("alpha.json").exists());
        assert!(dir.path().join("beta.json").exists());
    }

    #[test]
    fn redirects_preserve_multi_key_semantic_sort_order() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sorted.json");
        let command = format!(
            "object list | S state asc, rank desc AS num > {}",
            path.display()
        );
        let candidate = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist");
        let (_, stages) = hubuum_filter::split_pipeline(&candidate.line).expect("sort pipeline");
        let output = Pipeline::from_stages(stages)
            .expect("pipeline should validate")
            .apply(OutputEnvelope::rows(
                vec![
                    json!({"name": "alpha", "state": "b", "rank": 1}),
                    json!({"name": "beta", "state": "a", "rank": 1}),
                    json!({"name": "gamma", "state": "a", "rank": 2}),
                ],
                vec!["name".to_string(), "state".to_string(), "rank".to_string()],
            ))
            .expect("sort should apply");
        let lines = serde_json::to_string_pretty(output.value())
            .expect("sorted JSON should render")
            .lines()
            .map(str::to_string)
            .collect();
        let snapshot = OutputSnapshot {
            lines,
            semantic: vec![output],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &candidate.redirect).expect("redirect should write");

        let rendered = read_to_string(path).expect("redirect output");
        let gamma = rendered.find("gamma").expect("gamma row");
        let beta = rendered.find("beta").expect("beta row");
        let alpha = rendered.find("alpha").expect("alpha row");
        assert!(gamma < beta && beta < alpha, "{rendered}");

        let template = dir.path().join("{n}-{name}.json");
        let each = split_redirect_candidate(&format!("object list > each:{}", template.display()))
            .expect("each redirect should parse")
            .expect("each redirect should exist")
            .redirect;
        write_output(&snapshot, &each).expect("each redirect should write");
        assert!(dir.path().join("1-gamma.json").exists());
        assert!(dir.path().join("2-beta.json").exists());
        assert!(dir.path().join("3-alpha.json").exists());
    }

    #[test]
    fn redirects_preserve_strict_ip_sort_order() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("addresses.json");
        let command = format!("object list | S address AS ip > {}", path.display());
        let candidate = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist");
        let (_, stages) = hubuum_filter::split_pipeline(&candidate.line).expect("IP sort pipeline");
        let output = Pipeline::from_stages(stages)
            .expect("pipeline should validate")
            .apply(OutputEnvelope::rows(
                vec![
                    json!({"name": "ten", "address": "10.0.0.10"}),
                    json!({"name": "v6", "address": "2001:db8::1"}),
                    json!({"name": "two", "address": "10.0.0.2"}),
                ],
                vec!["name".to_string(), "address".to_string()],
            ))
            .expect("IP sort should apply");
        let lines = serde_json::to_string_pretty(output.value())
            .expect("sorted JSON should render")
            .lines()
            .map(str::to_string)
            .collect();
        let snapshot = OutputSnapshot {
            lines,
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &candidate.redirect).expect("redirect should write");

        let rendered = read_to_string(path).expect("redirect output");
        let two = rendered.find("10.0.0.2").expect("numeric first IPv4");
        let ten = rendered.find("10.0.0.10").expect("numeric second IPv4");
        let v6 = rendered.find("2001:db8::1").expect("IPv6 row");
        assert!(two < ten && ten < v6, "{rendered}");
    }

    #[test]
    fn each_redirect_placeholders_use_projection_aliases() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("{Host}.json");
        let command = format!(
            "object list | P Name AS Host, address AS IP > each:{}",
            template.display()
        );
        let candidate = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist");
        let (_, stages) = hubuum_filter::split_pipeline(&candidate.line)
            .expect("projection alias pipeline should parse");
        let output = Pipeline::from_stages(stages)
            .expect("pipeline should validate")
            .apply(OutputEnvelope::rows(
                vec![
                    json!({"Name": "alpha", "address": "192.0.2.1"}),
                    json!({"Name": "beta", "address": "192.0.2.2"}),
                ],
                vec!["Name".to_string(), "address".to_string()],
            ))
            .expect("projection should apply");
        let snapshot = OutputSnapshot {
            semantic: vec![output],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &candidate.redirect).expect("each redirect should write");

        for host in ["alpha", "beta"] {
            let rendered = read_to_string(dir.path().join(format!("{host}.json")))
                .expect("aliased output file");
            assert!(rendered.contains("\"Host\""), "{rendered}");
            assert!(rendered.contains("\"IP\""), "{rendered}");
            assert!(!rendered.contains("\"Name\""), "{rendered}");
        }
    }

    #[test]
    fn file_and_each_redirects_use_stable_distinct_rows() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("distinct.json");
        let command = format!("object list | D state > {}", path.display());
        let candidate = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist");
        let (_, stages) =
            hubuum_filter::split_pipeline(&candidate.line).expect("distinct pipeline should parse");
        let output = Pipeline::from_stages(stages)
            .expect("pipeline should validate")
            .apply(OutputEnvelope::rows(
                vec![
                    json!({"name": "alpha", "state": "up"}),
                    json!({"name": "discarded", "state": "up"}),
                    json!({"name": "beta", "state": "down"}),
                ],
                vec!["name".to_string(), "state".to_string()],
            ))
            .expect("distinct should apply");
        let lines = serde_json::to_string_pretty(output.value())
            .expect("distinct JSON should render")
            .lines()
            .map(str::to_string)
            .collect();
        let snapshot = OutputSnapshot {
            lines,
            semantic: vec![output],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &candidate.redirect).expect("file redirect should write");
        let rendered = read_to_string(path).expect("redirect output");
        assert!(rendered.contains("alpha"), "{rendered}");
        assert!(rendered.contains("beta"), "{rendered}");
        assert!(!rendered.contains("discarded"), "{rendered}");

        let template = dir.path().join("{name}.json");
        let each = split_redirect_candidate(&format!("object list > each:{}", template.display()))
            .expect("each redirect should parse")
            .expect("each redirect should exist")
            .redirect;
        write_output(&snapshot, &each).expect("each redirect should write");
        assert!(dir.path().join("alpha.json").exists());
        assert!(dir.path().join("beta.json").exists());
        assert!(!dir.path().join("discarded.json").exists());
    }

    #[test]
    fn file_and_each_redirects_use_global_aggregate_rows() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("inventory.json");
        let command = format!(
            "object list | A GLOBAL count AS Hosts, count_distinct(version) AS Versions > {}",
            path.display()
        );
        let candidate = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist");
        let (_, stages) = hubuum_filter::split_pipeline(&candidate.line)
            .expect("global aggregate pipeline should parse");
        let output = Pipeline::from_stages(stages)
            .expect("pipeline should validate")
            .apply(OutputEnvelope::rows(
                vec![
                    json!({"name": "alpha", "version": "26"}),
                    json!({"name": "beta", "version": "26"}),
                    json!({"name": "gamma", "version": "27"}),
                ],
                vec!["name".to_string(), "version".to_string()],
            ))
            .expect("global aggregate should apply");
        let lines = serde_json::to_string_pretty(output.value())
            .expect("aggregate JSON should render")
            .lines()
            .map(str::to_string)
            .collect();
        let snapshot = OutputSnapshot {
            lines,
            semantic: vec![output],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &candidate.redirect).expect("file redirect should write");
        let rendered = read_to_string(path).expect("redirect output");
        assert!(rendered.contains("\"Hosts\": 3"), "{rendered}");
        assert!(rendered.contains("\"Versions\": 2"), "{rendered}");

        let template = dir.path().join("{Hosts}-{Versions}.json");
        let each = split_redirect_candidate(&format!("object list > each:{}", template.display()))
            .expect("each redirect should parse")
            .expect("each redirect should exist")
            .redirect;
        write_output(&snapshot, &each).expect("each redirect should write");
        assert!(dir.path().join("3-2.json").exists());
    }

    #[test]
    fn file_redirects_apply_color_choice() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("output.txt");
        let styled = "\x1b[31mred\x1b[0m\n";

        write_file_with_color_choice(styled, &path, false, ColorChoice::Auto)
            .expect("auto redirect");
        assert_eq!(read_to_string(&path).expect("auto output"), "red\n");

        write_file_with_color_choice(styled, &path, false, ColorChoice::Never)
            .expect("never redirect");
        assert_eq!(read_to_string(&path).expect("never output"), "red\n");

        write_file_with_color_choice(styled, &path, false, ColorChoice::Always)
            .expect("always redirect");
        assert_eq!(read_to_string(&path).expect("always output"), styled);
    }

    #[test]
    fn completes_redirect_target() {
        assert_eq!(
            redirect_completion_context("object list > ou", "object list > ou".len()),
            Some(("ou", "object list > ".len()))
        );
    }

    #[test]
    fn completes_each_redirect_target_after_prefix() {
        assert_eq!(
            redirect_completion_context(
                "object list > each:hosts/ho",
                "object list > each:hosts/ho".len()
            ),
            Some(("hosts/ho", "object list > each:".len()))
        );
    }

    #[test]
    fn writes_one_file_per_semantic_row_with_field_template() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("{Name}-{n}.json");
        let command = format!("object list --json > each:{}", template.display());
        let redirect = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::rows(
                vec![
                    json!({"Name": "alpha", "os_version": "26"}),
                    json!({"Name": "beta", "os_version": "25"}),
                ],
                vec!["Name".to_string(), "os_version".to_string()],
            )],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        write_output(&snapshot, &redirect).expect("each redirect should write");

        assert_eq!(
            read_to_string(dir.path().join("alpha-1.json")).expect("alpha file"),
            "{\n  \"Name\": \"alpha\",\n  \"os_version\": \"26\"\n}\n"
        );
        assert_eq!(
            read_to_string(dir.path().join("beta-2.json")).expect("beta file"),
            "{\n  \"Name\": \"beta\",\n  \"os_version\": \"25\"\n}\n"
        );
    }

    #[test]
    fn each_redirect_supports_value_placeholders() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("{value}.txt");
        let command = format!("object list | VALUE Name > each:{}", template.display());
        let redirect = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::values(vec![json!("alpha"), json!("beta")])],
            render_format: RenderFormat::Text,
            ..Default::default()
        };

        write_output(&snapshot, &redirect).expect("each redirect should write values");

        assert_eq!(
            read_to_string(dir.path().join("alpha.txt")).expect("alpha file"),
            "alpha\n"
        );
        assert_eq!(
            read_to_string(dir.path().join("beta.txt")).expect("beta file"),
            "beta\n"
        );
    }

    #[test]
    fn each_redirect_append_mode_appends_each_item_file() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("alpha.txt");
        write(&target, "existing\n").expect("seed file");
        let template = dir.path().join("{value}.txt");
        let command = format!("object list | VALUE Name >> each:{}", template.display());
        let redirect = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::values(vec![json!("alpha")])],
            render_format: RenderFormat::Text,
            ..Default::default()
        };

        write_output(&snapshot, &redirect).expect("each redirect should append values");

        assert_eq!(
            read_to_string(target).expect("target file"),
            "existing\nalpha\n"
        );
    }

    #[test]
    fn each_redirect_rejects_duplicate_paths_before_writing() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("{Name}.json");
        let command = format!("object list --json > each:{}", template.display());
        let redirect = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::rows(
                vec![json!({"Name": "alpha"}), json!({"Name": "alpha"})],
                vec!["Name".to_string()],
            )],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        let err = write_output(&snapshot, &redirect).expect_err("duplicate paths should fail");

        assert!(err.to_string().contains("duplicate path"));
        assert!(!dir.path().join("alpha.json").exists());
    }

    #[test]
    fn each_redirect_rejects_missing_and_multi_value_placeholders() {
        let missing = split_redirect_candidate("object list > each:out/{missing}.json")
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let multi = split_redirect_candidate("object list > each:out/{ips[*]}.json")
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::rows(
                vec![json!({"Name": "alpha", "ips": ["one", "two"]})],
                vec!["Name".to_string()],
            )],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        assert!(write_output(&snapshot, &missing)
            .expect_err("missing placeholder should fail")
            .to_string()
            .contains("did not match"));
        assert!(write_output(&snapshot, &multi)
            .expect_err("multi placeholder should fail")
            .to_string()
            .contains("multiple values"));
    }

    #[test]
    fn each_redirect_rejects_non_scalar_placeholders() {
        let redirect = split_redirect_candidate("object list > each:out/{metadata}.json")
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::rows(
                vec![json!({"Name": "alpha", "metadata": {"owner": "ops"}})],
                vec!["Name".to_string()],
            )],
            render_format: RenderFormat::Json,
            ..Default::default()
        };

        assert!(write_output(&snapshot, &redirect)
            .expect_err("non-scalar placeholder should fail")
            .to_string()
            .contains("non-scalar"));
    }

    #[test]
    fn each_redirect_sanitizes_field_values_in_paths() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("{Name}.txt");
        let command = format!("object list > each:{}", template.display());
        let redirect = split_redirect_candidate(&command)
            .expect("redirect should parse")
            .expect("redirect should exist")
            .redirect;
        let snapshot = OutputSnapshot {
            semantic: vec![OutputEnvelope::rows(
                vec![json!({"Name": "../bad/name"})],
                vec!["Name".to_string()],
            )],
            render_format: RenderFormat::Text,
            ..Default::default()
        };

        write_output(&snapshot, &redirect).expect("each redirect should write sanitized path");

        assert!(dir.path().join(".._bad_name.txt").exists());
        assert!(!dir.path().join("bad").exists());
    }
}
