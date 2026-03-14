use crate::list_query::{
    completion_operators, resolve_filter_field_spec, FilterFieldSpec, FilterValueProfile,
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
    clause: &str,
    ends_with_space: bool,
) -> Vec<FilterCompletion> {
    let Some(specs) = filter_specs_for_command_path(command_path) else {
        return Vec::new();
    };

    match clause_stage(clause, ends_with_space, specs) {
        ClauseStage::Field { prefix } => complete_field(prefix, specs),
        ClauseStage::Operator { field, prefix } => {
            let Some((spec, _)) = resolve_filter_field_spec(specs, field) else {
                return complete_field(field, specs);
            };
            completion_operators(spec.operator_profile)
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
    complete_for_path(ctx, &["class", "list"], prefix)
}

pub fn group_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["group", "list"], prefix)
}

pub fn namespace_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["namespace", "list"], prefix)
}

pub fn object_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["object", "list"], prefix)
}

pub fn relation_class_list_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "list"], prefix)
}

pub fn relation_class_direct_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "direct"], prefix)
}

pub fn relation_class_graph_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "graph"], prefix)
}

pub fn relation_object_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "list"], prefix)
}

pub fn relation_object_direct_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "direct"], prefix)
}

pub fn relation_object_graph_where(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "graph"], prefix)
}

pub fn report_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["report", "list"], prefix)
}

pub fn user_where(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["user", "list"], prefix)
}

fn complete_for_path(ctx: &CompletionContext, command_path: &[&str], clause: &str) -> Vec<String> {
    let owned_path = command_path
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    complete_where_clause(ctx, &owned_path, clause, false)
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

fn clause_stage<'a>(
    clause: &'a str,
    ends_with_space: bool,
    specs: &[FilterFieldSpec],
) -> ClauseStage<'a> {
    let tokens = clause.split_whitespace().collect::<Vec<_>>();

    match tokens.as_slice() {
        [] => ClauseStage::Field { prefix: "" },
        [field] if ends_with_space => ClauseStage::Operator { field, prefix: "" },
        [field] => {
            if resolve_filter_field_spec(specs, field).is_some() {
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

fn complete_field(prefix: &str, specs: &[FilterFieldSpec]) -> Vec<FilterCompletion> {
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
        "namespace" => values(ctx.namespaces(prefix)),
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
    use crate::list_query::{FilterOperatorProfile, FilterValueProfile};

    use super::{clause_stage, complete_field, placeholder_value, ClauseStage};
    use crate::list_query::FilterFieldSpec;

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

        let completions = complete_field("da", &specs);
        assert!(completions
            .iter()
            .any(|candidate| candidate.value == "data"));
        assert!(completions
            .iter()
            .any(|candidate| candidate.value == "data."));
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
