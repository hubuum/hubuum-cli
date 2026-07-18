use crate::list_query::{completion_sort_directions, resolve_sort_field_spec, SortFieldSpec};
use crate::services::{sort_specs_for_command_path, CompletionContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SortCompletion {
    pub value: String,
    pub description: Option<String>,
    pub append_whitespace: bool,
}

pub(crate) fn complete_sort_clause(
    ctx: &CompletionContext,
    command_path: &[String],
    parts: &[String],
    clause: &str,
    ends_with_space: bool,
) -> Vec<SortCompletion> {
    let additional_fields = if command_path == ["object", "list"] {
        ctx.computed_sort_fields(parts)
    } else {
        Vec::new()
    };
    complete_sort_clause_with_fields(
        ctx,
        command_path,
        clause,
        ends_with_space,
        &additional_fields,
    )
}

fn complete_sort_clause_with_fields(
    _ctx: &CompletionContext,
    command_path: &[String],
    clause: &str,
    ends_with_space: bool,
    additional_fields: &[String],
) -> Vec<SortCompletion> {
    let Some(specs) = sort_specs_for_command_path(command_path) else {
        return Vec::new();
    };

    match clause_stage_with_fields(clause, ends_with_space, specs, additional_fields) {
        SortClauseStage::Field { prefix } => complete_field(prefix, specs, additional_fields),
        SortClauseStage::Direction { field, prefix } => {
            if resolve_sort_field_spec(specs, field).is_none()
                && !additional_fields.iter().any(|candidate| candidate == field)
            {
                return complete_field(field, specs, additional_fields);
            }

            completion_sort_directions()
                .iter()
                .filter(|direction| direction.starts_with(prefix))
                .map(|direction| SortCompletion {
                    value: (*direction).to_string(),
                    description: Some(format!("sort direction for {field}")),
                    append_whitespace: true,
                })
                .collect()
        }
        SortClauseStage::Finished => Vec::new(),
    }
}

pub fn class_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["class", "list"], prefix)
}

pub fn group_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["group", "list"], prefix)
}

pub fn collection_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["collection", "list"], prefix)
}

pub fn object_sort(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    let command_path = ["object".to_string(), "list".to_string()];
    complete_sort_clause(ctx, &command_path, parts, prefix, false)
        .into_iter()
        .map(|completion| completion.value)
        .collect()
}

pub fn relation_class_list_sort(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "list"], prefix)
}

pub fn relation_class_direct_sort(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "class", "direct"], prefix)
}

pub fn relation_object_sort(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "list"], prefix)
}

pub fn relation_object_direct_sort(
    ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_for_path(ctx, &["relation", "object", "direct"], prefix)
}

pub fn export_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["export", "list"], prefix)
}

pub fn user_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["user", "list"], prefix)
}

pub fn task_event_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["task", "events"], prefix)
}

pub fn import_result_sort(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_for_path(ctx, &["import", "results"], prefix)
}

fn complete_for_path(ctx: &CompletionContext, command_path: &[&str], clause: &str) -> Vec<String> {
    let owned_path = command_path
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    complete_sort_clause(ctx, &owned_path, &[], clause, false)
        .into_iter()
        .map(|completion| completion.value)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortClauseStage<'a> {
    Field { prefix: &'a str },
    Direction { field: &'a str, prefix: &'a str },
    Finished,
}

#[cfg(test)]
fn clause_stage<'a>(
    clause: &'a str,
    ends_with_space: bool,
    specs: &[SortFieldSpec],
) -> SortClauseStage<'a> {
    clause_stage_with_fields(clause, ends_with_space, specs, &[])
}

fn clause_stage_with_fields<'a>(
    clause: &'a str,
    ends_with_space: bool,
    specs: &[SortFieldSpec],
    additional_fields: &[String],
) -> SortClauseStage<'a> {
    let tokens = clause.split_whitespace().collect::<Vec<_>>();

    match tokens.as_slice() {
        [] => SortClauseStage::Field { prefix: "" },
        [field] if ends_with_space => SortClauseStage::Direction { field, prefix: "" },
        [field] => {
            if resolve_sort_field_spec(specs, field).is_some()
                || additional_fields.iter().any(|candidate| candidate == field)
            {
                SortClauseStage::Direction { field, prefix: "" }
            } else {
                SortClauseStage::Field { prefix: field }
            }
        }
        [_field, _direction] if ends_with_space => SortClauseStage::Finished,
        [field, direction] => SortClauseStage::Direction {
            field,
            prefix: direction,
        },
        [_field, _direction, ..] => SortClauseStage::Finished,
    }
}

fn complete_field(
    prefix: &str,
    specs: &[SortFieldSpec],
    additional_fields: &[String],
) -> Vec<SortCompletion> {
    let mut completions = specs
        .iter()
        .filter(|spec| spec.public_name.starts_with(prefix))
        .map(|spec| SortCompletion {
            value: spec.public_name.to_string(),
            description: Some(format!("sort field {}", spec.public_name)),
            append_whitespace: true,
        })
        .collect::<Vec<_>>();
    completions.extend(
        additional_fields
            .iter()
            .filter(|field| field.starts_with(prefix))
            .map(|field| SortCompletion {
                value: field.clone(),
                description: Some(format!("computed sort field {field}")),
                append_whitespace: true,
            }),
    );
    completions
}

#[cfg(test)]
mod tests {
    use super::{clause_stage, clause_stage_with_fields, complete_field, SortClauseStage};
    use crate::list_query::SortFieldSpec;

    #[test]
    fn clause_stage_understands_field_and_direction_boundaries() {
        let specs = [SortFieldSpec::new("name", "name")];

        assert_eq!(
            clause_stage("", false, &specs),
            SortClauseStage::Field { prefix: "" }
        );
        assert_eq!(
            clause_stage("name", false, &specs),
            SortClauseStage::Direction {
                field: "name",
                prefix: "",
            }
        );
        assert_eq!(
            clause_stage("name ", true, &specs),
            SortClauseStage::Direction {
                field: "name",
                prefix: "",
            }
        );
        assert_eq!(
            clause_stage("name asc", false, &specs),
            SortClauseStage::Direction {
                field: "name",
                prefix: "asc",
            }
        );
        assert_eq!(
            clause_stage("name asc ", true, &specs),
            SortClauseStage::Finished
        );
    }

    #[test]
    fn computed_sort_fields_complete_and_accept_directions() {
        let specs = [SortFieldSpec::new("name", "name")];
        let computed = vec!["S:load".to_string(), "P:label".to_string()];

        assert_eq!(
            clause_stage_with_fields("S:load", false, &specs, &computed),
            SortClauseStage::Direction {
                field: "S:load",
                prefix: "",
            }
        );
        assert_eq!(
            complete_field("P:", &specs, &computed)
                .into_iter()
                .map(|completion| completion.value)
                .collect::<Vec<_>>(),
            vec!["P:label"]
        );
    }
}
