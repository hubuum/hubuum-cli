use crate::services::CompletionContext;

pub fn objects_from_class(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--class")
}

pub fn objects_from_class_a(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--class-a")
}

pub fn objects_from_class_b(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--class-b")
}

pub fn objects_from_root_class(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.objects_from_class(prefix, parts, "--root-class")
}

pub fn computed_field_paths(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    ctx.computed_field_paths(prefix, parts)
}

pub fn computed_fields(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    ["all".to_string(), "none".to_string()]
        .into_iter()
        .chain(ctx.computed_sort_fields(parts))
        .filter(|field| field.starts_with(prefix))
        .collect()
}
