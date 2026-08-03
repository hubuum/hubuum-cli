use crate::services::CompletionContext;

pub fn user_token_ids(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    ctx.user_token_ids(prefix, parts)
}

pub fn service_account_token_ids(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.service_account_token_ids(prefix, parts)
}
