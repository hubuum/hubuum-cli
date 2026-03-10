use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, SubmitImportInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["import"],
            catalog_command(
                "submit",
                ImportSubmit::default(),
                CommandDocs {
                    about: Some("Submit an import request"),
                    long_about: Some("Submit an import request from a JSON file."),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["import"],
            catalog_command(
                "show",
                ImportShow::default(),
                CommandDocs {
                    about: Some("Show import task details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["import"],
            catalog_command(
                "results",
                ImportResults::default(),
                CommandDocs {
                    about: Some("List import results"),
                    ..CommandDocs::default()
                },
            ),
        );
}

trait GetTaskId {
    fn task_id(&self) -> Option<i32>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ImportSubmit {
    #[option(short = "f", long = "file", help = "Path to import JSON file")]
    pub file: String,
    #[option(
        short = "k",
        long = "idempotency-key",
        help = "Optional idempotency key"
    )]
    pub idempotency_key: Option<String>,
}

impl CliCommand for ImportSubmit {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let request_json = std::fs::read_to_string(query.file)?;
        let task = services.gateway().submit_import(SubmitImportInput {
            request_json,
            idempotency_key: query.idempotency_key,
        })?;
        let watcher = services
            .background()
            .watch_task(task.clone(), format!("import {}", task.0.id));

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&task)?)?,
            OutputFormat::Text => {
                task.format_noreturn()?;
                if let Some(registration) = watcher {
                    let message = if registration.created {
                        format!(
                            "Watching import task {} as local background job {}",
                            registration.task_id, registration.local_id
                        )
                    } else {
                        format!(
                            "Already watching import task {} as local background job {}",
                            registration.task_id, registration.local_id
                        )
                    };
                    append_line(message)?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ImportShow {
    #[option(short = "i", long = "id", help = "Import task ID")]
    pub id: Option<i32>,
}

impl GetTaskId for &ImportShow {
    fn task_id(&self) -> Option<i32> {
        self.id
    }
}

impl CliCommand for ImportShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = task_id_or_pos(&query, tokens, 0)?;
        let task = services.gateway().import_task(
            query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&task)?)?,
            OutputFormat::Text => task.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ImportResults {
    #[option(short = "i", long = "id", help = "Import task ID")]
    pub id: Option<i32>,
}

impl GetTaskId for &ImportResults {
    fn task_id(&self) -> Option<i32> {
        self.id
    }
}

impl CliCommand for ImportResults {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = task_id_or_pos(&query, tokens, 0)?;
        let results = services.gateway().import_results(
            query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&results)?)?,
            OutputFormat::Text => results.format_noreturn()?,
        }

        Ok(())
    }
}

fn task_id_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<i32>, AppError>
where
    U: GetTaskId,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.task_id().is_none() {
        if let Some(value) = pos0 {
            return Ok(Some(value.parse()?));
        }
        return Err(AppError::MissingOptions(vec!["id".to_string()]));
    }
    Ok(query.task_id())
}
