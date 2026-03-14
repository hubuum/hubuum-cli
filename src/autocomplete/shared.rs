use crate::config::config_key_names;
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
