use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{build_list_query, desired_format, render_list_page, CliCommand};
use crate::autocomplete::{
    classes, namespaces, objects_from_class, report_missing_data_policies, report_scope_kinds,
    report_templates, report_where,
};
use crate::catalog::CommandCatalogBuilder;
use crate::domain::ReportOutput;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{
    AppServices, CreateReportTemplateInput, RunReportInput, UpdateReportTemplateInput,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["report"],
            catalog_command(
                "list",
                ReportList::default(),
                CommandDocs {
                    about: Some("List available report templates"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["report"],
            catalog_command(
                "show",
                ReportShow::default(),
                CommandDocs {
                    about: Some("Show report template details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["report"],
            catalog_command(
                "create",
                ReportCreate::default(),
                CommandDocs {
                    about: Some("Create a report template"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["report"],
            catalog_command(
                "modify",
                ReportModify::default(),
                CommandDocs {
                    about: Some("Modify a report template"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["report"],
            catalog_command(
                "delete",
                ReportDelete::default(),
                CommandDocs {
                    about: Some("Delete a report template"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["report"],
            catalog_command(
                "run",
                ReportRun::default(),
                CommandDocs {
                    about: Some("Run a report"),
                    long_about: Some(
                        "Run a report for a given scope, optionally using a named report template.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        );
}

trait GetReportName {
    fn report_name(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ReportList {
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "report_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for ReportList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(&query.where_clauses, query.limit, query.cursor, [])?;
        let reports = services.gateway().list_report_templates(&list_query)?;
        render_list_page(tokens, &reports)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ReportShow {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the report template",
        autocomplete = "report_templates"
    )]
    pub name: Option<String>,
}

impl GetReportName for &ReportShow {
    fn report_name(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for ReportShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = report_name_or_pos(&query, tokens, 0)?;
        let report = services.gateway().report_template(
            &query
                .name
                .clone()
                .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?,
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&report)?)?,
            OutputFormat::Text => report.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ReportCreate {
    #[option(short = "n", long = "name", help = "Name of the report template")]
    pub name: String,
    #[option(
        short = "N",
        long = "namespace",
        help = "Namespace containing the report template",
        autocomplete = "namespaces"
    )]
    pub namespace: String,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the report template"
    )]
    pub description: String,
    #[option(
        short = "c",
        long = "content-type",
        help = "Rendered content type (application/json, text/plain, text/html, text/csv)"
    )]
    pub content_type: String,
    #[option(short = "t", long = "template", help = "Template source as a string")]
    pub template: Option<String>,
    #[option(short = "f", long = "file", help = "Read template source from a file")]
    pub file: Option<String>,
}

impl CliCommand for ReportCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let template = read_template_source(query.template, query.file)?;
        let report = services
            .gateway()
            .create_report_template(CreateReportTemplateInput {
                name: query.name,
                namespace: query.namespace,
                description: query.description,
                content_type: query.content_type,
                template,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&report)?)?,
            OutputFormat::Text => report.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ReportModify {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the report template",
        autocomplete = "report_templates"
    )]
    pub name: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the report template")]
    pub rename: Option<String>,
    #[option(
        short = "N",
        long = "namespace",
        help = "Move the report template to another namespace",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the report template"
    )]
    pub description: Option<String>,
    #[option(short = "t", long = "template", help = "Template source as a string")]
    pub template: Option<String>,
    #[option(short = "f", long = "file", help = "Read template source from a file")]
    pub file: Option<String>,
}

impl GetReportName for &ReportModify {
    fn report_name(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for ReportModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = report_name_or_pos(&query, tokens, 0)?;
        let template = read_optional_template_source(query.template, query.file)?;
        let report = services
            .gateway()
            .update_report_template(UpdateReportTemplateInput {
                name: query
                    .name
                    .clone()
                    .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?,
                rename: query.rename,
                namespace: query.namespace,
                description: query.description,
                template,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&report)?)?,
            OutputFormat::Text => report.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ReportDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the report template",
        autocomplete = "report_templates"
    )]
    pub name: Option<String>,
}

impl GetReportName for &ReportDelete {
    fn report_name(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for ReportDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = report_name_or_pos(&query, tokens, 0)?;
        let name = query
            .name
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        services.gateway().delete_report_template(&name)?;

        let message = format!("Report template '{name}' deleted");
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ReportRun {
    #[option(
        short = "t",
        long = "template",
        help = "Named report template to use",
        autocomplete = "report_templates"
    )]
    pub template: Option<String>,
    #[option(
        short = "s",
        long = "scope",
        help = "Report scope kind",
        autocomplete = "report_scope_kinds"
    )]
    pub scope: String,
    #[option(
        short = "c",
        long = "class",
        help = "Class name for scoped reports",
        autocomplete = "classes"
    )]
    pub class: Option<String>,
    #[option(
        short = "o",
        long = "object",
        help = "Object name for scoped reports",
        autocomplete = "objects_from_class"
    )]
    pub object: Option<String>,
    #[option(short = "q", long = "query", help = "Optional report query expression")]
    pub query: Option<String>,
    #[option(
        short = "m",
        long = "missing-data-policy",
        help = "Missing data policy",
        autocomplete = "report_missing_data_policies"
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
}

impl CliCommand for ReportRun {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let output = services.gateway().run_report(RunReportInput {
            template: query.template,
            scope_kind: query.scope,
            class_name: query.class,
            object_name: query.object,
            query: query.query,
            missing_data_policy: query.missing_data_policy,
            max_items: query.max_items,
            max_output_bytes: query.max_output_bytes,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&output)?)?,
            OutputFormat::Text => match output {
                ReportOutput::Json { body } => append_line(serde_json::to_string_pretty(&body)?)?,
                ReportOutput::Rendered(rendered) => append_line(rendered.body)?,
            },
        }

        Ok(())
    }
}

fn report_name_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<String>, AppError>
where
    U: GetReportName,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.report_name().is_none() {
        if pos0.is_none() {
            return Err(AppError::MissingOptions(vec!["name".to_string()]));
        }
        return Ok(pos0.cloned());
    }
    Ok(query.report_name().clone())
}

fn read_template_source(
    template: Option<String>,
    file: Option<String>,
) -> Result<String, AppError> {
    match (template, file) {
        (Some(template), None) => Ok(template),
        (None, Some(file)) => Ok(std::fs::read_to_string(file)?),
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
        (None, Some(file)) => Ok(Some(std::fs::read_to_string(file)?)),
        (Some(_), Some(_)) => Err(AppError::ParseError(
            "Use either --template or --file, not both".to_string(),
        )),
        (None, None) => Ok(None),
    }
}
