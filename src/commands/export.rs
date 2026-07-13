use std::fs::read_to_string;

use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{parse_task_submit_options, run_task_backed};
use super::{
    build_list_query, desired_format, render_list_page, required_option_or_pos, CliCommand,
};
use crate::autocomplete::{
    classes, collections, export_content_types, export_missing_data_policies, export_scope_kinds,
    export_sort, export_templates, export_where, objects_from_class,
};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{
    AppServices, CreateExportTemplateInput, RunExportInput, UpdateExportTemplateInput,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["export"],
            catalog_command(
                "list",
                ExportList::default(),
                CommandDocs {
                    about: Some("List available export templates"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["export"],
            catalog_command(
                "show",
                ExportShow::default(),
                CommandDocs {
                    about: Some("Show export template details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["export"],
            catalog_command(
                "create",
                ExportCreate::default(),
                CommandDocs {
                    about: Some("Create an export template"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["export"],
            catalog_command(
                "modify",
                ExportModify::default(),
                CommandDocs {
                    about: Some("Modify an export template"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["export"],
            catalog_command(
                "delete",
                ExportDelete::default(),
                CommandDocs {
                    about: Some("Delete an export template"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["export"],
            catalog_command(
                "run",
                ExportRun::default(),
                CommandDocs {
                    about: Some("Run an export"),
                    long_about: Some(
                        "Run an export for a given scope, optionally using a named export template.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ExportList {
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "export_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "export_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
}

impl CliCommand for ExportList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [],
        )?;
        let exports = services.gateway().list_export_templates(&list_query)?;
        render_list_page(tokens, &exports)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ExportShow {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the export template",
        autocomplete = "export_templates"
    )]
    pub name: Option<String>,
}

impl CliCommand for ExportShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        let export = services.gateway().export_template(&name)?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&export)?)?,
            OutputFormat::Text => export.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ExportCreate {
    #[option(short = "n", long = "name", help = "Name of the export template")]
    pub name: String,
    #[option(
        short = "N",
        long = "collection",
        help = "Collection containing the export template",
        autocomplete = "collections"
    )]
    pub collection: String,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the export template"
    )]
    pub description: String,
    #[option(
        short = "c",
        long = "content-type",
        help = "Rendered content type (application/json, text/plain, text/html, text/csv)",
        autocomplete = "export_content_types"
    )]
    pub content_type: String,
    #[option(
        short = "t",
        long = "template",
        help = "Template source as a string",
        value_source = true
    )]
    pub template: Option<String>,
    #[option(short = "f", long = "file", help = "Read template source from a file")]
    pub file: Option<String>,
}

impl CliCommand for ExportCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let template = read_template_source(query.template, query.file)?;
        let export = services
            .gateway()
            .create_export_template(CreateExportTemplateInput {
                name: query.name,
                collection: query.collection,
                description: query.description,
                content_type: query.content_type,
                template,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&export)?)?,
            OutputFormat::Text => export.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ExportModify {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the export template",
        autocomplete = "export_templates"
    )]
    pub name: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the export template")]
    pub rename: Option<String>,
    #[option(
        short = "N",
        long = "collection",
        help = "Move the export template to another collection",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the export template"
    )]
    pub description: Option<String>,
    #[option(
        short = "t",
        long = "template",
        help = "Template source as a string",
        value_source = true
    )]
    pub template: Option<String>,
    #[option(short = "f", long = "file", help = "Read template source from a file")]
    pub file: Option<String>,
}

impl CliCommand for ExportModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        let template = read_optional_template_source(query.template, query.file)?;
        let export = services
            .gateway()
            .update_export_template(UpdateExportTemplateInput {
                name,
                rename: query.rename,
                collection: query.collection,
                description: query.description,
                template,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&export)?)?,
            OutputFormat::Text => export.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ExportDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the export template",
        autocomplete = "export_templates"
    )]
    pub name: Option<String>,
}

impl CliCommand for ExportDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        services.gateway().delete_export_template(&name)?;

        let message = format!("Export template '{name}' deleted");
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ExportRun {
    #[option(
        short = "t",
        long = "template",
        help = "Named export template to use",
        autocomplete = "export_templates"
    )]
    pub template: Option<String>,
    #[option(
        short = "s",
        long = "scope",
        help = "Export scope kind",
        autocomplete = "export_scope_kinds"
    )]
    pub scope: String,
    #[option(
        short = "c",
        long = "class",
        help = "Class name for scoped exports",
        autocomplete = "classes"
    )]
    pub class: Option<String>,
    #[option(
        short = "o",
        long = "object",
        help = "Object name for scoped exports",
        autocomplete = "objects_from_class"
    )]
    pub object: Option<String>,
    #[option(short = "q", long = "query", help = "Optional export query expression")]
    pub query: Option<String>,
    #[option(
        short = "m",
        long = "missing-data-policy",
        help = "Missing data policy",
        autocomplete = "export_missing_data_policies"
    )]
    pub missing_data_policy: Option<String>,
    #[option(short = "I", long = "max-items", help = "Maximum number of items")]
    pub max_items: Option<u64>,
    #[option(
        short = "B",
        long = "max-output-bytes",
        help = "Maximum output size in bytes"
    )]
    pub max_output_bytes: Option<u64>,
    #[option(long = "relation-depth", help = "Relation context depth for traversal")]
    pub relation_depth: Option<i32>,
    #[option(
        long = "include-related",
        help = "Include related objects: '<key>:<class_name>[:<max_depth>]' (repeatable)"
    )]
    pub include_related: Vec<String>,
    #[option(long = "wait", flag, help = "Wait for task completion")]
    pub wait: bool,
    #[option(long = "timeout", help = "Timeout in seconds when waiting")]
    pub timeout: Option<u64>,
    #[option(long = "poll-interval", help = "Poll interval in seconds when waiting")]
    pub poll_interval: Option<u64>,
}

impl CliCommand for ExportRun {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let opts = parse_task_submit_options(tokens)?;
        let input = RunExportInput {
            template: query.template,
            scope_kind: query.scope,
            class_name: query.class,
            object_name: query.object,
            query: query.query,
            missing_data_policy: query.missing_data_policy,
            max_items: query.max_items,
            max_output_bytes: query.max_output_bytes,
            relation_depth: query.relation_depth,
            include_related: query.include_related,
        };
        let task = services.gateway().submit_export(input)?;
        run_task_backed(
            services,
            tokens,
            format!("export {}", task.0.id),
            opts,
            task,
        )
    }
}

fn read_template_source(
    template: Option<String>,
    file: Option<String>,
) -> Result<String, AppError> {
    match (template, file) {
        (Some(template), None) => Ok(template),
        (None, Some(file)) => Ok(read_to_string(file)?),
        (Some(_), Some(_)) => Err(AppError::ParseError(
            "Use either --template or --file, not both".to_string(),
        )),
        (None, None) => Err(AppError::MissingOptions(vec!["template".to_string()])),
    }
}

fn read_optional_template_source(
    template: Option<String>,
    file: Option<String>,
) -> Result<Option<String>, AppError> {
    match (template, file) {
        (Some(template), None) => Ok(Some(template)),
        (None, Some(file)) => Ok(Some(read_to_string(file)?)),
        (Some(_), Some(_)) => Err(AppError::ParseError(
            "Use either --template or --file, not both".to_string(),
        )),
        (None, None) => Ok(None),
    }
}
