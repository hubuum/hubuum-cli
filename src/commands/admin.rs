use cli_command_derive::CommandArgs;
use hubuum_client::ResourceRevision;
use hubuum_filter::OutputEnvelope;
use serde::Serialize;
use serde_json::{json, Value};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::models::OutputFormat;
use crate::output::set_semantic_output;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &["admin", "settings"],
        catalog_command(
            "describe",
            RuntimeSettingsDescribe::default(),
            CommandDocs {
                about: Some(
                    "Show server-provided help, types, limits and choice explanations for a group",
                ),
                ..Default::default()
            },
        ),
    );
    builder.add_command(
        &["admin", "settings"],
        catalog_command(
            "list",
            RuntimeSettingsList::default(),
            CommandDocs {
                about: Some("Discover supported runtime settings, types, bounds and workflows"),
                ..Default::default()
            },
        ),
    );
    builder.add_command(
        &["admin", "settings"],
        catalog_command(
            "show",
            RuntimeSettingsShow::default(),
            CommandDocs {
                about: Some("Read one runtime settings group and its revision"),
                ..Default::default()
            },
        ),
    );
    builder.add_command(
        &["admin", "settings"],
        catalog_command(
            "set",
            RuntimeSettingsSet::default(),
            CommandDocs {
                about: Some("Patch edited fields atomically using an expected revision"),
                ..Default::default()
            },
        ),
    );
    builder.add_command(
        &["admin", "settings"],
        catalog_command(
            "reset",
            RuntimeSettingsReset::default(),
            CommandDocs {
                about: Some("Return a group to deployment ownership using an expected revision"),
                ..Default::default()
            },
        ),
    );

    builder
        .add_command(
            &["admin", "query-observations"],
            catalog_command(
                "show",
                QueryObservationsShow::default(),
                observation_docs("Show effective query-observation settings"),
            ),
        )
        .add_command(
            &["admin", "query-observations"],
            catalog_command(
                "enable",
                QueryObservationsEnable::default(),
                observation_docs("Enable shared query-observation collection"),
            ),
        )
        .add_command(
            &["admin", "query-observations"],
            catalog_command(
                "disable",
                QueryObservationsDisable::default(),
                observation_docs("Disable shared query-observation collection"),
            ),
        );
    builder.add_command(
        &["admin"],
        catalog_command(
            "config",
            AdminConfig::default(),
            CommandDocs {
                about: Some("Show the server's effective configuration"),
                long_about: Some(
                    "Show the authenticated server's effective process configuration. Secrets are redacted by the server. Administrator access is required.",
                ),
                examples: Some("--output json"),
            },
        ),
    );
}

fn observation_docs(about: &'static str) -> CommandDocs {
    CommandDocs { about: Some(about), long_about: Some("Requires an unscoped human administrator. Requires server runtime settings discovery. Show returns group state and enable/disable patches only enabled using the current revision; conflicts are returned without retry. Other replicas apply changes asynchronously; existing observations remain retained."), ..Default::default() }
}

#[derive(Clone, Default, CommandArgs)]
struct QueryObservationsShow {}

impl CliCommand for QueryObservationsShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        Self::parse_tokens(tokens)?;
        render_structured_value(
            services.gateway().query_observations()?,
            desired_format(tokens)?,
        )
    }
}

macro_rules! observation_update {
    ($name:ident, $enabled:expr) => {
        #[derive(Clone, Default, CommandArgs)]
        struct $name {}
        impl CliCommand for $name {
            fn execute(
                &self,
                services: &AppServices,
                tokens: &CommandTokenizer,
            ) -> Result<(), AppError> {
                Self::parse_tokens(tokens)?;
                render_structured_value(
                    services.gateway().set_query_observations($enabled)?,
                    desired_format(tokens)?,
                )
            }
        }
    };
}
observation_update!(QueryObservationsEnable, true);
observation_update!(QueryObservationsDisable, false);

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AdminConfig {}

impl CliCommand for AdminConfig {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let _query = Self::parse_tokens(tokens)?;
        let config = services.gateway().server_config()?;
        render_server_config(config, desired_format(tokens)?)
    }
}

fn render_server_config(config: Value, format: OutputFormat) -> Result<(), AppError> {
    render_structured_value(config, format)
}

pub(super) fn render_structured_value(config: Value, format: OutputFormat) -> Result<(), AppError> {
    match format {
        OutputFormat::Json => set_semantic_output(OutputEnvelope::detail(config, Vec::new())),
        OutputFormat::Text => {
            let mut rows = Vec::new();
            flatten_config(None, &config, &mut rows);
            set_semantic_output(OutputEnvelope::rows(
                rows,
                vec!["key".to_string(), "value".to_string()],
            ))
        }
    }
}

fn flatten_config(prefix: Option<&str>, value: &Value, rows: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let path = match prefix {
                    Some(prefix) => format!("{prefix}.{key}"),
                    None => key.clone(),
                };
                flatten_config(Some(&path), value, rows);
            }
        }
        Value::Array(values) => {
            rows.push(json!({
                "key": prefix.unwrap_or_default(),
                "value": Value::Array(values.clone()).to_string(),
            }));
        }
        Value::String(value) => rows.push(json!({
            "key": prefix.unwrap_or_default(),
            "value": value,
        })),
        Value::Null => rows.push(json!({
            "key": prefix.unwrap_or_default(),
            "value": "",
        })),
        value => rows.push(json!({
            "key": prefix.unwrap_or_default(),
            "value": value.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::flatten_config;

    #[test]
    fn server_config_is_flattened_to_stable_dotted_keys() {
        let mut rows = Vec::new();
        flatten_config(
            None,
            &json!({
                "server": {"bind_port": 8080, "metrics_enabled": true},
                "authentication": {"admin_identity_scope": null}
            }),
            &mut rows,
        );

        assert_eq!(
            rows,
            vec![
                json!({"key": "authentication.admin_identity_scope", "value": ""}),
                json!({"key": "server.bind_port", "value": "8080"}),
                json!({"key": "server.metrics_enabled", "value": "true"}),
            ]
        );
    }
}

#[derive(Clone, Default, CommandArgs)]
struct RuntimeSettingsList {}

impl CliCommand for RuntimeSettingsList {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        Self::parse_tokens(tokens)?;
        render_structured_value(
            services.gateway().runtime_settings_schema()?,
            desired_format(tokens)?,
        )
    }
}

#[derive(Clone, Default, CommandArgs)]
struct RuntimeSettingsDescribe {
    #[option(long = "group", help = "Group key from admin settings list")]
    group: String,
}

impl CliCommand for RuntimeSettingsDescribe {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let catalogue = services.gateway().runtime_settings_schema()?;
        let descriptor = catalogue["groups"]
            .as_array()
            .and_then(|groups| groups.iter().find(|group| group["key"] == query.group))
            .ok_or_else(|| {
                AppError::InvalidOption("The server does not advertise this settings group".into())
            })?;
        render_structured_value(descriptor.clone(), desired_format(tokens)?)
    }
}

#[derive(Clone, Default, CommandArgs)]
struct RuntimeSettingsShow {
    #[option(long = "group", help = "Group key from admin settings list")]
    group: String,
}

impl CliCommand for RuntimeSettingsShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        render_structured_value(
            services.gateway().runtime_settings_group(&query.group)?,
            desired_format(tokens)?,
        )
    }
}

#[derive(Clone, Default, CommandArgs)]
struct RuntimeSettingsSet {
    #[option(long = "group", help = "Group key from admin settings list")]
    group: String,
    #[option(
        long = "expected-revision",
        help = "Revision returned by admin settings show"
    )]
    expected_revision: Option<i64>,
    #[option(long = "changes", help = "JSON object containing only edited fields")]
    changes: String,
}

impl CliCommand for RuntimeSettingsSet {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let revision = settings_revision(query.expected_revision)?;
        let changes = serde_json::from_str(&query.changes)?;
        render_structured_value(
            services
                .gateway()
                .patch_runtime_settings(&query.group, revision, changes)?,
            desired_format(tokens)?,
        )
    }
}

#[derive(Clone, Default, CommandArgs)]
struct RuntimeSettingsReset {
    #[option(long = "group", help = "Group to return to deployment ownership")]
    group: String,
    #[option(
        long = "expected-revision",
        help = "Revision returned by admin settings show"
    )]
    expected_revision: Option<i64>,
}

impl CliCommand for RuntimeSettingsReset {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let revision = settings_revision(query.expected_revision)?;
        render_structured_value(
            services
                .gateway()
                .release_runtime_settings(&query.group, revision)?,
            desired_format(tokens)?,
        )
    }
}

fn settings_revision(value: Option<i64>) -> Result<ResourceRevision, AppError> {
    Ok(ResourceRevision::new(value.ok_or_else(|| {
        AppError::MissingOptions(vec!["expected-revision".into()])
    })?)?)
}
