use strum::IntoEnumIterator;

use crate::{domain::CollectionPermission, services::CompletionContext};

pub fn collections(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.collections(prefix)
}

#[allow(dead_code)]
pub fn permissions(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    CollectionPermission::iter()
        .filter(|permission| permission.to_string().starts_with(prefix))
        .map(|permission| permission.to_string())
        .collect()
}
