use crate::services::CompletionContext;

pub fn report_templates(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.report_templates(prefix)
}

pub fn report_scope_kinds(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    [
        "namespaces",
        "classes",
        "objects_in_class",
        "class_relations",
        "object_relations",
        "related_objects",
    ]
    .into_iter()
    .filter(|value| value.starts_with(prefix))
    .map(str::to_string)
    .collect()
}

pub fn report_missing_data_policies(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    ["strict", "null", "omit"]
        .into_iter()
        .filter(|value| value.starts_with(prefix))
        .map(str::to_string)
        .collect()
}
