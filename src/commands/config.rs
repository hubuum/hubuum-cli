use cli_command_derive::CommandArgs;
use serde::Serialize;
use serde_json::{json, to_string_pretty, Map, Value};

use hubuum_filter::OutputEnvelope;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::autocomplete::{config_keys, config_values};
use crate::catalog::CommandCatalogBuilder;
use crate::config::{
    config_key_names, get_config, get_config_state, is_user_preference_key,
    persist_user_preferences, reload_runtime_config, set_persisted_value, unset_persisted_value,
    ConfigEntry, ConfigSource, UserPreferences,
};
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line, set_semantic_output};
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["config"],
            catalog_command(
                "show",
                ConfigShow::default(),
                CommandDocs {
                    about: Some("Show effective configuration"),
                    long_about: Some(
                        "Show the effective configuration, including which source supplied each value.",
                    ),
                    examples: Some(
                        r#"show
show --key server.hostname"#,
                    ),
                },
            ),
        )
        .add_command(
            &["config"],
            catalog_command(
                "paths",
                ConfigPaths::default(),
                CommandDocs {
                    about: Some("Show configuration file paths"),
                    long_about: Some(
                        "Show the system, user, custom, and active writable configuration paths.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["config"],
            catalog_command(
                "set",
                ConfigSet::default(),
                CommandDocs {
                    about: Some("Persist a configuration value"),
                    long_about: Some(
                        "Save a configuration value to the active writable config file and reload it into the current CLI session.",
                    ),
                    examples: Some(
                        r#"--key server.hostname --value api.example.com
--key repl.enter_fetches_next_page --value true
--key output.object_class_computed_fields.Hosts --value S:load,P:note"#,
                    ),
                },
            ),
        )
        .add_command(
            &["config"],
            catalog_command(
                "unset",
                ConfigUnset::default(),
                CommandDocs {
                    about: Some("Remove a persisted configuration value"),
                    long_about: Some(
                        "Remove a configuration value from the active writable config file so lower-precedence sources can take effect again, then reload the current CLI session.",
                    ),
                    examples: Some("--key repl.enter_fetches_next_page"),
                },
            ),
        )
        .add_command(
            &["config"],
            catalog_command(
                "export",
                ConfigExport::default(),
                CommandDocs {
                    about: Some("Export local preferences to the server"),
                    long_about: Some(
                        "Store portable CLI preferences in the authenticated principal's settings under the 'hubuum-cli' namespace. Connection credentials and machine-specific settings are excluded.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["config"],
            catalog_command(
                "import",
                ConfigImport::default(),
                CommandDocs {
                    about: Some("Import preferences from the server"),
                    long_about: Some(
                        "Load the authenticated principal's 'hubuum-cli' settings into the active writable config file and reload the current session.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["config"],
            catalog_command(
                "store",
                ConfigStore::default(),
                CommandDocs {
                    about: Some("Configure automatic server storage"),
                    long_about: Some(
                        "Enable or disable copying portable preferences to the server after local config set and unset operations. Enabling it also exports the current preferences immediately.",
                    ),
                    examples: Some("--enabled true\n--enabled false"),
                },
            ),
        );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigShow {
    #[option(
        short = "k",
        long = "key",
        help = "Specific config key to show",
        autocomplete = "config_keys"
    )]
    pub key: Option<String>,
}

impl CliCommand for ConfigShow {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_config_show(tokens)
    }
}

pub(crate) fn render_config_show(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = ConfigShow::parse_tokens(tokens)?;
    let state = get_config_state();

    if let Some(key) = query.key {
        let entry = state.entry(&key).ok_or_else(|| {
            AppError::ParseError(format!(
                "Unknown config key: {key}. Use one of: {}",
                config_key_names().join(", ")
            ))
        })?;
        return render_single_entry(entry, desired_format(tokens));
    }

    match desired_format(tokens) {
        OutputFormat::Json => append_line(to_string_pretty(&state)?)?,
        OutputFormat::Text => render_config_entries(&state.entries)?,
    }

    Ok(())
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigPaths {}

impl CliCommand for ConfigPaths {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_config_paths(tokens)
    }
}

pub(crate) fn render_config_paths(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let _query = ConfigPaths::parse_tokens(tokens)?;
    let paths = &get_config_state().paths;
    match desired_format(tokens) {
        OutputFormat::Json => append_line(to_string_pretty(paths)?)?,
        OutputFormat::Text => {
            append_key_value("System", paths.system.display(), 12)?;
            append_key_value("User", paths.user.display(), 12)?;
            if let Some(custom) = &paths.custom {
                append_key_value("Custom", custom.display(), 12)?;
            }
            append_key_value("Write", paths.write_target.display(), 12)?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigSet {
    #[option(
        short = "k",
        long = "key",
        help = "Config key to persist",
        autocomplete = "config_keys"
    )]
    pub key: String,
    #[option(
        short = "v",
        long = "value",
        help = "Value to persist",
        autocomplete = "config_values"
    )]
    pub value: String,
}

impl CliCommand for ConfigSet {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let path = set_persisted_value(&query.key, &query.value)?;
        reload_runtime_config()?;
        services.invalidate_completion();
        if is_user_preference_key(&query.key) {
            services.sync_user_preferences_if_enabled()?;
        }
        let message = PersistMessage {
            key: query.key,
            path: path.display().to_string(),
            note: "Saved and reloaded for this CLI session.".to_string(),
        };
        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&message)?)?,
            OutputFormat::Text => {
                append_line(format!(
                    "Saved '{}' to {} and reloaded the current session.",
                    message.key, message.path
                ))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigUnset {
    #[option(
        short = "k",
        long = "key",
        help = "Config key to remove from persisted config",
        autocomplete = "config_keys"
    )]
    pub key: String,
}

impl CliCommand for ConfigUnset {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let path = unset_persisted_value(&query.key)?;
        reload_runtime_config()?;
        services.invalidate_completion();
        if is_user_preference_key(&query.key) {
            services.sync_user_preferences_if_enabled()?;
        }
        let message = PersistMessage {
            key: query.key,
            path: path.display().to_string(),
            note: "Removed and reloaded for this CLI session.".to_string(),
        };
        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&message)?)?,
            OutputFormat::Text => {
                append_line(format!(
                    "Removed '{}' from {} and reloaded the current session.",
                    message.key, message.path
                ))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigExport {}

impl CliCommand for ConfigExport {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let _query = Self::parse_tokens(tokens)?;
        let preferences = UserPreferences::from_config(&get_config());
        let stored = services.gateway().store_user_preferences(&preferences)?;
        render_preferences_result(tokens, &stored, "Exported preferences to the server.")
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigImport {}

impl CliCommand for ConfigImport {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let _query = Self::parse_tokens(tokens)?;
        let preferences = services.gateway().load_user_preferences()?;
        let path = persist_user_preferences(&preferences)?;
        reload_runtime_config()?;
        services.invalidate_completion();
        render_preferences_result(
            tokens,
            &preferences,
            &format!(
                "Imported server preferences into {} and reloaded the current session.",
                path.display()
            ),
        )
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigStore {
    #[option(
        long = "enabled",
        help = "Whether local config changes are copied to the server"
    )]
    pub enabled: bool,
}

impl CliCommand for ConfigStore {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let path = set_persisted_value("settings.store_on_server", &query.enabled.to_string())?;
        reload_runtime_config()?;
        services.invalidate_completion();
        if query.enabled {
            services.sync_user_preferences_if_enabled()?;
        }
        let message = format!(
            "Automatic server storage {} in {}.",
            if query.enabled { "enabled" } else { "disabled" },
            path.display()
        );
        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&json!({
                "enabled": query.enabled,
                "path": path,
            }))?)?,
            OutputFormat::Text => append_line(message)?,
        }
        Ok(())
    }
}

fn render_preferences_result(
    tokens: &CommandTokenizer,
    preferences: &UserPreferences,
    message: &str,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_line(to_string_pretty(preferences)?)?,
        OutputFormat::Text => append_line(message)?,
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct PersistMessage {
    key: String,
    path: String,
    note: String,
}

fn render_single_entry(entry: &ConfigEntry, format: OutputFormat) -> Result<(), AppError> {
    match format {
        OutputFormat::Json => append_line(to_string_pretty(entry)?)?,
        OutputFormat::Text => {
            let mut object = Map::new();
            object.insert("key".to_string(), Value::String(entry.key.clone()));
            object.insert("value".to_string(), Value::String(entry.value.clone()));
            object.insert("source".to_string(), Value::String(format_source(entry)));
            set_semantic_output(OutputEnvelope::detail(
                Value::Object(object),
                vec!["key".to_string(), "value".to_string(), "source".to_string()],
            ))?;
        }
    }
    Ok(())
}

fn render_config_entries(entries: &[ConfigEntry]) -> Result<(), AppError> {
    let rows = entries
        .iter()
        .map(|entry| {
            json!({
                "key": entry.key,
                "value": entry.value,
                "source": format_source_kind(entry),
                "detail": entry.source_detail.as_deref().unwrap_or(""),
            })
        })
        .collect::<Vec<_>>();
    set_semantic_output(OutputEnvelope::rows(
        rows,
        vec![
            "key".to_string(),
            "value".to_string(),
            "source".to_string(),
            "detail".to_string(),
        ],
    ))?;
    Ok(())
}

fn format_source(entry: &ConfigEntry) -> String {
    match &entry.source_detail {
        Some(detail) => format!("{} ({detail})", format_source_kind(entry)),
        None => format_source_kind(entry).to_string(),
    }
}

fn format_source_kind(entry: &ConfigEntry) -> &'static str {
    match entry.source {
        ConfigSource::Default => "default",
        ConfigSource::SystemFile => "system file",
        ConfigSource::UserFile => "user file",
        ConfigSource::CustomFile => "custom file",
        ConfigSource::Environment => "env",
        ConfigSource::CliOption => "cli",
    }
}

#[cfg(test)]
mod tests {
    use super::{format_source_kind, ConfigStore};
    use crate::commands::command_options;
    use crate::config::{ConfigEntry, ConfigSource};
    use crate::tokenizer::CommandTokenizer;

    #[test]
    fn format_source_kind_uses_short_labels() {
        assert_eq!(
            format_source_kind(&ConfigEntry {
                key: "server.hostname".to_string(),
                value: "localhost".to_string(),
                source: ConfigSource::Environment,
                source_detail: Some("HUBUUM_CLI__SERVER__HOSTNAME".to_string()),
                sensitive: false,
            }),
            "env"
        );
    }

    #[test]
    fn store_command_requires_and_parses_boolean_value() {
        let tokens = CommandTokenizer::new(
            "store --enabled true",
            "store",
            &command_options::<ConfigStore>(),
        )
        .expect("tokenization should succeed");
        let parsed = ConfigStore::parse_tokens(&tokens).expect("store options should parse");
        assert!(parsed.enabled);
    }
}
