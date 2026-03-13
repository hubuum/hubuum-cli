use crate::config::config_key_names;
use crate::services::CompletionContext;

pub fn bool(_ctx: &CompletionContext, _prefix: &str, _parts: &[String]) -> Vec<String> {
    vec!["true".to_string(), "false".to_string()]
}

pub fn config_keys(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    config_key_names()
        .into_iter()
        .filter(|key| key.starts_with(prefix))
        .map(str::to_string)
        .collect()
}
