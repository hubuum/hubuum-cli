use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, render_list_page, CliCommand};
use crate::autocomplete::{audit_resources, event_actions};
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
                        "Lists audit events scoped to a resource such as a namespace, class, object, user, group, template, or remote target.",
                    ),
                    examples: Some("--resource namespace --id 12\n--resource object --class-id 7 --id 42"),
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
    #[option(long = "id", help = "Audit event ID")]
    pub id: Option<i64>,
}

impl CliCommand for AuditShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        if query.id.is_none() {
            query.id = positional_i64(tokens, 0, "id")?;
        }
        let event = services
            .gateway()
            .audit_event_by_id(required_i64(query.id, "id")?)?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&event)?)?,
            OutputFormat::Text => event.format_noreturn()?,
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct AuditResource {
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

impl CliCommand for AuditResource {
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

fn positional_i64(
    tokens: &CommandTokenizer,
    pos: usize,
    name: &str,
) -> Result<Option<i64>, AppError> {
    tokens
        .get_positionals()
        .get(pos)
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| AppError::ParseError(format!("{name} must be an integer")))
        })
        .transpose()
}

fn required_i64(value: Option<i64>, name: &str) -> Result<i64, AppError> {
    value.ok_or_else(|| AppError::MissingOptions(vec![name.to_string()]))
}
