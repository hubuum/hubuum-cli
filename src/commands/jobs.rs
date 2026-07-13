use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, option_or_pos, CliCommand};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, TaskLookupInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    // `bg` is a true alias of `jobs`: both prefixes reuse the same command structs.
    register_group(builder, "jobs");
    register_group(builder, "bg");
}

fn register_group(builder: &mut CommandCatalogBuilder, prefix: &'static str) {
    builder
        .add_command(
            &[prefix],
            catalog_command(
                "list",
                JobsList::default(),
                CommandDocs {
                    about: Some("List local background jobs"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &[prefix],
            catalog_command(
                "show",
                JobsShow::default(),
                CommandDocs {
                    about: Some("Show a local background job"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &[prefix],
            catalog_command(
                "output",
                JobsOutput::default(),
                CommandDocs {
                    about: Some("View output of a background job"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &[prefix],
            catalog_command(
                "watch",
                JobsWatch::default(),
                CommandDocs {
                    about: Some("Watch a server task in the background"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &[prefix],
            catalog_command(
                "forget",
                JobsForget::default(),
                CommandDocs {
                    about: Some("Stop tracking a local background job"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct JobsList {}

impl CliCommand for JobsList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let jobs = services.background().list_jobs();

        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&jobs)?)?,
            OutputFormat::Text => {
                jobs.format_noreturn()?;
                if !jobs.is_empty() {
                    append_line("Use 'jobs show <id>' for local details.")?;
                    append_line("Use 'jobs output <id>' to view task results.")?;
                    append_line("Use 'task show <task-id>' or 'task events <task-id>' for server status and history.")?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct JobsShow {
    #[option(short = "i", long = "id", help = "Local background job ID")]
    pub id: Option<u64>,
}

impl CliCommand for JobsShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let id = query
            .id
            .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?;
        let job = services
            .background()
            .job(id)
            .ok_or_else(|| AppError::EntityNotFound(format!("background job {id}")))?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&job)?)?,
            OutputFormat::Text => {
                job.format_noreturn()?;
                append_line(format!(
                    "Use 'task show {}' for server status and 'task events {}' for history.",
                    job.task_id, job.task_id
                ))?;
                if job.state == "completed" {
                    append_line(format!("View results with: jobs output {}", id))?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct JobsOutput {
    #[option(short = "i", long = "id", help = "Local background job ID")]
    pub id: Option<u64>,
}

impl CliCommand for JobsOutput {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let local_id = query
            .id
            .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?;
        let job = services
            .background()
            .job(local_id)
            .ok_or_else(|| AppError::EntityNotFound(format!("background job {local_id}")))?;
        let output = services.gateway().task_output(job.task_id)?;
        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&output)?)?,
            OutputFormat::Text => {
                for line in output.render_lines() {
                    append_line(line)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct JobsWatch {
    #[option(short = "t", long = "task", help = "Server task ID")]
    pub task: Option<i32>,
}

impl CliCommand for JobsWatch {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.task = option_or_pos(query.task, tokens, 0, "task")?;
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
                    "Use 'jobs show {}' for local watcher state.",
                    registration.local_id
                ))?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct JobsForget {
    #[option(short = "i", long = "id", help = "Local background job ID")]
    pub id: Option<u64>,
}

impl CliCommand for JobsForget {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        services.background().require_enabled()?;
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
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
