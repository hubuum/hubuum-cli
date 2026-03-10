use crate::services::CompletionContext;

pub fn groups(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.groups(prefix)
}
