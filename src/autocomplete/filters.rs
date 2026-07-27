use crate::list_query::{
    completion_operators, resolve_filter_field_spec, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile,
};
use crate::services::{filter_specs_for_command_path, CompletionContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilterCompletion {
    pub value: String,
    pub description: Option<String>,
    pub append_whitespace: bool,
}

pub(crate) fn complete_where_clause(
    ctx: &CompletionContext,
    command_path: &[String],
    command_parts: &[String],
    clause: &str,
    ends_with_space: bool,
) -> Vec<FilterCompletion> {
    let Some(specs) = filter_specs_for_command_path(command_path) else {
        return Vec::new();
    };
    let additional_fields = if command_path == ["object", "aggregate"] {
        ctx.computed_sort_fields(command_parts)
    } else {
        Vec::new()
    };

    match clause_stage_with_fields(clause, ends_with_space, specs, &additional_fields) {
        ClauseStage::Field { prefix } => complete_field_or_json_path(
            ctx,
            command_path,
            command_parts,
            prefix,
            specs,
            &additional_fields,
        ),
        ClauseStage::Operator { field, prefix } => {
            let profile = match resolve_filter_field_spec(specs, field) {
                Some((spec, _)) => spec.operator_profile,
                None if additional_fields.iter().any(|candidate| candidate == field) => {
                    FilterOperatorProfile::Any
                }
                None => return complete_field(field, specs, &additional_fields),
            };
            completion_operators(profile)
                .iter()
                .filter(|operator| operator.starts_with(prefix))
                .map(|operator| FilterCompletion {
                    value: (*operator).to_string(),
                    description: Some(format!("operator for {field}")),
                    append_whitespace: true,
                })
                .collect()
        }
        ClauseStage::Value {
            field,
            operator,
            value_prefix,
        } => complete_value(ctx, specs, field, operator, value_prefix),
        ClauseStage::Finished => Vec::new(),
    }
}

pub fn class_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["class", "list"], prefix, _parts)
}

pub fn group_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["group", "list"], prefix, _parts)
}

pub fn collection_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["collection", "list"], prefix, _parts)
}

pub fn object_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["object", "list"], prefix, _parts)
}

pub fn object_aggregate_where(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["object", "aggregate"], prefix, parts)
}

pub fn relation_class_list_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "list"], prefix, _parts)
}

pub fn relation_class_direct_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "direct"], prefix, _parts)
}

pub fn relation_class_graph_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "graph"], prefix, _parts)
}

pub fn relation_object_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "list"], prefix, _parts)
}

pub fn relation_object_direct_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "direct"], prefix, _parts)
}

pub fn relation_object_graph_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "graph"], prefix, _parts)
}

pub fn export_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["export", "list"], prefix, _parts)
}

pub fn user_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["user", "list"], prefix, _parts)
}

fn complete_for_path(
    ctx: &CompletionContext,
    command_path: &[&str],
    clause: &str,
    command_parts: &[String],
) -> Vec<String> {
    let owned_path = command_path
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    complete_where_clause(ctx, &owned_path, command_parts, clause, false)
        .into_iter()
        .map(|completion| completion.value)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClauseStage<'a> {
    Field {
        prefix: &'a str,
    },
    Operator {
        field: &'a str,
        prefix: &'a str,
    },
    Value {
        field: &'a str,
        operator: &'a str,
        value_prefix: &'a str,
    },
    Finished,
}

#[cfg(test)]
fn clause_stage<'a>(
    clause: &'a str,
    ends_with_space: bool,
    specs: &[FilterFieldSpec],
) -> ClauseStage<'a> {
    clause_stage_with_fields(clause, ends_with_space, specs, &[])
}

fn clause_stage_with_fields<'a>(
    clause: &'a str,
    ends_with_space: bool,
    specs: &[FilterFieldSpec],
    additional_fields: &[String],
) -> ClauseStage<'a> {
    let tokens = clause.split_whitespace().collect::<Vec<_>>();

    match tokens.as_slice() {
        [] => ClauseStage::Field { prefix: "" },
        [field] if ends_with_space => ClauseStage::Operator { field, prefix: "" },
        [field] => {
            if is_dotted_json_path(specs, field) {
                ClauseStage::Field { prefix: field }
            } else if resolve_filter_field_spec(specs, field).is_some()
                || additional_fields.iter().any(|candidate| candidate == field)
            {
                ClauseStage::Operator { field, prefix: "" }
            } else {
                ClauseStage::Field { prefix: field }
            }
        }
        [field, _operator] if ends_with_space => ClauseStage::Value {
            field,
            operator: tokens[1],
            value_prefix: "",
        },
        [field, operator] => ClauseStage::Operator {
            field,
            prefix: operator,
        },
        [_field, _operator, ..] if ends_with_space => ClauseStage::Finished,
        [field, operator, remainder @ ..] => ClauseStage::Value {
            field,
            operator,
            value_prefix: remainder.last().copied().unwrap_or(operator),
        },
    }
}

fn is_dotted_json_path(specs: &[FilterFieldSpec], field: &str) -> bool {
    field.contains('.')
        && resolve_filter_field_spec(specs, field)
            .map(|(spec, path)| spec.json_root && !path.is_empty())
            .unwrap_or(false)
}

fn complete_field_or_json_path(
    ctx: &CompletionContext,
    command_path: &[String],
    command_parts: &[String],
    prefix: &str,
    specs: &[FilterFieldSpec],
    additional_fields: &[String],
) -> Vec<FilterCompletion> {
    let completions = complete_field(prefix, specs, additional_fields);
    if !completions.is_empty() {
        return completions;
    }

    let data_completions = complete_object_data_path(ctx, command_path, command_parts, prefix);
    if !data_completions.is_empty() {
        return data_completions;
    }

    complete_json_path_fallback(prefix, specs)
}

fn complete_json_path_fallback(prefix: &str, specs: &[FilterFieldSpec]) -> Vec<FilterCompletion> {
    if is_dotted_json_path(specs, prefix) {
        return vec![FilterCompletion {
            value: prefix.to_string(),
            description: Some("JSON path".to_string()),
            append_whitespace: true,
        }];
    }

    Vec::new()
}

fn complete_object_data_path(
    ctx: &CompletionContext,
    command_path: &[String],
    command_parts: &[String],
    prefix: &str,
) -> Vec<FilterCompletion> {
    if !matches!(
        command_path,
        [scope, command]
            if scope == "object" && (command == "list" || command == "aggregate")
    ) || !(prefix.starts_with("data.") || prefix.starts_with("json_data."))
    {
        return Vec::new();
    }

    let Some(class_name) = class_name_from_parts(command_parts) else {
        return Vec::new();
    };

    let fields = ctx.object_data_fields_for_class(&class_name);
    if fields.is_empty() {
        return status_completions(prefix, "no schema or observed fields");
    }

    let root = if prefix.starts_with("json_data.") {
        "json_data"
    } else {
        "data"
    };
    let paths = fields
        .into_iter()
        .filter_map(|field| field.strip_prefix("data.").map(str::to_string))
        .collect::<Vec<_>>();
    let completions = data_path_completions_from_paths(root, prefix, &paths);
    if completions.is_empty() {
        return status_completions(prefix, "no field match");
    }

    completions
}

fn status_completions(prefix: &str, status: &str) -> Vec<FilterCompletion> {
    vec![
        FilterCompletion {
            value: prefix.to_string(),
            description: Some(status.to_string()),
            append_whitespace: false,
        },
        FilterCompletion {
            value: prefix.to_string(),
            description: Some("type path manually".to_string()),
            append_whitespace: false,
        },
    ]
}

fn class_name_from_parts(parts: &[String]) -> Option<String> {
    parts
        .windows(2)
        .find(|pair| pair[0] == "--class" || pair[0] == "-c")
        .map(|pair| pair[1].clone())
}

fn data_path_completions_from_paths(
    root: &str,
    prefix: &str,
    paths: &[String],
) -> Vec<FilterCompletion> {
    paths
        .iter()
        .flat_map(|path| {
            let field = format!("{root}.{path}");
            let field_completion = FilterCompletion {
                value: field.clone(),
                description: Some("object data field".to_string()),
                append_whitespace: true,
            };
            let child_prefix = format!("{path}.");
            let nested_completion = paths
                .iter()
                .any(|candidate| candidate.starts_with(&child_prefix))
                .then(|| FilterCompletion {
                    value: format!("{field}."),
                    description: Some("object data container".to_string()),
                    append_whitespace: false,
                });
            [Some(field_completion), nested_completion]
        })
        .flatten()
        .filter(|candidate| candidate.value.starts_with(prefix))
        .collect()
}

fn complete_field(
    prefix: &str,
    specs: &[FilterFieldSpec],
    additional_fields: &[String],
) -> Vec<FilterCompletion> {
    specs
        .iter()
        .flat_map(|spec| {
            let root_description = if spec.json_root {
                Some("JSON root; append a dotted path".to_string())
            } else {
                None
            };
            let root = FilterCompletion {
                value: spec.public_name.to_string(),
                description: root_description.clone(),
                append_whitespace: true,
            };
            let nested = spec.json_root.then(|| FilterCompletion {
                value: format!("{}.", spec.public_name),
                description: root_description,
                append_whitespace: false,
            });
            [Some(root), nested]
        })
        .flatten()
        .chain(additional_fields.iter().map(|field| FilterCompletion {
            value: field.clone(),
            description: Some("computed field".to_string()),
            append_whitespace: true,
        }))
        .filter(|candidate| candidate.value.starts_with(prefix))
        .collect()
}

fn complete_value(
    ctx: &CompletionContext,
    specs: &[FilterFieldSpec],
    field: &str,
    operator: &str,
    prefix: &str,
) -> Vec<FilterCompletion> {
    let Some((spec, _)) = resolve_filter_field_spec(specs, field) else {
        return Vec::new();
    };

    let values = match spec.public_name {
        "collection" => values(ctx.collections(prefix)),
        "class" | "class_a" | "class_b" | "root_class" | "related_class" => {
            values(ctx.classes(prefix))
        }
        "object_a" | "object_b" => Vec::new(),
        _ if spec.value_profile == FilterValueProfile::Boolean => values(
            ["true".to_string(), "false".to_string()]
                .into_iter()
                .filter(|value| value.starts_with(prefix))
                .collect(),
        ),
        _ => Vec::new(),
    };

    if values.is_empty() {
        return vec![placeholder_value(spec, operator)];
    }

    values
}

fn values(values: Vec<String>) -> Vec<FilterCompletion> {
    values
        .into_iter()
        .map(|value| FilterCompletion {
            value,
            description: None,
            append_whitespace: true,
        })
        .collect()
}

fn placeholder_value(spec: FilterFieldSpec, operator: &str) -> FilterCompletion {
    let value = if operator == "between" || operator == "not_between" {
        "<low,high>"
    } else {
        match spec.value_profile {
            FilterValueProfile::String => "<string>",
            FilterValueProfile::Integer => "<integer>",
            FilterValueProfile::DateTime => "<date-time>",
            FilterValueProfile::Boolean => "<bool>",
            FilterValueProfile::Any => "<value>",
        }
    };

    FilterCompletion {
        value: value.to_string(),
        description: Some(format!("value for {}", spec.public_name)),
        append_whitespace: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        class_name_from_parts, clause_stage, clause_stage_with_fields, complete_field,
        complete_json_path_fallback, data_path_completions_from_paths, placeholder_value,
        status_completions, ClauseStage,
    };
    use crate::list_query::{FilterFieldSpec, FilterOperatorProfile, FilterValueProfile};

    #[test]
    fn clause_stage_understands_field_and_operator_boundaries() {
        let specs = [FilterFieldSpec::new(
            "name",
            "name",
            FilterOperatorProfile::String,
            FilterValueProfile::String,
        )];

        assert_eq!(
            clause_stage("", false, &specs),
            ClauseStage::Field { prefix: "" }
        );
        assert_eq!(
            clause_stage("name ", true, &specs),
            ClauseStage::Operator {
                field: "name",
                prefix: "",
            }
        );
        assert_eq!(
            clause_stage("name", false, &specs),
            ClauseStage::Operator {
                field: "name",
                prefix: "",
            }
        );
        assert_eq!(
            clause_stage("name ic", false, &specs),
            ClauseStage::Operator {
                field: "name",
                prefix: "ic",
            }
        );
        assert_eq!(
            clause_stage("name icontains ", true, &specs),
            ClauseStage::Value {
                field: "name",
                operator: "icontains",
                value_prefix: "",
            }
        );
    }

    #[test]
    fn field_completion_exposes_json_roots_with_dotted_variant() {
        let specs = [
            FilterFieldSpec::new(
                "name",
                "name",
                FilterOperatorProfile::String,
                FilterValueProfile::String,
            ),
            FilterFieldSpec::new(
                "data",
                "data",
                FilterOperatorProfile::Any,
                FilterValueProfile::Any,
            )
            .json_root(),
        ];

        let completions = complete_field("da", &specs, &[]);
        assert!(completions
            .iter()
            .any(|candidate| candidate.value == "data"));
        assert!(completions
            .iter()
            .any(|candidate| candidate.value == "data."));
    }

    #[test]
    fn computed_aggregate_filter_fields_accept_operator_completion() {
        let specs = [FilterFieldSpec::new(
            "name",
            "name",
            FilterOperatorProfile::String,
            FilterValueProfile::String,
        )];
        let computed = vec!["S:risk".to_string()];

        assert_eq!(
            clause_stage_with_fields("S:risk", false, &specs, &computed),
            ClauseStage::Operator {
                field: "S:risk",
                prefix: "",
            }
        );
        assert!(complete_field("S:", &specs, &computed)
            .iter()
            .any(|completion| completion.value == "S:risk"));
    }

    #[test]
    fn dotted_json_path_completion_preserves_field_and_appends_space() {
        let specs = [FilterFieldSpec::new(
            "json_data",
            "json_data",
            FilterOperatorProfile::Any,
            FilterValueProfile::Any,
        )
        .json_root()];

        assert_eq!(
            clause_stage("json_data.contact", false, &specs),
            ClauseStage::Field {
                prefix: "json_data.contact"
            }
        );

        let completions = complete_json_path_fallback("json_data.contact", &specs);
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].value, "json_data.contact");
        assert!(completions[0].append_whitespace);
    }

    #[test]
    fn dotted_json_schema_path_completion_preserves_field_and_appends_space() {
        let specs = [FilterFieldSpec::new(
            "json_schema",
            "json_schema",
            FilterOperatorProfile::Any,
            FilterValueProfile::Any,
        )
        .json_root()];

        let completions = complete_json_path_fallback("json_schema.contact", &specs);
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].value, "json_schema.contact");
        assert!(completions[0].append_whitespace);
    }

    #[test]
    fn data_path_completion_expands_nested_schema_or_observed_fields() {
        let completions = data_path_completions_from_paths(
            "json_data",
            "json_data.o",
            &[
                "contact".to_string(),
                "owner".to_string(),
                "owner.email".to_string(),
            ],
        );
        let values = completions
            .iter()
            .map(|completion| completion.value.as_str())
            .collect::<Vec<_>>();

        assert!(values.contains(&"json_data.owner"));
        assert!(values.contains(&"json_data.owner."));
        assert!(values.contains(&"json_data.owner.email"));
    }

    #[test]
    fn class_name_completion_context_accepts_long_and_short_options() {
        assert_eq!(
            class_name_from_parts(&[
                "object".to_string(),
                "list".to_string(),
                "--class".to_string(),
                "Hosts".to_string(),
            ]),
            Some("Hosts".to_string())
        );
        assert_eq!(
            class_name_from_parts(&[
                "object".to_string(),
                "list".to_string(),
                "-c".to_string(),
                "Hosts".to_string(),
            ]),
            Some("Hosts".to_string())
        );
    }

    #[test]
    fn status_completions_do_not_collapse_to_single_quick_completion() {
        let completions = status_completions("json_data.", "no data fields");

        assert_eq!(completions.len(), 2);
        assert!(completions
            .iter()
            .all(|completion| completion.value == "json_data."));
        assert_eq!(
            completions[0].description.as_deref(),
            Some("no data fields")
        );
        assert_eq!(
            completions[1].description.as_deref(),
            Some("type path manually")
        );
        assert!(completions
            .iter()
            .all(|completion| !completion.append_whitespace));
    }

    #[test]
    fn value_completion_falls_back_to_placeholder() {
        let spec = FilterFieldSpec::new(
            "name",
            "name",
            FilterOperatorProfile::String,
            FilterValueProfile::String,
        );

        let completion = placeholder_value(spec, "icontains");
        assert_eq!(completion.value, "<string>");
    }
}
