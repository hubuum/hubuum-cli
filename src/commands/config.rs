use cli_command_derive::CommandArgs;
use serde::Serialize;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::autocomplete::config_keys;
use crate::catalog::CommandCatalogBuilder;
use crate::config::{
    config_key_names, get_config_state, reload_runtime_config, set_persisted_value,
    unset_persisted_value, ConfigEntry,
};
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
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
--key repl.enter_fetches_next_page --value true"#,
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
        let query = Self::parse_tokens(tokens)?;
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
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&state)?)?,
            OutputFormat::Text => render_config_entries(&state.entries)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ConfigPaths {}

impl CliCommand for ConfigPaths {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let _query = Self::parse_tokens(tokens)?;
        let paths = &get_config_state().paths;
        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(paths)?)?,
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
    #[option(short = "v", long = "value", help = "Value to persist")]
    pub value: String,
}

impl CliCommand for ConfigSet {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let path = set_persisted_value(&query.key, &query.value)?;
        reload_runtime_config()?;
        _services.invalidate_completion();
        let message = PersistMessage {
            key: query.key,
            path: path.display().to_string(),
            note: "Saved and reloaded for this CLI session.".to_string(),
        };
        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&message)?)?,
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
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let path = unset_persisted_value(&query.key)?;
        reload_runtime_config()?;
        _services.invalidate_completion();
        let message = PersistMessage {
            key: query.key,
            path: path.display().to_string(),
            note: "Removed and reloaded for this CLI session.".to_string(),
        };
        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&message)?)?,
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

#[derive(Debug, Serialize)]
struct PersistMessage {
    key: String,
    path: String,
    note: String,
}

fn render_single_entry(entry: &ConfigEntry, format: OutputFormat) -> Result<(), AppError> {
    match format {
        OutputFormat::Json => append_line(serde_json::to_string_pretty(entry)?)?,
        OutputFormat::Text => {
            append_key_value("Key", &entry.key, 12)?;
            append_key_value("Value", &entry.value, 12)?;
            append_key_value("Source", format_source(entry), 12)?;
        }
    }
    Ok(())
}

fn render_config_entries(entries: &[ConfigEntry]) -> Result<(), AppError> {
    append_line(format!(
        "{:<34} {:<20} {:<14} {}",
        "Key", "Value", "Source", "Detail"
    ))?;
    for entry in entries {
        append_line(format!(
            "{:<34} {:<20} {:<14} {}",
            entry.key,
            entry.value,
            format_source_kind(entry),
            entry.source_detail.as_deref().unwrap_or("")
        ))?;
    }
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
        crate::config::ConfigSource::Default => "default",
        crate::config::ConfigSource::SystemFile => "system file",
        crate::config::ConfigSource::UserFile => "user file",
        crate::config::ConfigSource::CustomFile => "custom file",
        crate::config::ConfigSource::Environment => "env",
        crate::config::ConfigSource::CliOption => "cli",
    }
}

#[cfg(test)]
mod tests {
    use super::format_source_kind;
    use crate::config::{ConfigEntry, ConfigSource};

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
}
