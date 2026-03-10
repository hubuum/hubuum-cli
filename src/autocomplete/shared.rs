use crate::services::CompletionContext;

pub fn bool(_ctx: &CompletionContext, _prefix: &str, _parts: &[String]) -> Vec<String> {
    vec!["true".to_string(), "false".to_string()]
}
