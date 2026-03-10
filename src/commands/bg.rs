use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, TaskLookupInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["bg"],
            catalog_command(
                "list",
                BgList::default(),
                CommandDocs {
                    about: Some("List local background watchers"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["bg"],
            catalog_command(
                "show",
                BgShow::default(),
                CommandDocs {
                    about: Some("Show a local background watcher"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["bg"],
            catalog_command(
                "watch",
                BgWatch::default(),
                CommandDocs {
                    about: Some("Watch a server task in the background"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["bg"],
            catalog_command(
                "forget",
                BgForget::default(),
                CommandDocs {
                    about: Some("Stop tracking a local background watcher"),
                    ..CommandDocs::default()
                },
            ),
        );
}

trait GetLocalId {
    fn local_id(&self) -> Option<u64>;
}

trait GetTaskId {
    fn task_id(&self) -> Option<i32>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct BgList {}

impl CliCommand for BgList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let jobs = services.background().list_jobs();

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&jobs)?)?,
            OutputFormat::Text => {
                jobs.format_noreturn()?;
                if !jobs.is_empty() {
                    append_line("Use 'bg show <id>' for local details.")?;
                    append_line("Use 'task show <task-id>' or 'task events <task-id>' for server status and history.")?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct BgShow {
    #[option(short = "i", long = "id", help = "Local background job ID")]
    pub id: Option<u64>,
}

impl GetLocalId for &BgShow {
    fn local_id(&self) -> Option<u64> {
        self.id
    }
}

impl CliCommand for BgShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.id = local_id_or_pos(&query, tokens, 0)?;
        let id = query
            .id
            .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?;
        let job = services
            .background()
            .job(id)
            .ok_or_else(|| AppError::EntityNotFound(format!("background job {id}")))?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&job)?)?,
            OutputFormat::Text => {
                job.format_noreturn()?;
                append_line(format!(
                    "Use 'task show {}' for server status and 'task events {}' for history.",
                    job.task_id, job.task_id
                ))?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct BgWatch {
    #[option(short = "t", long = "task", help = "Server task ID")]
    pub task: Option<i32>,
}

impl GetTaskId for &BgWatch {
    fn task_id(&self) -> Option<i32> {
        self.task
    }
}

impl CliCommand for BgWatch {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.task = task_id_or_pos(&query, tokens, 0)?;
        let task_id = query
            .task
            .ok_or_else(|| AppError::MissingOptions(vec!["task".to_string()]))?;
        let task = services.gateway().task(TaskLookupInput { task_id })?;
        let registration = services
            .background()
            .watch_task(task, format!("task {task_id}"))
            .ok_or_else(|| {
                AppError::CommandExecutionError(
                    "Background jobs are only available in the interactive REPL".to_string(),
                )
            })?;

        let message = if registration.created {
            format!(
                "Watching task {} as local background job {}",
                registration.task_id, registration.local_id
            )
        } else {
            format!(
                "Already watching task {} as local background job {}",
                registration.task_id, registration.local_id
            )
        };

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => {
                append_line(message)?;
                append_line(format!(
                    "Use 'bg show {}' for local watcher state.",
                    registration.local_id
                ))?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct BgForget {
    #[option(short = "i", long = "id", help = "Local background job ID")]
    pub id: Option<u64>,
}

impl GetLocalId for &BgForget {
    fn local_id(&self) -> Option<u64> {
        self.id
    }
}

impl CliCommand for BgForget {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.id = local_id_or_pos(&query, tokens, 0)?;
        let id = query
            .id
            .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?;
        if !services.background().forget_job(id) {
            return Err(AppError::EntityNotFound(format!("background job {id}")));
        }

        let message = format!("Forgot background job {id}");
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

fn local_id_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<u64>, AppError>
where
    U: GetLocalId,
{
    let positional = tokens.get_positionals().get(pos);
    if query.local_id().is_none() {
        if let Some(value) = positional {
            return Ok(Some(value.parse()?));
        }
        return Err(AppError::MissingOptions(vec!["id".to_string()]));
    }
    Ok(query.local_id())
}

fn task_id_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<i32>, AppError>
where
    U: GetTaskId,
{
    let positional = tokens.get_positionals().get(pos);
    if query.task_id().is_none() {
        if let Some(value) = positional {
            return Ok(Some(value.parse()?));
        }
        return Err(AppError::MissingOptions(vec!["task".to_string()]));
    }
    Ok(query.task_id())
}
