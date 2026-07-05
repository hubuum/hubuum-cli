use cli_command_derive::CommandArgs;
use hubuum_client::{EventSubscriptionFilter, NewEventSubscription, UpdateEventSubscription};
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::event_sink::{id_or_pos, parse_json_object, render_record, required_i32};
use super::{build_list_query, render_list_page, CliCommand};
use crate::autocomplete::{event_actions, event_entity_types, event_sinks, namespaces};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::append_json_message;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["event-subscription"],
            catalog_command(
                "list",
                EventSubscriptionList::default(),
                docs("List event subscriptions"),
            ),
        )
        .add_command(
            &["event-subscription"],
            catalog_command(
                "show",
                EventSubscriptionShow::default(),
                docs("Show event subscription details"),
            ),
        )
        .add_command(
            &["event-subscription"],
            catalog_command(
                "create",
                EventSubscriptionCreate::default(),
                docs("Create an event subscription"),
            ),
        )
        .add_command(
            &["event-subscription"],
            catalog_command(
                "update",
                EventSubscriptionUpdate::default(),
                docs("Update an event subscription"),
            ),
        )
        .add_command(
            &["event-subscription"],
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
    #[option(long = "namespace-id", help = "Namespace ID")]
    pub namespace_id: Option<i32>,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for EventSubscriptionList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let namespace_id = resolve_namespace_id(services, query.namespace_id, query.namespace)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [],
        )?;
        render_list_page(
            tokens,
            &services
                .gateway()
                .event_subscriptions(namespace_id, &list_query)?,
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionShow {
    #[option(long = "namespace-id", help = "Namespace ID")]
    pub namespace_id: Option<i32>,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(long = "id", help = "Subscription ID")]
    pub id: Option<i32>,
}

impl CliCommand for EventSubscriptionShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = id_or_pos(query.id, tokens)?;
        let namespace_id = resolve_namespace_id(services, query.namespace_id, query.namespace)?;
        let record = services
            .gateway()
            .event_subscription(namespace_id, required_i32(query.id, "id")?)?;
        render_record(tokens, &record)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionCreate {
    #[option(long = "namespace-id", help = "Namespace ID")]
    pub namespace_id: Option<i32>,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(long = "sink-id", help = "Sink ID")]
    pub sink_id: Option<i32>,
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
        let namespace_id = resolve_namespace_id(services, query.namespace_id, query.namespace)?;
        let sink_id = resolve_sink_id(services, query.sink_id, query.sink)?;
        let record = services.gateway().create_event_subscription(
            namespace_id,
            NewEventSubscription {
                sink_id,
                name: query.name,
                entity_types: split_csv(&query.entity_types),
                actions: split_csv(&query.actions),
                description: query.description,
                routing: parse_json_object(query.routing)?,
                enabled: query.enabled.or(Some(true)),
                filter: parse_subscription_filter(query.filter)?,
            },
        )?;
        render_record(tokens, &record)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionUpdate {
    #[option(long = "namespace-id", help = "Namespace ID")]
    pub namespace_id: Option<i32>,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(long = "id", help = "Subscription ID")]
    pub id: Option<i32>,
    #[option(long = "sink-id", help = "Sink ID")]
    pub sink_id: Option<i32>,
    #[option(long = "sink", help = "Sink name", autocomplete = "event_sinks")]
    pub sink: Option<String>,
    #[option(long = "name", help = "Subscription name")]
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
        query.id = id_or_pos(query.id, tokens)?;
        let namespace_id = resolve_namespace_id(services, query.namespace_id, query.namespace)?;
        let record = services.gateway().update_event_subscription(
            namespace_id,
            required_i32(query.id, "id")?,
            UpdateEventSubscription {
                sink_id: resolve_optional_sink_id(services, query.sink_id, query.sink)?,
                name: query.name,
                description: query.description,
                entity_types: query.entity_types.map(|value| split_csv(&value)),
                actions: query.actions.map(|value| split_csv(&value)),
                routing: parse_json_object(query.routing)?,
                enabled: query.enabled,
                filter: parse_subscription_filter(query.filter)?,
            },
        )?;
        render_record(tokens, &record)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSubscriptionDelete {
    #[option(long = "namespace-id", help = "Namespace ID")]
    pub namespace_id: Option<i32>,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(long = "id", help = "Subscription ID")]
    pub id: Option<i32>,
}

impl CliCommand for EventSubscriptionDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = id_or_pos(query.id, tokens)?;
        let namespace_id = resolve_namespace_id(services, query.namespace_id, query.namespace)?;
        services
            .gateway()
            .delete_event_subscription(namespace_id, required_i32(query.id, "id")?)?;
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
        .map(serde_json::from_value)
        .transpose()
        .map_err(AppError::from)
}

fn resolve_namespace_id(
    services: &AppServices,
    namespace_id: Option<i32>,
    namespace: Option<String>,
) -> Result<i32, AppError> {
    match (namespace_id, namespace) {
        (Some(id), None) => Ok(id),
        (None, Some(name)) => services.gateway().namespace_id_by_name(&name),
        (Some(_), Some(_)) => Err(AppError::DuplicateOptions(vec![
            "namespace-id".to_string(),
            "namespace".to_string(),
        ])),
        (None, None) => Err(AppError::MissingOptions(vec!["namespace".to_string()])),
    }
}

fn resolve_sink_id(
    services: &AppServices,
    sink_id: Option<i32>,
    sink: Option<String>,
) -> Result<i32, AppError> {
    resolve_optional_sink_id(services, sink_id, sink)?
        .ok_or_else(|| AppError::MissingOptions(vec!["sink".to_string()]))
}

fn resolve_optional_sink_id(
    services: &AppServices,
    sink_id: Option<i32>,
    sink: Option<String>,
) -> Result<Option<i32>, AppError> {
    match (sink_id, sink) {
        (Some(id), None) => Ok(Some(id)),
        (None, Some(name)) => services.gateway().event_sink_id_by_name(&name).map(Some),
        (Some(_), Some(_)) => Err(AppError::DuplicateOptions(vec![
            "sink-id".to_string(),
            "sink".to_string(),
        ])),
        (None, None) => Ok(None),
    }
}
