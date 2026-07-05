use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{render_list_page, CliCommand};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::services::{AppServices, HistoryInput, HistoryScope};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["history"],
            catalog_command(
                "class",
                ClassHistory::default(),
                CommandDocs {
                    about: Some("List class temporal history"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "object",
                ObjectHistory::default(),
                CommandDocs {
                    about: Some("List object temporal history"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassHistory {
    #[option(long = "class-id", help = "Class ID")]
    pub class_id: i32,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -history_id")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for ClassHistory {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let history = services.gateway().history(
            HistoryScope::Class(query.class_id),
            HistoryInput {
                limit: query.limit,
                sort: query.sort,
                cursor: query.cursor,
                at: query.at,
            },
        )?;
        render_list_page(tokens, &history)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectHistory {
    #[option(long = "class-id", help = "Class ID")]
    pub class_id: i32,
    #[option(long = "object-id", help = "Object ID")]
    pub object_id: i32,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -history_id")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for ObjectHistory {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let history = services.gateway().history(
            HistoryScope::Object {
                class_id: query.class_id,
                object_id: query.object_id,
            },
            HistoryInput {
                limit: query.limit,
                sort: query.sort,
                cursor: query.cursor,
                at: query.at,
            },
        )?;
        render_list_page(tokens, &history)
    }
}
