use crate::config::{config_key_names, config_value_candidates};
use crate::services::CompletionContext;

pub fn bool(_ctx: &CompletionContext, _prefix: &str, _parts: &[String]) -> Vec<String> {
    vec!["true".to_string(), "false".to_string()]
}

pub fn search_kinds(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ["namespace", "class", "object"]
        .into_iter()
        .filter(|kind| kind.starts_with(prefix))
        .map(str::to_string)
        .collect()
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
        .map(str::to_string)
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
            schema_paths(&schema)
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

fn schema_paths(schema: &serde_json::Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_schema_paths(schema, "", &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

fn collect_schema_paths(schema: &serde_json::Value, prefix: &str, paths: &mut Vec<String>) {
    let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) else {
        return;
    };

    for (name, property_schema) in properties {
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        paths.push(path.clone());
        collect_schema_paths(property_schema, &path, paths);
        if let Some(items) = property_schema.get("items") {
            collect_schema_paths(items, &format!("{path}[*]"), paths);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        comma_completion_prefix, config_key_from_parts, config_value_candidates_for_parts,
        schema_paths,
    };

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
            schema_paths(&schema),
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
