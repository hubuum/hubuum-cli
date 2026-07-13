use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anstream::{AutoStream, ColorChoice};
use dirs::home_dir;
use hubuum_filter::{group_summary_rows, scalar_text, select_values, OutputShape};
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
        match envelope.shape {
            OutputShape::Rows | OutputShape::Values | OutputShape::Lines => {
                let values = envelope.value.as_array().ok_or_else(|| {
                    AppError::ParseError("each: semantic output is not an array".to_string())
                })?;
                items.extend(values.iter().map(|value| SemanticItem {
                    value: value.clone(),
                    source_shape: envelope.shape,
                    columns: &envelope.columns,
                }));
            }
            OutputShape::Detail | OutputShape::Message => {
                items.push(SemanticItem {
                    value: envelope.value.clone(),
                    source_shape: envelope.shape,
                    columns: &envelope.columns,
                });
            }
            OutputShape::Groups => {
                // Store grouped summaries for per-item redirects so templates can use
                // group and aggregate field names without exposing member rows.
                items.extend(
                    group_summary_rows(&envelope.value)
                        .into_iter()
                        .map(|value| SemanticItem {
                            value,
                            source_shape: OutputShape::Rows,
                            columns: &envelope.columns,
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

        Ok(Self { template })
    }

    fn path_for(&self, value: &Value, number: usize) -> Result<PathBuf, AppError> {
        let mut path = String::new();
        let mut rest = self.template.as_str();
        while let Some(start) = rest.find('{') {
            path.push_str(&rest[..start]);
            let after_start = &rest[start + 1..];
            let Some(end) = after_start.find('}') else {
                return Err(AppError::ParseError(
                    "each: redirect template has an unclosed placeholder".to_string(),
                ));
            };
            let placeholder = &after_start[..end];
            let replacement = if placeholder == "n" {
                number.to_string()
            } else {
                field_placeholder(value, placeholder)?
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

fn field_placeholder(value: &Value, selector: &str) -> Result<String, AppError> {
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

fn select_placeholder_values<'a>(value: &'a Value, selector: &str) -> Vec<&'a Value> {
    if selector == "value" && !value.is_object() {
        return vec![value];
    }

    if let Value::Object(object) = value {
        if let Some(value) = object.get(selector) {
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
    let mut candidate = None;
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
                    candidate = Some((index, operator_len));
                }
            }
            _ => {}
        }
    }

    candidate
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
    use hubuum_filter::OutputEnvelope;
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
