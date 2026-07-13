use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, desired_format, option_or_pos, render_list_page, render_task_record,
    CliCommand,
};
use crate::autocomplete::{task_event_sort, task_kinds, task_statuses};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, ListTasksInput, TaskLookupInput};
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
        )
        .add_command(
            &["task"],
            catalog_command(
                "list",
                TaskList::default(),
                CommandDocs {
                    about: Some("List tasks"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["task"],
            catalog_command(
                "output",
                TaskOutputCmd::default(),
                CommandDocs {
                    about: Some("Show task output"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct TaskShow {
    #[option(short = "i", long = "id", help = "Task ID")]
    pub id: Option<i32>,
}

impl CliCommand for TaskShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let task = services.gateway().task(TaskLookupInput {
            task_id: query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
        })?;

        render_task_record(tokens, &task)
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
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
}

impl CliCommand for TaskEvents {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let list_query = build_list_query(
            &[],
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [],
        )?;
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
            OutputFormat::Json => append_line(to_string_pretty(&state)?)?,
            OutputFormat::Text => state.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct TaskList {
    #[option(
        long = "kind",
        help = "Filter by task kind",
        autocomplete = "task_kinds"
    )]
    pub kind: Option<String>,
    #[option(
        long = "status",
        help = "Filter by task status",
        autocomplete = "task_statuses"
    )]
    pub status: Option<String>,
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

impl CliCommand for TaskList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let tasks = services.gateway().list_tasks(ListTasksInput {
            kind: query.kind,
            status: query.status,
            limit: query.limit,
            cursor: query.cursor,
            include_total: query.include_total.unwrap_or(false),
        })?;
        render_list_page(tokens, &tasks)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct TaskOutputCmd {
    #[option(short = "i", long = "id", help = "Task ID")]
    pub id: Option<i32>,
}

impl CliCommand for TaskOutputCmd {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let task_id = query
            .id
            .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?;
        let output = services.gateway().task_output(task_id)?;

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
