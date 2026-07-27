use crate::services::CompletionContext;
use crate::{json_schema::schema_paths, services::sort_specs_for_command_path};

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

pub fn object_aggregate_dimensions(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    aggregate_fields(ctx, parts)
        .into_iter()
        .filter(|field| field.starts_with(prefix))
        .collect()
}

pub fn object_aggregate_measures(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    let operations = ["sum", "average", "min", "max"];
    let Some((operation, field_prefix)) = prefix.split_once(':') else {
        return operations
            .into_iter()
            .filter(|operation| operation.starts_with(prefix))
            .map(|operation| format!("{operation}:"))
            .collect();
    };
    if !operations.contains(&operation) {
        return Vec::new();
    }

    aggregate_fields(ctx, parts)
        .into_iter()
        .filter(|field| {
            field.starts_with(field_prefix)
                && (field.starts_with("data.")
                    || field.starts_with("S:")
                    || field.starts_with("P:"))
        })
        .map(|field| format!("{operation}:{field}"))
        .collect()
}

fn aggregate_fields(ctx: &CompletionContext, parts: &[String]) -> Vec<String> {
    let mut fields = vec![
        "name".to_string(),
        "description".to_string(),
        "collection_id".to_string(),
        "created_at".to_string(),
        "updated_at".to_string(),
    ];
    fields.extend(aggregate_data_fields(ctx, parts));
    fields.extend(ctx.computed_sort_fields(parts));
    fields
}

fn aggregate_data_fields(ctx: &CompletionContext, parts: &[String]) -> Vec<String> {
    let Some(class_name) = class_name_from_parts(parts) else {
        return vec!["data.".to_string()];
    };
    let Some(schema) = ctx.class_schema(&class_name).flatten() else {
        return vec!["data.".to_string()];
    };
    let fields = schema_paths(&schema, false)
        .into_iter()
        .map(|path| format!("data.{path}"))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        vec!["data.".to_string()]
    } else {
        fields
    }
}

fn class_name_from_parts(parts: &[String]) -> Option<String> {
    parts
        .windows(2)
        .find(|pair| pair[0] == "--class" || pair[0] == "-c")
        .map(|pair| pair[1].clone())
}

pub fn object_aggregate_sort(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    let command_path = ["object".to_string(), "aggregate".to_string()];
    if sort_specs_for_command_path(&command_path).is_none() {
        return Vec::new();
    }
    super::complete_sort_clause(ctx, &command_path, parts, prefix, false)
        .into_iter()
        .map(|completion| completion.value)
        .collect()
}
