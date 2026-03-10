use strum::IntoEnumIterator;

use crate::services::CompletionContext;

pub fn bool(_ctx: &CompletionContext, _prefix: &str, _parts: &[String]) -> Vec<String> {
    vec!["true".to_string(), "false".to_string()]
}

pub fn groups(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.groups(prefix)
}

pub fn classes(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.classes(prefix)
}

pub fn namespaces(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.namespaces(prefix)
}

#[allow(dead_code)]
pub fn permissions(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    use crate::domain::NamespacePermission;

    NamespacePermission::iter()
        .filter(|permission| permission.to_string().starts_with(prefix))
        .map(|permission| permission.to_string())
        .collect()
}

pub fn objects_from_class(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--class")
}

pub fn objects_from_class_from(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--class_from")
}

pub fn objects_from_class_to(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--class_to")
}
