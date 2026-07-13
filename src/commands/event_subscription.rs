use cli_command_derive::CommandArgs;
use hubuum_client::{EventSubscriptionFilter, NewEventSubscription, UpdateEventSubscription};
use serde::{Deserialize, Serialize};
use serde_json::from_value;

use super::builder::{catalog_command, CommandDocs};
use super::event_sink::parse_json_object;
use super::{
    build_list_query, name_or_first_pos, render_json_record, render_list_page, required_str,
    CliCommand,
};
use crate::autocomplete::{
    collections, event_actions, event_entity_types, event_sinks, event_subscriptions,
};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::append_json_message;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["event", "subscription"],
            catalog_command(
                "list",
                EventSubscriptionList::default(),
                docs("List event subscriptions"),
            ),
        )
        .add_command(
            &["event", "subscription"],
            catalog_command(
                "show",
                EventSubscriptionShow::default(),
                docs("Show event subscription details"),
            ),
        )
        .add_command(
            &["event", "subscription"],
            catalog_command(
                "create",
                EventSubscriptionCreate::default(),
                docs("Create an event subscription"),
            ),
        )
        .add_command(
            &["event", "subscription"],
            catalog_command(
                "update",
                EventSubscriptionUpdate::default(),
                docs("Update an event subscription"),
            ),
        )
        .add_command(
            &["event", "subscription"],
            catalog_command(
                "delete",
                EventSubscriptionDelete::default(),
                docs("Delete an event subscription"),
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
pub struct EventSubscriptionList {
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
}

impl CliCommand for EventSubscriptionList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let collection_id = resolve_collection_id(services, query.collection)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [],
        )?;
        render_list_page(
            tokens,
            &services
                .gateway()
                .event_subscriptions(collection_id, &list_query)?,
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionShow {
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(
        long = "name",
        help = "Subscription name",
        autocomplete = "event_subscriptions"
    )]
    pub name: Option<String>,
}

impl CliCommand for EventSubscriptionShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = name_or_first_pos(query.name, tokens);
        let collection_id = resolve_collection_id(services, query.collection)?;
        let record = services.gateway().event_subscription_by_name(
            collection_id,
            required_str(query.name.as_deref(), "name")?,
        )?;
        render_json_record(tokens, &record)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionCreate {
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "sink", help = "Sink name", autocomplete = "event_sinks")]
    pub sink: Option<String>,
    #[option(long = "name", help = "Subscription name")]
    pub name: String,
    #[option(long = "description", help = "Description")]
    pub description: Option<String>,
    #[option(
        long = "entity-types",
        help = "Comma-separated entity types",
        autocomplete = "event_entity_types"
    )]
    pub entity_types: String,
    #[option(
        long = "actions",
        help = "Comma-separated actions",
        autocomplete = "event_actions"
    )]
    pub actions: String,
    #[option(long = "filter", help = "Filter JSON object", value_source = true)]
    pub filter: Option<String>,
    #[option(long = "routing", help = "Routing JSON object", value_source = true)]
    pub routing: Option<String>,
    #[option(long = "enabled", help = "Enabled flag", flag = true)]
    pub enabled: Option<bool>,
}

impl CliCommand for EventSubscriptionCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let collection_id = resolve_collection_id(services, query.collection)?;
        let sink_id = resolve_sink_id(services, query.sink)?;
        let record = services.gateway().create_event_subscription(
            collection_id,
            NewEventSubscription {
                sink_id: sink_id.into(),
                name: query.name,
                entity_types: split_csv(&query.entity_types),
                actions: split_csv(&query.actions),
                description: query.description,
                routing: parse_json_object(query.routing)?,
                enabled: query.enabled.or(Some(true)),
                filter: parse_subscription_filter(query.filter)?,
            },
        )?;
        render_json_record(tokens, &record)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionUpdate {
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(
        long = "subscription",
        help = "Subscription name",
        autocomplete = "event_subscriptions"
    )]
    pub subscription: Option<String>,
    #[option(long = "sink", help = "Sink name", autocomplete = "event_sinks")]
    pub sink: Option<String>,
    #[option(
        long = "name",
        help = "Subscription name",
        autocomplete = "event_subscriptions"
    )]
    pub name: Option<String>,
    #[option(long = "description", help = "Description")]
    pub description: Option<String>,
    #[option(
        long = "entity-types",
        help = "Comma-separated entity types",
        autocomplete = "event_entity_types"
    )]
    pub entity_types: Option<String>,
    #[option(
        long = "actions",
        help = "Comma-separated actions",
        autocomplete = "event_actions"
    )]
    pub actions: Option<String>,
    #[option(long = "filter", help = "Filter JSON object", value_source = true)]
    pub filter: Option<String>,
    #[option(long = "routing", help = "Routing JSON object", value_source = true)]
    pub routing: Option<String>,
    #[option(long = "enabled", help = "Enabled flag")]
    pub enabled: Option<bool>,
}

impl CliCommand for EventSubscriptionUpdate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.subscription = name_or_first_pos(query.subscription, tokens);
        let collection_id = resolve_collection_id(services, query.collection)?;
        let record = services.gateway().update_event_subscription(
            collection_id,
            required_str(query.subscription.as_deref(), "subscription")?,
            UpdateEventSubscription {
                sink_id: resolve_optional_sink_id(services, query.sink)?.map(Into::into),
                name: query.name,
                description: query.description,
                entity_types: query.entity_types.map(|value| split_csv(&value)),
                actions: query.actions.map(|value| split_csv(&value)),
                routing: parse_json_object(query.routing)?,
                enabled: query.enabled,
                filter: parse_subscription_filter(query.filter)?,
            },
        )?;
        render_json_record(tokens, &record)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionDelete {
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "name", help = "Subscription name")]
    pub name: Option<String>,
}

impl CliCommand for EventSubscriptionDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = name_or_first_pos(query.name, tokens);
        let collection_id = resolve_collection_id(services, query.collection)?;
        services.gateway().delete_event_subscription_by_name(
            collection_id,
            required_str(query.name.as_deref(), "name")?,
        )?;
        append_json_message("event subscription deleted")
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_subscription_filter(
    input: Option<String>,
) -> Result<Option<EventSubscriptionFilter>, AppError> {
    parse_json_object(input)?
        .map(from_value)
        .transpose()
        .map_err(AppError::from)
}

fn resolve_collection_id(
    services: &AppServices,
    collection: Option<String>,
) -> Result<i32, AppError> {
    collection
        .as_deref()
        .ok_or_else(|| AppError::MissingOptions(vec!["collection".to_string()]))
        .and_then(|name| services.gateway().collection_id_by_name(name))
}

fn resolve_sink_id(services: &AppServices, sink: Option<String>) -> Result<i32, AppError> {
    resolve_optional_sink_id(services, sink)?
        .ok_or_else(|| AppError::MissingOptions(vec!["sink".to_string()]))
}

fn resolve_optional_sink_id(
    services: &AppServices,
    sink: Option<String>,
) -> Result<Option<i32>, AppError> {
    sink.as_deref()
        .map(|name| services.gateway().event_sink_id_by_name(name).map(Some))
        .unwrap_or(Ok(None))
}
