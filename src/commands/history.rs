use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{normalize_server_page_size, render_list_page, CliCommand};
use crate::autocomplete::{classes, objects_from_class};
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
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -history_id")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
}

impl CliCommand for ClassHistory {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.class = query
            .class
            .or_else(|| tokens.get_positionals().first().cloned());
        let class_name = query
            .class
            .as_deref()
            .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
        let history = services.gateway().history(
            HistoryScope::ClassName(class_name.to_string()),
            HistoryInput {
                limit: normalize_server_page_size(query.limit)?,
                sort: query.sort,
                cursor: query.cursor,
                at: query.at,
                include_total: query.include_total.unwrap_or(false),
            },
        )?;
        render_list_page(tokens, &history)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectHistory {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(
        long = "name",
        help = "Object name",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -history_id")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
}

impl CliCommand for ObjectHistory {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        let positionals = tokens.get_positionals();
        query.class = query.class.or_else(|| positionals.first().cloned());
        query.name = query.name.or_else(|| positionals.get(1).cloned());
        let class_name = query
            .class
            .as_deref()
            .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
        let object_name = query
            .name
            .as_deref()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        let history = services.gateway().history(
            HistoryScope::ObjectName {
                class_name: class_name.to_string(),
                object_name: object_name.to_string(),
            },
            HistoryInput {
                limit: normalize_server_page_size(query.limit)?,
                sort: query.sort,
                cursor: query.cursor,
                at: query.at,
                include_total: query.include_total.unwrap_or(false),
            },
        )?;
        render_list_page(tokens, &history)
    }
}
