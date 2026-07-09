use cli_command_derive::CommandArgs;
use hubuum_client::{EventSinkKind, NewEventSink, UpdateEventSink};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, name_or_first_pos, render_json_record, render_list_page, required_str,
    CliCommand,
};
use crate::autocomplete::{event_sink_kinds, event_sinks};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::append_json_message;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["event-sink"],
            catalog_command("list", EventSinkList::default(), docs("List event sinks")),
        )
        .add_command(
            &["event-sink"],
            catalog_command(
                "show",
                EventSinkShow::default(),
                docs("Show event sink details"),
            ),
        )
        .add_command(
            &["event-sink"],
            catalog_command(
                "create",
                EventSinkCreate::default(),
                docs("Create an event sink"),
            ),
        )
        .add_command(
            &["event-sink"],
            catalog_command(
                "update",
                EventSinkUpdate::default(),
                docs("Update an event sink"),
            ),
        )
        .add_command(
            &["event-sink"],
            catalog_command(
                "delete",
                EventSinkDelete::default(),
                docs("Delete an event sink"),
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
pub struct EventSinkList {
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for EventSinkList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [],
        )?;
        render_list_page(tokens, &services.gateway().event_sinks(&list_query)?)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkShow {
    #[option(long = "name", help = "Event sink name", autocomplete = "event_sinks")]
    pub name: Option<String>,
}

impl CliCommand for EventSinkShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = name_or_first_pos(query.name, tokens);
        let sink = services
            .gateway()
            .event_sink_by_name(required_str(query.name.as_deref(), "name")?)?;
        render_json_record(tokens, &sink)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkCreate {
    #[option(long = "name", help = "Event sink name")]
    pub name: String,
    #[option(
        long = "kind",
        help = "webhook, amqp, valkey_stream, or email",
        autocomplete = "event_sink_kinds"
    )]
    pub kind: String,
    #[option(long = "config", help = "Sink config JSON object", value_source = true)]
    pub config: Option<String>,
    #[option(long = "secret-ref", help = "Secret reference")]
    pub secret_ref: Option<String>,
    #[option(long = "enabled", help = "Enabled flag", flag = true)]
    pub enabled: Option<bool>,
}

impl CliCommand for EventSinkCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let sink = services.gateway().create_event_sink(NewEventSink {
            name: query.name,
            kind: parse_event_sink_kind(&query.kind)?,
            config: parse_json_object(query.config)?,
            enabled: query.enabled.or(Some(true)),
            secret_ref: query.secret_ref,
        })?;
        render_json_record(tokens, &sink)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkUpdate {
    #[option(long = "sink", help = "Event sink name", autocomplete = "event_sinks")]
    pub current_name: Option<String>,
    #[option(long = "name", help = "New name")]
    pub name: Option<String>,
    #[option(long = "kind", help = "New kind", autocomplete = "event_sink_kinds")]
    pub kind: Option<String>,
    #[option(
        long = "config",
        help = "Replacement config JSON object",
        value_source = true
    )]
    pub config: Option<String>,
    #[option(long = "secret-ref", help = "Secret reference")]
    pub secret_ref: Option<String>,
    #[option(
        long = "clear-secret-ref",
        help = "Clear the secret reference",
        flag = true
    )]
    pub clear_secret_ref: Option<bool>,
    #[option(long = "enabled", help = "Enabled flag")]
    pub enabled: Option<bool>,
}

impl CliCommand for EventSinkUpdate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.current_name = name_or_first_pos(query.current_name, tokens);
        if query.clear_secret_ref.unwrap_or(false) {
            return Err(AppError::InvalidOption(
                "clear-secret-ref is not exposed by the official hubuum_client update type yet"
                    .to_string(),
            ));
        }
        let sink = services.gateway().update_event_sink(
            required_str(query.current_name.as_deref(), "name")?,
            UpdateEventSink {
                name: query.name,
                kind: query
                    .kind
                    .as_deref()
                    .map(parse_event_sink_kind)
                    .transpose()?,
                config: parse_json_object(query.config)?,
                enabled: query.enabled,
                secret_ref: query.secret_ref,
            },
        )?;
        render_json_record(tokens, &sink)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkDelete {
    #[option(long = "name", help = "Event sink name", autocomplete = "event_sinks")]
    pub name: Option<String>,
}

impl CliCommand for EventSinkDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = name_or_first_pos(query.name, tokens);
        services
            .gateway()
            .delete_event_sink_by_name(required_str(query.name.as_deref(), "name")?)?;
        append_json_message("event sink deleted")
    }
}

pub(super) fn parse_json_object(input: Option<String>) -> Result<Option<Value>, AppError> {
    input
        .map(|raw| {
            let value: Value = serde_json::from_str(&raw)?;
            if !value.is_object() {
                return Err(AppError::ParseError(
                    "JSON value must be an object".to_string(),
                ));
            }
            Ok(value)
        })
        .transpose()
}

pub(super) fn parse_event_sink_kind(value: &str) -> Result<EventSinkKind, AppError> {
    serde_json::from_value(Value::String(value.to_string())).map_err(AppError::from)
}
