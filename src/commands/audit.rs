use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, render_list_page, CliCommand};
use crate::autocomplete::{audit_resources, event_actions, event_entity_types};
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
                    about: Some("Show audit events for a resource"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct AuditList {
    #[option(
        long = "entity-type",
        help = "Entity type filter",
        autocomplete = "event_entity_types"
    )]
    pub entity_type: Option<String>,
    #[option(long = "entity-id", help = "Entity ID filter")]
    pub entity_id: Option<i32>,
    #[option(
        long = "action",
        help = "Action filter",
        autocomplete = "event_actions"
    )]
    pub action: Option<String>,
    #[option(long = "actor-kind", help = "Actor kind filter")]
    pub actor_kind: Option<String>,
    #[option(long = "actor-user-id", help = "Actor principal ID filter")]
    pub actor_user_id: Option<i32>,
    #[option(long = "namespace-id", help = "Namespace ID filter")]
    pub namespace_id: Option<i32>,
    #[option(long = "occurred-after", help = "Lower occurred_at bound")]
    pub occurred_after: Option<String>,
    #[option(long = "occurred-before", help = "Upper occurred_at bound")]
    pub occurred_before: Option<String>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -occurred_at")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for AuditList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let events = services
            .gateway()
            .audit_events(AuditScope::Global, query.into())?;
        render_list_page(tokens, &events)
    }
}

impl From<AuditList> for AuditListInput {
    fn from(value: AuditList) -> Self {
        Self {
            entity_type: value.entity_type,
            entity_id: value.entity_id,
            action: value.action,
            actor_kind: value.actor_kind,
            actor_user_id: value.actor_user_id,
            namespace_id: value.namespace_id,
            occurred_after: value.occurred_after,
            occurred_before: value.occurred_before,
            limit: value.limit,
            sort: value.sort,
            cursor: value.cursor,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct AuditShow {
    #[option(
        long = "resource",
        help = "Resource: namespace,class,object,user,group,template,remote-target",
        autocomplete = "audit_resources"
    )]
    pub resource: String,
    #[option(long = "id", help = "Resource ID")]
    pub id: i32,
    #[option(long = "class-id", help = "Class ID for object events")]
    pub class_id: Option<i32>,
    #[option(
        long = "action",
        help = "Action filter",
        autocomplete = "event_actions"
    )]
    pub action: Option<String>,
    #[option(long = "namespace-id", help = "Namespace ID filter")]
    pub namespace_id: Option<i32>,
    #[option(long = "limit", help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -occurred_at")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
}

impl CliCommand for AuditShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let scope = match query.resource.as_str() {
            "namespace" => AuditScope::Namespace(query.id),
            "class" => AuditScope::Class(query.id),
            "object" => AuditScope::Object {
                class_id: query
                    .class_id
                    .ok_or_else(|| AppError::MissingOptions(vec!["class_id".to_string()]))?,
                object_id: query.id,
            },
            "user" => AuditScope::User(query.id),
            "group" => AuditScope::Group(query.id),
            "template" => AuditScope::Template(query.id),
            "remote-target" => AuditScope::RemoteTarget(query.id),
            other => return Err(AppError::InvalidOption(format!("resource={other}"))),
        };
        let events = services.gateway().audit_events(
            scope,
            AuditListInput {
                action: query.action,
                namespace_id: query.namespace_id,
                limit: query.limit,
                sort: query.sort,
                cursor: query.cursor,
                ..AuditListInput::default()
            },
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&events)?)?,
            OutputFormat::Text => events.items.format_noreturn()?,
        }
        Ok(())
    }
}
