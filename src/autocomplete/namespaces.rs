use strum::IntoEnumIterator;

use crate::{domain::NamespacePermission, services::CompletionContext};

pub fn namespaces(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.namespaces(prefix)
}

#[allow(dead_code)]
pub fn permissions(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    NamespacePermission::iter()
        .filter(|permission| permission.to_string().starts_with(prefix))
        .map(|permission| permission.to_string())
        .collect()
}
