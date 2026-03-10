use crate::services::CompletionContext;

pub fn classes(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.classes(prefix)
}
