use std::fs::read_to_string;

use cli_command_derive::CommandArgs;
use hubuum_client::{EventDeliveryPolicy, EventSinkKind, NewEventSink, UpdateEventSink};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, from_value, Value};

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, name_or_first_pos, render_json_record, render_list_page, required_str,
    CliCommand, PageSelection,
};
use crate::autocomplete::{collections, event_sink_kinds, event_sinks, webhook_targets};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::domain::{WebhookTarget, WebhookUrlSecret};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::append_json_message;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(&["event", "sink"], catalog_command("grant", EventSinkGrant::default(), docs("Allow a collection to use a global sink (administrator)")))
        .add_command(&["event", "sink"], catalog_command("revoke", EventSinkRevoke::default(), docs("Revoke a collection's global sink grant (administrator)")))
        .add_command(&["event", "sink"], catalog_command("collections", EventSinkCollections::default(), docs("List a global sink's direct grants (administrator)")))
        .add_command(
            &["event", "sink"],
            catalog_command("list", EventSinkList::default(), docs("List event sinks")),
        )
        .add_command(
            &["event", "sink"],
            catalog_command(
                "show",
                EventSinkShow::default(),
                docs("Show event sink details"),
            ),
        )
        .add_command(
            &["event", "sink"],
            catalog_command(
                "create",
                EventSinkCreate::default(),
                CommandDocs {
                    about: Some("Create an event sink"),
                    long_about: Some("Use --kind webhook --config for any HTTPS JSON POST receiver. Set config.url_secret_ref to a server secret alias, or set routing.url on subscriptions for a public destination. Generic webhooks send the event envelope, accept any HTTP 2xx, retry other statuses, and have no configured pacing unless you supply body_template, response rules, or --delivery-policy. Choose --target slack, mattermost, or discord for a preset and pass the URL alias with --url-secret-ref. Presets configure JSON-safe messages, provider acknowledgements, HTTP 429 cooldowns, and one-second pacing. Discord URLs stored on the server must include wait=true. Configure delivery workers and create an event subscription to select events; sink creation sends no message. See docs/webhooks.md for generic setup and equivalent full preset commands."),
                    examples: Some("event sink create --name inventory-hook --kind webhook --config '{\"url_secret_ref\":\"inventory_webhook\"}'\nevent sink create --name inventory-hook --kind webhook --config file://inventory-webhook.json --delivery-policy '{\"min_interval_ms\":1000}'\nevent sink create --name ops-slack --target slack --url-secret-ref ops_slack_webhook\nevent sink create --name ops-mattermost --target mattermost --url-secret-ref ops_mattermost_webhook\nevent sink create --name ops-discord --target discord --url-secret-ref ops_discord_webhook"),
                },
            ),
        )
        .add_command(
            &["event", "sink"],
            catalog_command(
                "update",
                EventSinkUpdate::default(),
                docs("Update an event sink"),
            ),
        )
        .add_command(
            &["event", "sink"],
            catalog_command(
                "delete",
                EventSinkDelete::default(),
                docs("Delete an event sink"),
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkGrant {
    #[option(long = "name", help = "Global sink name", autocomplete = "event_sinks")]
    pub name: String,
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: String,
}
impl CliCommand for EventSinkGrant {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        services.gateway().grant_event_sink(
            &query.name,
            services
                .gateway()
                .collection_id_by_name(&query.collection)?,
        )?;
        append_json_message("event sink grant created")
    }
}
#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkRevoke {
    #[option(long = "name", help = "Global sink name", autocomplete = "event_sinks")]
    pub name: String,
    #[option(
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: String,
}
impl CliCommand for EventSinkRevoke {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        services.gateway().revoke_event_sink(
            &query.name,
            services
                .gateway()
                .collection_id_by_name(&query.collection)?,
        )?;
        append_json_message("event sink grant revoked")
    }
}
#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkCollections {
    #[option(long = "name", help = "Global sink name", autocomplete = "event_sinks")]
    pub name: String,
}
impl CliCommand for EventSinkCollections {
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        render_json_record(
            tokens,
            &services.gateway().event_sink_collections(&query.name)?,
        )
    }
}

fn docs(about: &'static str) -> CommandDocs {
    CommandDocs {
        about: Some(about),
        ..CommandDocs::default()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkList {
    #[option(
        long = "collection",
        help = "Collection name for permitted destinations and owned webhook management",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
    #[option(
        long = "all",
        help = "Fetch and buffer all result pages before applying pipelines",
        flag = "true"
    )]
    pub all: Option<bool>,
}

impl CliCommand for EventSinkList {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [],
        )?
        .page_selection(PageSelection::from_all(query.all.unwrap_or(false)));
        let page = if let Some(collection) = query.collection {
            services.gateway().collection_event_sinks(
                services.gateway().collection_id_by_name(&collection)?,
                &list_query,
            )?
        } else {
            services.gateway().event_sinks(&list_query)?
        };
        render_list_page(tokens, &page)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkShow {
    #[option(
        long = "collection",
        help = "Collection name for permitted destinations and owned webhook management",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "name", help = "Event sink name", autocomplete = "event_sinks")]
    pub name: Option<String>,
}

impl CliCommand for EventSinkShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = name_or_first_pos(query.name, tokens);
        let name = required_str(query.name.as_deref(), "name")?;
        let sink = if let Some(collection) = query.collection {
            services.gateway().collection_event_sink_by_name(
                services.gateway().collection_id_by_name(&collection)?,
                name,
            )?
        } else {
            services.gateway().event_sink_by_name(name)?
        };
        render_json_record(tokens, &sink)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkCreate {
    #[option(
        long = "collection",
        help = "Collection name for permitted destinations and owned webhook management",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "name", help = "Event sink name")]
    pub name: String,
    #[option(
        long = "kind",
        help = "webhook, amqp, valkey_stream, or email",
        autocomplete = "event_sink_kinds"
    )]
    pub kind: Option<String>,
    #[option(
        long = "target",
        help = "Webhook preset: slack, mattermost, or discord",
        autocomplete = "webhook_targets"
    )]
    pub target: Option<String>,
    #[option(
        long = "url-secret-ref",
        help = "Server secret alias for the full HTTPS webhook URL (Discord: include wait=true)"
    )]
    pub url_secret_ref: Option<String>,
    #[option(
        long = "destination-url",
        help = "Fixed HTTPS URL for a webhook preset; supports file:// input"
    )]
    pub destination_url: Option<String>,
    #[option(long = "config", help = "Sink config JSON object", value_source = true)]
    pub config: Option<String>,
    #[option(
        long = "delivery-policy",
        help = "Delivery policy JSON, e.g. {\"min_interval_ms\":1000}",
        value_source = true
    )]
    pub delivery_policy: Option<String>,
    #[option(long = "secret-ref", help = "Secret reference")]
    pub secret_ref: Option<String>,
    #[option(long = "enabled", help = "Enabled flag", flag = true)]
    pub enabled: Option<bool>,
}

impl CliCommand for EventSinkCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let collection = query
            .collection
            .as_deref()
            .map(|name| services.gateway().collection_id_by_name(name))
            .transpose()?;
        let input = query.into_request()?;
        let sink = if let Some(collection) = collection {
            services
                .gateway()
                .create_collection_event_sink(collection, input)?
        } else {
            services.gateway().create_event_sink(input)?
        };
        render_json_record(tokens, &sink)
    }
}

impl EventSinkCreate {
    fn into_request(self) -> Result<NewEventSink, AppError> {
        let policy = parse_delivery_policy(self.delivery_policy)?;
        let (kind, config, delivery_policy) = if let Some(target) = self.target {
            let target = WebhookTarget::parse(&target)?;
            if self.config.is_some()
                || self.secret_ref.is_some()
                || self.kind.as_deref().is_some_and(|kind| kind != "webhook")
            {
                return Err(AppError::InvalidOption(
                    "--target cannot be combined with --config, --secret-ref, or a non-webhook --kind; use --url-secret-ref for the URL alias".to_string(),
                ));
            }
            let config = match (self.url_secret_ref, self.destination_url) {
                (Some(alias), None) => target.config(WebhookUrlSecret::new(alias)?),
                (None, Some(url)) => {
                    let url = if let Some(path) = url.strip_prefix("file://") {
                        read_to_string(path).map_err(|_| {
                            AppError::InvalidOption("Cannot read the destination URL file".into())
                        })?
                    } else {
                        url
                    };
                    target.config_with_url(&url)?
                }
                _ => {
                    return Err(AppError::InvalidOption(
                        "Choose exactly one of --url-secret-ref or --destination-url".into(),
                    ))
                }
            };
            (
                EventSinkKind::Webhook,
                Some(config),
                Some(policy.unwrap_or(EventDeliveryPolicy::new(1000)?)),
            )
        } else {
            if self.url_secret_ref.is_some() || self.destination_url.is_some() {
                return Err(AppError::InvalidOption("--url-secret-ref requires --target; custom webhook configurations can set config.url_secret_ref".to_string()));
            }
            (
                parse_event_sink_kind(required_str(self.kind.as_deref(), "kind or target")?)?,
                parse_json_object(self.config)?,
                policy,
            )
        };
        if self.collection.is_some()
            && (kind != EventSinkKind::Webhook
                || self.secret_ref.is_some()
                || config
                    .as_ref()
                    .and_then(|value| value.get("destination_url"))
                    .is_none()
                || config
                    .as_ref()
                    .and_then(|value| value.get("url_secret_ref"))
                    .is_some())
        {
            return Err(AppError::InvalidOption("Collection webhooks require their own fixed destination_url and cannot use server secret aliases".into()));
        }
        Ok(NewEventSink {
            name: self.name,
            kind,
            config,
            delivery_policy,
            enabled: self.enabled.or(Some(true)),
            secret_ref: self.secret_ref,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkUpdate {
    #[option(
        long = "collection",
        help = "Collection name for permitted destinations and owned webhook management",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
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
    #[option(
        long = "delivery-policy",
        help = "Replacement delivery policy JSON; {} clears pacing",
        value_source = true
    )]
    pub delivery_policy: Option<String>,
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
        let name = required_str(query.current_name.as_deref(), "name")?;
        let input = UpdateEventSink {
            name: query.name,
            kind: query
                .kind
                .as_deref()
                .map(parse_event_sink_kind)
                .transpose()?,
            config: parse_json_object(query.config)?,
            delivery_policy: parse_delivery_policy(query.delivery_policy)?,
            enabled: query.enabled,
            secret_ref: query.secret_ref,
        };
        let sink = if let Some(collection) = query.collection {
            services.gateway().update_collection_event_sink(
                services.gateway().collection_id_by_name(&collection)?,
                name,
                input,
            )?
        } else {
            services.gateway().update_event_sink(name, input)?
        };
        render_json_record(tokens, &sink)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct EventSinkDelete {
    #[option(
        long = "collection",
        help = "Collection name for permitted destinations and owned webhook management",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(long = "name", help = "Event sink name", autocomplete = "event_sinks")]
    pub name: Option<String>,
}

impl CliCommand for EventSinkDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = name_or_first_pos(query.name, tokens);
        let name = required_str(query.name.as_deref(), "name")?;
        if let Some(collection) = query.collection {
            services.gateway().delete_collection_event_sink(
                services.gateway().collection_id_by_name(&collection)?,
                name,
            )?;
        } else {
            services.gateway().delete_event_sink_by_name(name)?;
        }
        append_json_message("event sink deleted")
    }
}

pub(super) fn parse_json_object(input: Option<String>) -> Result<Option<Value>, AppError> {
    input
        .map(|raw| {
            let value: Value = from_str(&raw)?;
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
    from_value(Value::String(value.to_string())).map_err(AppError::from)
}

fn parse_delivery_policy(input: Option<String>) -> Result<Option<EventDeliveryPolicy>, AppError> {
    parse_json_object(input)?
        .map(from_value)
        .transpose()
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value};

    use super::{parse_delivery_policy, EventSinkCreate};
    use crate::commands::CommandArgs;
    use crate::tokenizer::CommandTokenizer;

    fn preset(target: &str) -> EventSinkCreate {
        EventSinkCreate {
            name: "ops".to_string(),
            target: Some(target.to_string()),
            url_secret_ref: Some("ops_webhook".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn collection_chat_preset_uses_a_fixed_url_without_a_server_secret() {
        let tokens = CommandTokenizer::new(
            "event sink create --collection inventory --name notifications --target slack --destination-url https://example.test/private",
            "create", &EventSinkCreate::options(),
        ).unwrap();
        let command = EventSinkCreate::parse_tokens(&tokens).unwrap();
        let input = command.into_request().unwrap();
        let config = input.config.unwrap();
        assert_eq!(config["destination_url"], "https://example.test/private");
        assert!(config.get("url_secret_ref").is_none());
    }

    #[test]
    fn collection_presets_reject_server_secret_aliases() {
        let mut command = preset("slack");
        command.collection = Some("inventory".into());
        assert!(command.into_request().is_err());
    }

    #[test]
    fn presets_serialize_as_normal_webhooks_with_provider_acknowledgements() {
        for target in ["slack", "mattermost", "discord"] {
            let request = to_value(preset(target).into_request().unwrap()).unwrap();
            assert_eq!(request["kind"], "webhook");
            assert!(request.get("target").is_none());
            assert!(request.get("secret_ref").is_none());
            assert_eq!(request["config"]["url_secret_ref"], "ops_webhook");
            assert_eq!(request["delivery_policy"], json!({"min_interval_ms": 1000}));
            let response = &request["config"]["response"];
            assert_eq!(response["success_statuses"], json!([200]));
            assert_eq!(response["rate_limit"], true);
            assert_eq!(response["retry_statuses"], json!([408, 500, 502, 503, 504]));
            if target == "discord" {
                assert!(response.get("body").is_none());
                assert!(request["config"]["body_template"]
                    .as_str()
                    .unwrap()
                    .contains("allowed_mentions"));
            } else {
                assert_eq!(
                    response["body"],
                    json!({"kind":"text_equals", "value":"ok"})
                );
            }
        }
    }

    #[test]
    fn presets_reject_ambiguous_or_incomplete_inputs() {
        let mut custom_config = preset("slack");
        custom_config.config = Some("{}".to_string());
        let mut bearer = preset("slack");
        bearer.secret_ref = Some("bearer".to_string());
        let mut kind = preset("slack");
        kind.kind = Some("email".to_string());
        let mut no_alias = preset("slack");
        no_alias.url_secret_ref = None;
        let mut no_target = preset("slack");
        no_target.target = None;
        for request in [
            custom_config,
            bearer,
            kind,
            no_alias,
            no_target,
            preset("unknown"),
        ] {
            assert!(request.into_request().is_err());
        }
    }

    #[test]
    fn ordinary_sink_requests_and_explicit_pacing_remain_available() {
        let tokens = CommandTokenizer::new(
            "event sink create --name ops --kind webhook --config '{\"custom\":true}'",
            "create",
            &EventSinkCreate::options(),
        )
        .unwrap();
        let request = EventSinkCreate::parse_tokens(&tokens)
            .unwrap()
            .into_request()
            .unwrap();
        assert_eq!(request.config, Some(json!({"custom":true})));
        assert!(request.delivery_policy.is_none());
        let tokens = CommandTokenizer::new("event sink create --name ops --target discord --url-secret-ref ops_webhook --delivery-policy '{\"min_interval_ms\":2000}'", "create", &EventSinkCreate::options()).unwrap();
        let request = EventSinkCreate::parse_tokens(&tokens)
            .unwrap()
            .into_request()
            .unwrap();
        assert_eq!(
            request.delivery_policy.unwrap().min_interval_ms(),
            Some(2000)
        );
        assert!(parse_delivery_policy(Some("{}".to_string()))
            .unwrap()
            .unwrap()
            .min_interval_ms()
            .is_none());
        for value in [
            "[]",
            "null",
            r#"{"min_interval_ms":0}"#,
            r#"{"min_interval_ms":86400001}"#,
        ] {
            assert!(parse_delivery_policy(Some(value.to_string())).is_err());
        }
    }
}
