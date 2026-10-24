use cli_command_derive::CommandArgs;
use hubuum_client::{EventNotificationRequest, EventSubscriptionId};
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{render_json_record, required_option, CliCommand};
use crate::autocomplete::event_sinks;
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::domain::EventUuid;
use crate::errors::{AppError, ReauthenticationRetry};
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &["event", "sink"],
        catalog_command("preview", EventNotificationPreview::default(), CommandDocs {
            about: Some("Render a notification without sending it (administrator)"),
            long_about: Some("Render a source event with a collection or system subscription. Use the event_id UUID from audit output and the subscription's numeric ID. Requires administrator access and bypasses enabled flags and subscription filters. Rendered payloads may contain sensitive event data."),
            examples: Some("--name ops-slack --subscription-id 7 --event 11111111-1111-4111-8111-111111111111"),
        }),
    ).add_command(
        &["event", "sink"],
        catalog_command("test", EventNotificationTest::default(), CommandDocs {
            about: Some("Queue a real test notification (administrator)"),
            long_about: Some("Queues a real delivery to the sink's destination, bypassing enabled flags and subscription filters. Requires administrator access. Returns the delivery record; use event delivery show to inspect its outcome. Ambiguous failures are not replayed automatically."),
            examples: Some("--name ops-slack --subscription-id 7 --event 11111111-1111-4111-8111-111111111111"),
        }),
    );
}

macro_rules! notification_args {
    ($name:ident) => {
        #[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
        pub struct $name {
            #[option(long = "name", help = "Sink name", autocomplete = "event_sinks")]
            pub name: String,
            #[option(
                long = "subscription-id",
                help = "Positive collection or system subscription ID"
            )]
            pub subscription_id: Option<EventSubscriptionId>,
            #[option(
                long = "event",
                help = "Source event_id UUID from audit output (not the numeric audit ID)"
            )]
            pub event: Option<EventUuid>,
        }
    };
}

notification_args!(EventNotificationPreview);
notification_args!(EventNotificationTest);

fn request(
    subscription_id: Option<EventSubscriptionId>,
    event: Option<EventUuid>,
) -> Result<EventNotificationRequest, AppError> {
    let subscription_id = required_option(subscription_id, "subscription-id")?;
    if subscription_id.get() <= 0 {
        return Err(AppError::InvalidOption(
            "--subscription-id must be positive".into(),
        ));
    }
    Ok(EventNotificationRequest {
        subscription_id,
        event_id: required_option(event, "event")?.to_string(),
    })
}

impl CliCommand for EventNotificationPreview {
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let request = request(query.subscription_id, query.event)?;
        let gateway = services.gateway();
        let sink = gateway.event_sink_id_by_name(&query.name)?;
        render_json_record(tokens, &gateway.preview_event_notification(sink, request)?)
    }
}

impl CliCommand for EventNotificationTest {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let request = request(query.subscription_id, query.event)?;
        let gateway = services.gateway();
        let sink = gateway.event_sink_id_by_name(&query.name)?;
        render_json_record(tokens, &gateway.test_event_notification(sink, request)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_validation_rejects_numeric_audit_ids_and_malformed_sources() {
        for invalid in [
            "7",
            "",
            "11111111111141118111111111111111",
            "11111111-1111-4111-8111-11111111111z",
            "11111111_1111-4111-8111-111111111111",
        ] {
            assert!(invalid.parse::<EventUuid>().is_err());
            assert!(serde_json::from_value::<EventUuid>(serde_json::json!(invalid)).is_err());
        }
        assert_eq!(EventNotificationPreview::EFFECTS, CommandEffects::ReadOnly);
        assert_eq!(EventNotificationTest::EFFECTS, CommandEffects::Mutating);
    }

    #[test]
    fn notification_requires_a_positive_subscription_and_uuid() {
        let uuid = "11111111-1111-4111-8111-111111111111"
            .parse::<EventUuid>()
            .unwrap();
        assert!(request(Some(7.try_into().unwrap()), Some(uuid.clone())).is_ok());
        assert!("0".parse::<EventSubscriptionId>().is_err());
        assert!(request(None, Some(uuid)).is_err());
        assert!(request(Some(7.try_into().unwrap()), None).is_err());
    }
}
