use std::str::FromStr;

use cli_command_derive::CommandArgs;
use serde_json::Value;

use super::builder::{catalog_command, CommandDocs};
use super::{build_list_query, desired_format, render_list_page, CliCommand};
use crate::autocomplete::{
    bool, classes, computed_field_paths, computed_operations, computed_result_types,
    objects_from_class,
};
use crate::catalog::CommandCatalogBuilder;
use crate::domain::{
    ClassComputationStateRecord, ComputedFieldMutationRecord, ComputedFieldPreviewRecord,
    ComputedFieldRecord, SharedComputedFieldListRecord,
};
use crate::errors::AppError;
use crate::formatting::{append_json, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::{
    AppServices, ComputedDefinitionInput, ComputedOperationInput, ComputedOperationKind,
    ComputedPatchInput, ComputedPreviewTarget, ComputedResultKind,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["computed", "shared"],
            catalog_command(
                "list",
                SharedComputedList::default(),
                CommandDocs {
                    about: Some("List shared computed fields for a class"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "shared"],
            catalog_command(
                "create",
                SharedComputedCreate::default(),
                CommandDocs {
                    about: Some("Create a shared computed field"),
                    examples: Some(
                        "--class Hosts --key average_load --label \"Average load\" --operation average --path /load/one --path /load/five --result-type number",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "shared"],
            catalog_command(
                "update",
                SharedComputedUpdate::default(),
                CommandDocs {
                    about: Some("Update a shared computed field"),
                    long_about: Some(
                        "Update a shared computed field by key. The current revision is required for optimistic concurrency.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "shared"],
            catalog_command(
                "delete",
                SharedComputedDelete::default(),
                CommandDocs {
                    about: Some("Delete a shared computed field"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "shared"],
            catalog_command(
                "preview",
                SharedComputedPreview::default(),
                CommandDocs {
                    about: Some("Preview a shared computed field"),
                    long_about: Some(
                        "Evaluate an unsaved shared definition against either an existing object or inline JSON data.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "shared"],
            catalog_command(
                "rebuild",
                SharedComputedRebuild::default(),
                CommandDocs {
                    about: Some("Rebuild shared computed values for a class"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "personal"],
            catalog_command(
                "list",
                PersonalComputedList::default(),
                CommandDocs {
                    about: Some("List personal computed fields"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "personal"],
            catalog_command(
                "create",
                PersonalComputedCreate::default(),
                CommandDocs {
                    about: Some("Create a personal computed field"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "personal"],
            catalog_command(
                "update",
                PersonalComputedUpdate::default(),
                CommandDocs {
                    about: Some("Update a personal computed field"),
                    long_about: Some(
                        "Update a personal computed field by class and key. The current revision is required for optimistic concurrency.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "personal"],
            catalog_command(
                "delete",
                PersonalComputedDelete::default(),
                CommandDocs {
                    about: Some("Delete a personal computed field"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["computed", "personal"],
            catalog_command(
                "preview",
                PersonalComputedPreview::default(),
                CommandDocs {
                    about: Some("Preview a personal computed field"),
                    long_about: Some(
                        "Evaluate an unsaved personal definition against either an existing object or inline JSON data.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct SharedComputedList {
    #[option(
        short = "c",
        long = "class",
        help = "Class containing the shared definitions",
        autocomplete = "classes"
    )]
    class: String,
}

impl CliCommand for SharedComputedList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let fields = services
            .gateway()
            .list_shared_computed_fields(&query.class)?;
        render_shared_list(tokens, &fields)
    }
}

macro_rules! definition_args {
    ($name:ident) => {
        #[derive(Debug, Clone, CommandArgs, Default)]
        pub struct $name {
            #[option(
                short = "c",
                long = "class",
                help = "Target class",
                autocomplete = "classes"
            )]
            class: String,
            #[option(long = "key", help = "Stable computed-field key")]
            key: String,
            #[option(long = "label", help = "Human-readable label")]
            label: String,
            #[option(long = "description", help = "Description")]
            description: Option<String>,
            #[option(
                long = "operation",
                help = "Computed operation",
                autocomplete = "computed_operations"
            )]
            operation: String,
            #[option(
                long = "path",
                help = "JSON Pointer input path; repeat for multiple paths",
                autocomplete = "computed_field_paths"
            )]
            paths: Vec<String>,
            #[option(
                long = "result-type",
                help = "Computed result type",
                autocomplete = "computed_result_types"
            )]
            result_type: String,
            #[option(
                long = "enabled",
                help = "Whether the definition is enabled (default: true)",
                autocomplete = "bool"
            )]
            enabled: Option<bool>,
        }

        impl DefinitionArgs for $name {
            fn class(&self) -> &str {
                &self.class
            }

            fn key(&self) -> &str {
                &self.key
            }

            fn label(&self) -> &str {
                &self.label
            }

            fn description(&self) -> Option<&str> {
                self.description.as_deref()
            }

            fn operation(&self) -> &str {
                &self.operation
            }

            fn paths(&self) -> &[String] {
                &self.paths
            }

            fn result_type(&self) -> &str {
                &self.result_type
            }

            fn enabled(&self) -> Option<bool> {
                self.enabled
            }
        }
    };
}

trait DefinitionArgs {
    fn class(&self) -> &str;
    fn key(&self) -> &str;
    fn label(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn operation(&self) -> &str;
    fn paths(&self) -> &[String];
    fn result_type(&self) -> &str;
    fn enabled(&self) -> Option<bool>;
}

definition_args!(SharedComputedCreate);
definition_args!(PersonalComputedCreate);

impl CliCommand for SharedComputedCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = query.class().to_string();
        let result = services
            .gateway()
            .create_shared_computed_field(&class, definition_input(&query)?)?;
        render_mutation(tokens, &result)
    }
}

impl CliCommand for PersonalComputedCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = query.class().to_string();
        let result = services
            .gateway()
            .create_personal_computed_field(&class, definition_input(&query)?)?;
        render_definition(tokens, &result)
    }
}

macro_rules! update_args {
    ($name:ident) => {
        #[derive(Debug, Clone, CommandArgs, Default)]
        pub struct $name {
            #[option(
                short = "c",
                long = "class",
                help = "Target class",
                autocomplete = "classes"
            )]
            class: String,
            #[option(long = "key", help = "Current computed-field key")]
            key: String,
            #[option(long = "revision", help = "Expected current revision")]
            revision: i64,
            #[option(long = "new-key", help = "Replacement key")]
            new_key: Option<String>,
            #[option(long = "label", help = "Replacement label")]
            label: Option<String>,
            #[option(long = "description", help = "Replacement description")]
            description: Option<String>,
            #[option(
                long = "operation",
                help = "Replacement computed operation",
                autocomplete = "computed_operations"
            )]
            operation: Option<String>,
            #[option(
                long = "path",
                help = "Replacement JSON Pointer input path; repeat for multiple paths",
                autocomplete = "computed_field_paths"
            )]
            paths: Vec<String>,
            #[option(
                long = "result-type",
                help = "Replacement result type",
                autocomplete = "computed_result_types"
            )]
            result_type: Option<String>,
            #[option(
                long = "enabled",
                help = "Whether the definition is enabled",
                autocomplete = "bool"
            )]
            enabled: Option<bool>,
        }
    };
}

update_args!(SharedComputedUpdate);
update_args!(PersonalComputedUpdate);

impl CliCommand for SharedComputedUpdate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let patch = patch_input(
            query.revision,
            query.new_key,
            query.label,
            query.description,
            query.operation,
            query.paths,
            query.result_type,
            query.enabled,
        )?;
        let result =
            services
                .gateway()
                .update_shared_computed_field(&query.class, &query.key, patch)?;
        render_mutation(tokens, &result)
    }
}

impl CliCommand for PersonalComputedUpdate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let patch = patch_input(
            query.revision,
            query.new_key,
            query.label,
            query.description,
            query.operation,
            query.paths,
            query.result_type,
            query.enabled,
        )?;
        let result =
            services
                .gateway()
                .update_personal_computed_field(&query.class, &query.key, patch)?;
        render_definition(tokens, &result)
    }
}

macro_rules! delete_args {
    ($name:ident) => {
        #[derive(Debug, Clone, CommandArgs, Default)]
        pub struct $name {
            #[option(
                short = "c",
                long = "class",
                help = "Target class",
                autocomplete = "classes"
            )]
            class: String,
            #[option(long = "key", help = "Computed-field key")]
            key: String,
            #[option(long = "revision", help = "Expected current revision")]
            revision: i64,
        }
    };
}

delete_args!(SharedComputedDelete);
delete_args!(PersonalComputedDelete);

impl CliCommand for SharedComputedDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let deleted = services.gateway().delete_shared_computed_field(
            &query.class,
            &query.key,
            query.revision,
        )?;
        match desired_format(tokens) {
            OutputFormat::Json => append_json(&deleted)?,
            OutputFormat::Text => {
                append_line(format!("Deleted shared computed field '{}'.", query.key))?;
                render_state(&deleted.state)?;
            }
        }
        Ok(())
    }
}

impl CliCommand for PersonalComputedDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let deleted = services.gateway().delete_personal_computed_field(
            &query.class,
            &query.key,
            query.revision,
        )?;
        match desired_format(tokens) {
            OutputFormat::Json => append_json(&deleted)?,
            OutputFormat::Text => append_line(format!(
                "Deleted personal computed field '{}' from class '{}'.",
                deleted.key, query.class
            ))?,
        }
        Ok(())
    }
}

macro_rules! preview_args {
    ($name:ident) => {
        #[derive(Debug, Clone, CommandArgs, Default)]
        pub struct $name {
            #[option(
                short = "c",
                long = "class",
                help = "Target class",
                autocomplete = "classes"
            )]
            class: String,
            #[option(long = "key", help = "Preview definition key")]
            key: String,
            #[option(long = "label", help = "Preview definition label")]
            label: String,
            #[option(long = "description", help = "Preview definition description")]
            description: Option<String>,
            #[option(
                long = "operation",
                help = "Computed operation",
                autocomplete = "computed_operations"
            )]
            operation: String,
            #[option(
                long = "path",
                help = "JSON Pointer input path; repeat for multiple paths",
                autocomplete = "computed_field_paths"
            )]
            paths: Vec<String>,
            #[option(
                long = "result-type",
                help = "Computed result type",
                autocomplete = "computed_result_types"
            )]
            result_type: String,
            #[option(
                long = "enabled",
                help = "Whether the preview definition is enabled (default: true)",
                autocomplete = "bool"
            )]
            enabled: Option<bool>,
            #[option(
                long = "object",
                help = "Existing object to evaluate",
                autocomplete = "objects_from_class"
            )]
            object: Option<String>,
            #[option(
                long = "data",
                help = "Inline JSON data to evaluate",
                value_source = true
            )]
            data: Option<Value>,
        }

        impl DefinitionArgs for $name {
            fn class(&self) -> &str {
                &self.class
            }

            fn key(&self) -> &str {
                &self.key
            }

            fn label(&self) -> &str {
                &self.label
            }

            fn description(&self) -> Option<&str> {
                self.description.as_deref()
            }

            fn operation(&self) -> &str {
                &self.operation
            }

            fn paths(&self) -> &[String] {
                &self.paths
            }

            fn result_type(&self) -> &str {
                &self.result_type
            }

            fn enabled(&self) -> Option<bool> {
                self.enabled
            }
        }
    };
}

preview_args!(SharedComputedPreview);
preview_args!(PersonalComputedPreview);

impl CliCommand for SharedComputedPreview {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = query.class().to_string();
        let definition = definition_input(&query)?;
        let target = preview_target(query.object, query.data)?;
        let preview = services
            .gateway()
            .preview_shared_computed_field(&class, definition, target)?;
        render_preview(tokens, &preview)
    }
}

impl CliCommand for PersonalComputedPreview {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = query.class().to_string();
        let definition = definition_input(&query)?;
        let target = preview_target(query.object, query.data)?;
        let preview = services
            .gateway()
            .preview_personal_computed_field(&class, definition, target)?;
        render_preview(tokens, &preview)
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct SharedComputedRebuild {
    #[option(
        short = "c",
        long = "class",
        help = "Class whose shared values should be rebuilt",
        autocomplete = "classes"
    )]
    class: String,
}

impl CliCommand for SharedComputedRebuild {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let state = services
            .gateway()
            .rebuild_shared_computed_fields(&query.class)?;
        match desired_format(tokens) {
            OutputFormat::Json => append_json(&state)?,
            OutputFormat::Text => render_state(&state)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct PersonalComputedList {
    #[option(
        short = "c",
        long = "class",
        help = "Only fields for this class",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = true
    )]
    include_total: Option<bool>,
}

impl CliCommand for PersonalComputedList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &[],
            &[],
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [],
        )?;
        let fields = services
            .gateway()
            .list_personal_computed_fields(query.class.as_deref(), &list_query)?;
        render_list_page(tokens, &fields)
    }
}

fn definition_input(args: &impl DefinitionArgs) -> Result<ComputedDefinitionInput, AppError> {
    let operation = ComputedOperationInput::new(
        ComputedOperationKind::from_str(args.operation())?,
        args.paths().to_vec(),
    )?;
    let mut input = ComputedDefinitionInput::new(
        args.key(),
        args.label(),
        operation,
        ComputedResultKind::from_str(args.result_type())?,
    );
    if let Some(description) = args.description() {
        input = input.description(description);
    }
    if let Some(enabled) = args.enabled() {
        input = input.enabled(enabled);
    }
    Ok(input)
}

#[allow(clippy::too_many_arguments)]
fn patch_input(
    revision: i64,
    key: Option<String>,
    label: Option<String>,
    description: Option<String>,
    operation: Option<String>,
    paths: Vec<String>,
    result_type: Option<String>,
    enabled: Option<bool>,
) -> Result<ComputedPatchInput, AppError> {
    if operation.is_none() && !paths.is_empty() {
        return Err(AppError::MissingOptions(vec!["operation".to_string()]));
    }
    let operation = operation
        .map(|operation| {
            ComputedOperationInput::new(ComputedOperationKind::from_str(&operation)?, paths)
        })
        .transpose()?;
    let result_type = result_type
        .map(|result_type| ComputedResultKind::from_str(&result_type))
        .transpose()?;
    let patch = ComputedPatchInput::new(revision)
        .key(key)
        .label(label)
        .description(description)
        .operation(operation)
        .result_type(result_type)
        .enabled(enabled);
    if patch.is_empty() {
        return Err(AppError::InvalidOption(
            "At least one field to update must be supplied".to_string(),
        ));
    }
    Ok(patch)
}

fn preview_target(
    object: Option<String>,
    data: Option<Value>,
) -> Result<ComputedPreviewTarget, AppError> {
    match (object, data) {
        (Some(object), None) => Ok(ComputedPreviewTarget::Object(object)),
        (None, Some(data)) => Ok(ComputedPreviewTarget::Data(data)),
        (Some(_), Some(_)) => Err(AppError::InvalidOption(
            "Use either --object or --data, not both".to_string(),
        )),
        (None, None) => Err(AppError::MissingOptions(vec![
            "object".to_string(),
            "data".to_string(),
        ])),
    }
}

fn render_shared_list(
    tokens: &CommandTokenizer,
    fields: &SharedComputedFieldListRecord,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_json(fields)?,
        OutputFormat::Text => {
            fields.definitions.format_noreturn()?;
            render_state(&fields.state)?;
        }
    }
    Ok(())
}

fn render_mutation(
    tokens: &CommandTokenizer,
    mutation: &ComputedFieldMutationRecord,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_json(mutation)?,
        OutputFormat::Text => {
            mutation.definition.format_noreturn()?;
            render_state(&mutation.state)?;
        }
    }
    Ok(())
}

fn render_definition(
    tokens: &CommandTokenizer,
    definition: &ComputedFieldRecord,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_json(definition)?,
        OutputFormat::Text => definition.format_noreturn()?,
    }
    Ok(())
}

fn render_preview(
    tokens: &CommandTokenizer,
    preview: &ComputedFieldPreviewRecord,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_json(preview)?,
        OutputFormat::Text => {
            append_key_value("Value", preview.value.to_string(), 10)?;
            if let Some(error) = &preview.error {
                append_key_value("Error", &error.message, 10)?;
                append_key_value("Code", &error.code, 10)?;
                if let Some(path) = &error.path {
                    append_key_value("Path", path, 10)?;
                }
            }
        }
    }
    Ok(())
}

fn render_state(state: &ClassComputationStateRecord) -> Result<(), AppError> {
    append_line("")?;
    append_key_value("Rebuild", &state.rebuild_status, 12)?;
    append_key_value("Revision", state.evaluation_revision, 12)?;
    if let Some(task_id) = state.active_task_id {
        append_key_value("Task", task_id, 12)?;
    }
    if let Some(error) = &state.last_error {
        append_key_value("Last error", error, 12)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::preview_target;
    use crate::services::ComputedPreviewTarget;

    #[test]
    fn preview_requires_exactly_one_target() {
        assert!(matches!(
            preview_target(Some("host-1".to_string()), None),
            Ok(ComputedPreviewTarget::Object(_))
        ));
        assert!(matches!(
            preview_target(None, Some(json!({"load": 1}))),
            Ok(ComputedPreviewTarget::Data(_))
        ));
        assert!(preview_target(None, None).is_err());
        assert!(preview_target(Some("host-1".to_string()), Some(json!({}))).is_err());
    }
}
