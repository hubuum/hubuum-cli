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

pub fn config_keys(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    config_key_names()
        .into_iter()
        .filter(|key| key.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

pub fn config_values(_ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::{config_key_from_parts, config_value_candidates_for_parts};

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
}
