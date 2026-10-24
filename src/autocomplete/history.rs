use crate::services::CompletionContext;

pub fn history_times(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    ctx.history_times(prefix, parts)
}
