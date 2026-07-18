use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

use super::builder::{catalog_command, CommandDocs};
use super::{
    desired_format, normalize_server_page_size, option_or_pos, render_json_record,
    render_list_page, required_i64, CliCommand,
};
use crate::autocomplete::{
    audit_event_ids, audit_resource_names, audit_resources, classes, collections, event_actions,
};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, AuditListInput, AuditScope};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["audit"],
            catalog_command(
                "list",
                AuditList::default(),
                CommandDocs {
                    about: Some("List visible audit events"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["audit"],
            catalog_command(
                "show",
                AuditShow::default(),
                CommandDocs {
                    about: Some("Show a single audit event by id"),
                    long_about: Some(
                        "Looks for a visible audit event by id. The current hubuum_client does not expose a direct event-id endpoint, so this command scans recent visible audit pages until it finds the event.",
                    ),
                    examples: Some("12345\n--id 12345"),
                },
            ),
        )
        .add_command(
            &["audit"],
            catalog_command(
                "resource",
                AuditResource::default(),
                CommandDocs {
                    about: Some("Show audit events for a resource"),
                    long_about: Some(
                        "Lists audit events scoped to a resource such as a collection, class, object, user, group, template, or remote target.",
                    ),
                    examples: Some("--resource collection --name Math\n--resource object --class Hosts --name host.example.org"),
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct AuditList {
    #[option(
        long = "action",
        help = "Action filter",
        autocomplete = "event_actions"
    )]
    pub action: Option<String>,
    #[option(long = "actor-kind", help = "Actor kind filter")]
    pub actor_kind: Option<String>,
    #[option(long = "actor-user", help = "Actor user name")]
    pub actor_user: Option<String>,
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "occurred-after", help = "Lower occurred_at bound")]
    pub occurred_after: Option<String>,
    #[option(long = "occurred-before", help = "Upper occurred_at bound")]
    pub occurred_before: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -occurred_at")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for AuditList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let input = AuditListInput {
            action: query.action,
            actor_kind: query.actor_kind,
            actor_user_id: query
                .actor_user
                .as_deref()
                .map(|name| services.gateway().user_id_by_name(name))
                .transpose()?,
            collection_id: query
                .collection
                .as_deref()
                .map(|name| services.gateway().collection_id_by_name(name))
                .transpose()?,
            occurred_after: query.occurred_after,
            occurred_before: query.occurred_before,
            limit: normalize_server_page_size(query.limit)?,
            sort: query.sort,
            cursor: query.cursor,
        };
        let events = services.gateway().audit_events(AuditScope::Global, input)?;
        render_list_page(tokens, &events)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct AuditShow {
    #[option(long = "id", help = "Audit event ID", autocomplete = "audit_event_ids")]
    pub id: Option<i64>,
}

impl CliCommand for AuditShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let event = services
            .gateway()
            .audit_event_by_id(required_i64(query.id, "id")?)?;
        render_json_record(tokens, &event)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct AuditResource {
    #[option(
        long = "resource",
        help = "Resource: collection,class,object,user,group,template,remote-target",
        autocomplete = "audit_resources"
    )]
    pub resource: String,
    #[option(
        long = "name",
        help = "Resource name",
        autocomplete = "audit_resource_names"
    )]
    pub name: Option<String>,
    #[option(
        long = "class",
        help = "Class name for object events",
        autocomplete = "classes"
    )]
    pub class: Option<String>,
    #[option(
        long = "action",
        help = "Action filter",
        autocomplete = "event_actions"
    )]
    pub action: Option<String>,
    #[option(
        long = "collection",
        help = "Collection name filter",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -occurred_at")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for AuditResource {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let scope = services.gateway().audit_scope_by_name(
            &query.resource,
            query.name.as_deref(),
            query.class.as_deref(),
        )?;
        let events = services.gateway().audit_events(
            scope,
            AuditListInput {
                action: query.action,
                collection_id: query
                    .collection
                    .as_deref()
                    .map(|name| services.gateway().collection_id_by_name(name))
                    .transpose()?,
                limit: normalize_server_page_size(query.limit)?,
                sort: query.sort,
                cursor: query.cursor,
                ..AuditListInput::default()
            },
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&events)?)?,
            OutputFormat::Text => events.items.format_noreturn()?,
        }
        Ok(())
    }
}
