use std::path::{Path, PathBuf};

use crate::config::{config_key_names, config_value_candidates};
use crate::services::CompletionContext;

pub fn bool(_ctx: &CompletionContext, _prefix: &str, _parts: &[String]) -> Vec<String> {
    vec!["true".to_string(), "false".to_string()]
}

pub fn output_formats(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(&["text", "json", "jsonl", "csv", "tsv"], prefix)
}

pub fn theme_names(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    crate::config::theme_value_candidates()
        .into_iter()
        .filter(|value| value.starts_with(prefix))
        .collect()
}

pub fn task_kinds(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(
        &["import", "report", "export", "reindex", "remotecall"],
        prefix,
    )
}

pub fn task_statuses(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(
        &[
            "queued",
            "validating",
            "running",
            "succeeded",
            "failed",
            "partiallysucceeded",
            "cancelled",
        ],
        prefix,
    )
}

pub fn remote_http_methods(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_values(&["get", "post", "patch", "delete"], prefix)
}

pub fn remote_auth_types(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(&["none", "bearer", "basic", "apikey"], prefix)
}

pub fn remote_subject_types(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_csv_values(
        &[
            "namespace",
            "class",
            "object",
            "class_relation",
            "object_relation",
        ],
        prefix,
    )
}

pub fn remote_subject_kinds(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_values(
        &[
            "namespace",
            "class",
            "object",
            "class_relation",
            "object_relation",
        ],
        prefix,
    )
}

pub fn report_content_types(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_values(
        &["application/json", "text/plain", "text/html", "text/csv"],
        prefix,
    )
}

pub fn search_kinds(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(&["namespace", "class", "object"], prefix)
}

pub fn principal_kinds(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(&["user", "group", "service-account"], prefix)
}

pub fn file_paths(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    file_path_candidates(prefix)
}

const OBJECT_LIST_CLASS_COLUMNS_PREFIX: &str = "output.object_list_class_columns.";
const OBJECT_LIST_CLASS_META_PREFIX: &str = "output.object_list_class_meta.";

pub fn config_keys(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    let mut keys = config_key_names()
        .into_iter()
        .filter(|key| key.starts_with(prefix))
        .map(str::to_string)
        .collect::<Vec<_>>();

    if let Some(class_prefix) = prefix.strip_prefix(OBJECT_LIST_CLASS_COLUMNS_PREFIX) {
        keys.extend(
            ctx.classes(class_prefix)
                .into_iter()
                .map(|class| format!("{OBJECT_LIST_CLASS_COLUMNS_PREFIX}{class}")),
        );
    }
    if let Some(rest) = prefix.strip_prefix(OBJECT_LIST_CLASS_META_PREFIX) {
        if let Some((class_name, alias_prefix)) = rest.split_once('.') {
            let config = crate::config::get_config();
            if let Some(aliases) = config.output.object_list_class_meta.get(class_name) {
                keys.extend(
                    aliases
                        .keys()
                        .filter(|alias| alias.starts_with(alias_prefix))
                        .map(|alias| {
                            format!("{OBJECT_LIST_CLASS_META_PREFIX}{class_name}.{alias}")
                        }),
                );
            }
        } else {
            keys.extend(
                ctx.classes(rest)
                    .into_iter()
                    .map(|class| format!("{OBJECT_LIST_CLASS_META_PREFIX}{class}.")),
            );
        }
    }

    keys.sort();
    keys.dedup();
    keys
}

pub fn config_values(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    if let Some(class_name) = config_key_from_parts(parts)
        .and_then(|key| key.strip_prefix(OBJECT_LIST_CLASS_COLUMNS_PREFIX))
    {
        return object_list_class_column_values(ctx, class_name, prefix);
    }
    if let Some(class_name) = config_key_from_parts(parts)
        .and_then(|key| key.strip_prefix(OBJECT_LIST_CLASS_META_PREFIX))
        .and_then(|rest| rest.split_once('.').map(|(class_name, _)| class_name))
    {
        return object_list_class_column_values(ctx, class_name, prefix);
    }

    config_value_candidates_for_parts(prefix, parts)
}

fn config_value_candidates_for_parts(prefix: &str, parts: &[String]) -> Vec<String> {
    let Some(key) = config_key_from_parts(parts) else {
        return Vec::new();
    };

    config_value_candidates(key)
        .into_iter()
        .filter(|value| value.starts_with(prefix))
        .collect()
}

pub fn object_data_columns(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    ["auto", "preview", "all"]
        .into_iter()
        .filter(|value| value.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

fn file_path_candidates(prefix: &str) -> Vec<String> {
    let (typed_prefix, lookup_prefix) = normalize_path_prefix(prefix);
    let lookup_path = Path::new(&lookup_prefix);
    let ends_with_separator = lookup_prefix.ends_with(std::path::MAIN_SEPARATOR);
    let (lookup_dir, typed_dir, active_prefix) = if ends_with_separator {
        (
            lookup_path.to_path_buf(),
            typed_prefix.clone(),
            String::new(),
        )
    } else {
        let lookup_dir = lookup_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let typed_dir = path_parent_text(&typed_prefix);
        let active_prefix = lookup_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        (lookup_dir, typed_dir, active_prefix)
    };

    let Ok(entries) = std::fs::read_dir(&lookup_dir) else {
        return Vec::new();
    };

    let include_hidden = active_prefix.starts_with('.');
    let mut suggestions = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !include_hidden && name.starts_with('.') {
                return None;
            }
            if !name.starts_with(&active_prefix) {
                return None;
            }

            let is_dir = entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false);
            let mut value = format!("{typed_dir}{name}");
            if is_dir {
                value.push(std::path::MAIN_SEPARATOR);
            }
            Some(shell_escape_path(&value))
        })
        .collect::<Vec<_>>();

    suggestions.sort_by_key(|value| (completion_is_file(value), value.clone()));
    suggestions
}

fn normalize_path_prefix(prefix: &str) -> (String, String) {
    let unescaped = shlex::split(prefix)
        .and_then(|parts| (parts.len() == 1).then(|| parts[0].clone()))
        .unwrap_or_else(|| prefix.replace("\\ ", " "));

    if let Some(rest) = unescaped.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return (
                format!("~/{rest}"),
                Path::new(&home).join(rest).to_string_lossy().to_string(),
            );
        }
    }

    (unescaped.clone(), unescaped)
}

fn path_parent_text(path: &str) -> String {
    path.rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(parent, _)| format!("{parent}{}", std::path::MAIN_SEPARATOR))
        .unwrap_or_default()
}

fn completion_is_file(value: &str) -> bool {
    !value.ends_with(std::path::MAIN_SEPARATOR)
}

fn shell_escape_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for ch in path.chars() {
        if matches!(
            ch,
            ' ' | '\t'
                | '\n'
                | '\\'
                | '\''
                | '"'
                | '$'
                | '&'
                | ';'
                | '|'
                | '<'
                | '>'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '*'
                | '?'
                | '!'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn config_key_from_parts(parts: &[String]) -> Option<&str> {
    parts
        .windows(2)
        .filter_map(|window| match window {
            [option, value] if option == "--key" || option == "-k" => Some(value.as_str()),
            _ => None,
        })
        .next_back()
}

fn object_list_class_column_values(
    ctx: &CompletionContext,
    class_name: &str,
    prefix: &str,
) -> Vec<String> {
    let (base, segment_prefix) = comma_completion_prefix(prefix);
    let mut fields = vec![
        "id".to_string(),
        "Name".to_string(),
        "Description".to_string(),
        "Namespace".to_string(),
        "Class".to_string(),
        "Data".to_string(),
        "Created".to_string(),
        "Updated".to_string(),
        "name".to_string(),
        "description".to_string(),
        "namespace".to_string(),
        "class".to_string(),
        "data".to_string(),
        "created_at".to_string(),
        "updated_at".to_string(),
    ];

    if let Some(Some(schema)) = ctx.class_schema(class_name) {
        fields.extend(
            crate::json_schema::schema_paths(&schema, true)
                .into_iter()
                .map(|path| format!("data.{path}")),
        );
    }

    fields.sort();
    fields.dedup();
    fields
        .into_iter()
        .filter(|field| field.starts_with(segment_prefix))
        .map(|field| format!("{base}{field}"))
        .collect()
}

fn comma_completion_prefix(prefix: &str) -> (&str, &str) {
    prefix
        .rfind(',')
        .map(|index| (&prefix[..=index], &prefix[index + 1..]))
        .unwrap_or(("", prefix))
}

fn complete_values(values: &[&str], prefix: &str) -> Vec<String> {
    values
        .iter()
        .copied()
        .filter(|value| value.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

fn complete_csv_values(values: &[&str], prefix: &str) -> Vec<String> {
    let (head, tail) = prefix
        .rsplit_once(',')
        .map(|(head, tail)| (format!("{head},"), tail))
        .unwrap_or_else(|| (String::new(), prefix));

    values
        .iter()
        .copied()
        .filter(|value| value.starts_with(tail.trim_start()))
        .map(|value| format!("{head}{value}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        comma_completion_prefix, config_key_from_parts, config_value_candidates_for_parts,
        file_path_candidates,
    };
    use crate::json_schema::schema_paths;

    #[test]
    fn config_key_from_parts_uses_selected_key() {
        let parts = vec![
            "config".to_string(),
            "set".to_string(),
            "--key".to_string(),
            "output.table_style".to_string(),
            "--value".to_string(),
            "co".to_string(),
        ];

        assert_eq!(config_key_from_parts(&parts), Some("output.table_style"));
    }

    #[test]
    fn config_key_from_parts_accepts_short_key_option() {
        let parts = vec![
            "config".to_string(),
            "set".to_string(),
            "-k".to_string(),
            "output.table_style".to_string(),
            "--value".to_string(),
        ];

        assert_eq!(config_key_from_parts(&parts), Some("output.table_style"));
    }

    #[test]
    fn config_values_complete_object_list_data_column_modes() {
        let parts = vec![
            "config".to_string(),
            "set".to_string(),
            "--key".to_string(),
            "output.object_list_data_columns".to_string(),
            "--value".to_string(),
        ];

        assert_eq!(
            config_value_candidates_for_parts("", &parts),
            vec!["auto".to_string(), "preview".to_string(), "all".to_string()]
        );
    }

    #[test]
    fn comma_completion_prefix_replaces_only_active_segment() {
        assert_eq!(comma_completion_prefix("Name,co"), ("Name,", "co"));
        assert_eq!(comma_completion_prefix("co"), ("", "co"));
        assert_eq!(comma_completion_prefix("Name,"), ("Name,", ""));
    }

    #[test]
    fn file_path_candidates_include_matching_files_and_directories() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let nested_dir = dir.path().join("imports");
        std::fs::create_dir(&nested_dir).expect("nested dir should be created");
        std::fs::write(dir.path().join("import.json"), "{}").expect("file should be written");

        let prefix = dir.path().join("imp").to_string_lossy().to_string();
        let suggestions = file_path_candidates(&prefix);

        assert!(suggestions.iter().any(|value| value.ends_with("imports/")));
        assert!(suggestions
            .iter()
            .any(|value| value.ends_with("import.json")));
    }

    #[test]
    fn file_path_candidates_escape_spaces() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        std::fs::write(dir.path().join("import payload.json"), "{}")
            .expect("file should be written");

        let prefix = dir.path().join("import").to_string_lossy().to_string();
        let suggestions = file_path_candidates(&prefix);

        assert!(suggestions
            .iter()
            .any(|value| value.ends_with("import\\ payload.json")));
    }

    #[test]
    fn schema_paths_include_nested_properties() {
        let schema = serde_json::json!({
            "properties": {
                "contact": {"type": "string"},
                "hardware": {
                    "properties": {
                        "cpu": {"type": "string"}
                    }
                },
                "interfaces": {
                    "type": "array",
                    "items": {
                        "properties": {
                            "ipv4": {"type": "string"}
                        }
                    }
                }
            }
        });

        assert_eq!(
            schema_paths(&schema, true),
            vec![
                "contact".to_string(),
                "hardware".to_string(),
                "hardware.cpu".to_string(),
                "interfaces".to_string(),
                "interfaces[*].ipv4".to_string()
            ]
        );
    }
}
