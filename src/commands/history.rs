use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{
    normalize_server_page_size, option_or_pos, render_json_record, render_list_page, CliCommand,
};
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
        )
        .add_command(
            &["history"],
            catalog_command(
                "show",
                HistoryShow::default(),
                CommandDocs {
                    about: Some("Show one class or object history record"),
                    long_about: Some(
                        "Shows a complete class history record when --name is omitted, or an object history record when --name is supplied. Select exactly one version with --id or --at. ID lookup scans the selected resource's history because the current hubuum_client does not expose a direct history-id endpoint.",
                    ),
                    examples: Some(
                        "--class Hosts --name host.example.org --id 1498\n--class Hosts --name host.example.org --at 2026-07-21T20:17:03Z\n--class Hosts --id 42",
                    ),
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct HistoryShow {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(
        long = "name",
        help = "Object name; omit for class history",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(long = "id", help = "History record ID")]
    pub id: Option<i64>,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
}

enum HistorySelector {
    Id(i64),
    At(String),
}

impl CliCommand for HistoryShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let class_name = query
            .class
            .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
        let scope = match query.name {
            Some(object_name) => HistoryScope::ObjectName {
                class_name,
                object_name,
            },
            None => HistoryScope::ClassName(class_name),
        };

        let record = match history_selector(query.id, query.at)? {
            HistorySelector::Id(id) => services.gateway().history_record_by_id(scope, id)?,
            HistorySelector::At(at) => services.gateway().history_record_at(scope, &at)?,
        };

        render_json_record(tokens, &record)
    }
}

fn history_selector(id: Option<i64>, at: Option<String>) -> Result<HistorySelector, AppError> {
    match (id, at) {
        (Some(id), None) => Ok(HistorySelector::Id(id)),
        (None, Some(at)) => Ok(HistorySelector::At(at)),
        (Some(_), Some(_)) => Err(AppError::InvalidOption(
            "--id and --at are mutually exclusive".to_string(),
        )),
        (None, None) => Err(AppError::MissingOptions(
            vec!["one of id or at".to_string()],
        )),
    }
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

#[cfg(test)]
mod tests {
    use super::{history_selector, HistorySelector};

    #[test]
    fn history_show_accepts_exactly_one_selector() {
        assert!(matches!(
            history_selector(Some(1498), None).expect("id should be accepted"),
            HistorySelector::Id(1498)
        ));
        assert!(matches!(
            history_selector(None, Some("2026-07-21T20:17:03Z".to_string()))
                .expect("timestamp should be accepted"),
            HistorySelector::At(at) if at == "2026-07-21T20:17:03Z"
        ));
        assert!(history_selector(Some(1498), Some("2026-07-21T20:17:03Z".to_string())).is_err());
        assert!(history_selector(None, None).is_err());
    }
}
