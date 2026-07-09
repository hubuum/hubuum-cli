use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, first_positional_or, render_json_record, render_list_page, required_i64,
    CliCommand,
};
use crate::autocomplete::event_delivery_ids;
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["event-delivery"],
            catalog_command(
                "list",
                EventDeliveryList::default(),
                docs("List event deliveries"),
            ),
        )
        .add_command(
            &["event-delivery"],
            catalog_command(
                "show",
                EventDeliveryShow::default(),
                docs("Show event delivery details"),
            ),
        )
        .add_command(
            &["event-delivery"],
            catalog_command(
                "health",
                EventDeliveryHealth::default(),
                docs("Show event delivery health"),
            ),
        )
        .add_command(
            &["event-delivery"],
            catalog_command(
                "retry",
                EventDeliveryRetry::default(),
                docs("Retry an event delivery"),
            ),
        )
        .add_command(
            &["event-delivery"],
            catalog_command(
                "dead",
                EventDeliveryDead::default(),
                docs("Mark an event delivery dead"),
            ),
        );
}

fn docs(about: &'static str) -> CommandDocs {
    CommandDocs {
        about: Some(about),
        ..CommandDocs::default()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventDeliveryList {
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for EventDeliveryList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [],
        )?;
        render_list_page(tokens, &services.gateway().event_deliveries(&list_query)?)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventDeliveryShow {
    #[option(
        long = "id",
        help = "Event delivery ID",
        autocomplete = "event_delivery_ids"
    )]
    pub id: Option<i64>,
}

impl CliCommand for EventDeliveryShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = first_positional_or(query.id, tokens, "id")?;
        render_json_record(
            tokens,
            &services
                .gateway()
                .event_delivery(required_i64(query.id, "id")?)?,
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventDeliveryHealth {}

impl CliCommand for EventDeliveryHealth {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_json_record(tokens, &services.gateway().event_delivery_health()?)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventDeliveryRetry {
    #[option(
        long = "id",
        help = "Event delivery ID",
        autocomplete = "event_delivery_ids"
    )]
    pub id: Option<i64>,
}

impl CliCommand for EventDeliveryRetry {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = first_positional_or(query.id, tokens, "id")?;
        render_json_record(
            tokens,
            &services
                .gateway()
                .retry_event_delivery(required_i64(query.id, "id")?)?,
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventDeliveryDead {
    #[option(
        long = "id",
        help = "Event delivery ID",
        autocomplete = "event_delivery_ids"
    )]
    pub id: Option<i64>,
}

impl CliCommand for EventDeliveryDead {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = first_positional_or(query.id, tokens, "id")?;
        render_json_record(
            tokens,
            &services
                .gateway()
                .dead_event_delivery(required_i64(query.id, "id")?)?,
        )
    }
}
