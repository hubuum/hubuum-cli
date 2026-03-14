use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{build_list_query, desired_format, render_list_page, CliCommand};
use crate::autocomplete::task_event_sort;
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, TaskLookupInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["task"],
            catalog_command(
                "show",
                TaskShow::default(),
                CommandDocs {
                    about: Some("Show task details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["task"],
            catalog_command(
                "events",
                TaskEvents::default(),
                CommandDocs {
                    about: Some("List task events"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["task"],
            catalog_command(
                "queue",
                TaskQueue::default(),
                CommandDocs {
                    about: Some("Show task queue state"),
                    ..CommandDocs::default()
                },
            ),
        );
}

trait GetTaskId {
    fn task_id(&self) -> Option<i32>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct TaskShow {
    #[option(short = "i", long = "id", help = "Task ID")]
    pub id: Option<i32>,
}

impl GetTaskId for &TaskShow {
    fn task_id(&self) -> Option<i32> {
        self.id
    }
}

impl CliCommand for TaskShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = task_id_or_pos(&query, tokens, 0)?;
        let task = services.gateway().task(TaskLookupInput {
            task_id: query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&task)?)?,
            OutputFormat::Text => task.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct TaskEvents {
    #[option(short = "i", long = "id", help = "Task ID")]
    pub id: Option<i32>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "task_event_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl GetTaskId for &TaskEvents {
    fn task_id(&self) -> Option<i32> {
        self.id
    }
}

impl CliCommand for TaskEvents {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = task_id_or_pos(&query, tokens, 0)?;
        let list_query = build_list_query(&[], &query.sort_clauses, query.limit, query.cursor, [])?;
        let events = services.gateway().task_events(
            TaskLookupInput {
                task_id: query
                    .id
                    .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
            },
            &list_query,
        )?;
        render_list_page(tokens, &events)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct TaskQueue {}

impl CliCommand for TaskQueue {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let state = services.gateway().task_queue_state()?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&state)?)?,
            OutputFormat::Text => state.format_noreturn()?,
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
