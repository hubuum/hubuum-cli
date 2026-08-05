use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use serde::Serialize;
use serde_json::{json, to_string_pretty};

use super::builder::{catalog_command, CommandDocs};
use super::{build_command_catalog, desired_format, CliCommand};
use crate::autocomplete::{command_aliases, file_paths};
use crate::catalog::CommandCatalogBuilder;
use crate::config::{
    get_config, is_user_preference_key, persist_command_alias, reload_runtime_config,
    unset_persisted_value,
};
use crate::domain::{
    alias_name_shadows_command, CommandAliasDescription, CommandAliasName, CommandAliasTarget,
};
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::{append_line, set_semantic_output};
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["alias"],
            catalog_command(
                "list",
                AliasList::default(),
                CommandDocs {
                    about: Some("List personal command aliases"),
                    long_about: Some("List personal aliases and their short descriptions."),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["alias"],
            catalog_command(
                "show",
                AliasShow::default(),
                CommandDocs {
                    about: Some("Show one personal command alias"),
                    examples: Some("--name outdated-kernels"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["alias"],
            catalog_command(
                "set",
                AliasSet::default(),
                CommandDocs {
                    about: Some("Create or replace a personal command alias"),
                    long_about: Some(
                        "Bind a root-level alias to one complete CLI command, including pipe stages and redirects. The command may be inline or loaded from a local file with file://FILE.",
                    ),
                    examples: Some(
                        r#"--name hosts --command 'object list --class Hosts | P Name'
--name outdated-kernels --description 'Show hosts with kernels older than the newest observed for their OS release' --command file://examples/aliases/outdated-kernels.hubuum"#,
                    ),
                },
            ),
        )
        .add_command(
            &["alias"],
            catalog_command(
                "unset",
                AliasUnset::default(),
                CommandDocs {
                    about: Some("Remove a personal command alias"),
                    examples: Some("--name outdated-kernels"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AliasList {}

impl CliCommand for AliasList {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let _query = Self::parse_tokens(tokens)?;
        let aliases = get_config();
        let rows = aliases
            .aliases
            .iter()
            .map(|(name, _)| {
                json!({
                    "Name": name,
                    "Description": aliases.aliases.description(name).unwrap_or(""),
                })
            })
            .collect();
        set_semantic_output(OutputEnvelope::rows(
            rows,
            vec!["Name".to_string(), "Description".to_string()],
        ))?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AliasShow {
    #[option(
        short = "n",
        long = "name",
        help = "Alias name",
        autocomplete = "command_aliases"
    )]
    pub name: String,
}

impl CliCommand for AliasShow {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = CommandAliasName::new(query.name)?;
        let config = get_config();
        let command = config.aliases.get(name.as_str()).ok_or_else(|| {
            AppError::EntityNotFound(format!("command alias '{}'", name.as_str()))
        })?;
        let description = config.aliases.description(name.as_str());
        match desired_format(tokens) {
            OutputFormat::Json => append_line(to_string_pretty(&json!({
                "name": name.as_str(),
                "description": description,
                "command": command,
            }))?)?,
            OutputFormat::Text => set_semantic_output(OutputEnvelope::detail(
                json!({
                    "Name": name.as_str(),
                    "Description": description,
                    "Command": command,
                }),
                vec![
                    "Name".to_string(),
                    "Description".to_string(),
                    "Command".to_string(),
                ],
            ))?,
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AliasSet {
    #[option(short = "n", long = "name", help = "Alias name")]
    pub name: String,
    #[option(
        short = "c",
        long = "command",
        help = "Complete command line; use file://FILE to load it from a file",
        value_source = true,
        autocomplete = "file_paths"
    )]
    pub command: String,
    #[option(
        short = "d",
        long = "description",
        help = "Short one-line description shown in alias summaries"
    )]
    pub description: Option<String>,
}

impl CliCommand for AliasSet {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = CommandAliasName::new(query.name)?;
        let command = CommandAliasTarget::new(query.command)?;
        let description = query
            .description
            .map(CommandAliasDescription::new)
            .transpose()?;
        if alias_name_shadows_command(&build_command_catalog(), name.as_str()) {
            return Err(AppError::InvalidOption(format!(
                "alias name '{}' conflicts with a built-in command or scope",
                name.as_str()
            )));
        }

        let key = format!("aliases.{}", name.as_str());
        let path = persist_command_alias(&name, &command, description.as_ref())?;
        reload_runtime_config()?;
        if is_user_preference_key(&key) {
            services.sync_user_preferences_if_enabled()?;
        }
        render_persisted_alias(
            tokens,
            "Saved",
            name.as_str(),
            Some(command.as_str()),
            description.as_ref().map(CommandAliasDescription::as_str),
            &path.display().to_string(),
        )
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AliasUnset {
    #[option(
        short = "n",
        long = "name",
        help = "Alias name",
        autocomplete = "command_aliases"
    )]
    pub name: String,
}

impl CliCommand for AliasUnset {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = CommandAliasName::new(query.name)?;
        if get_config().aliases.get(name.as_str()).is_none() {
            return Err(AppError::EntityNotFound(format!(
                "command alias '{}'",
                name.as_str()
            )));
        }

        let key = format!("aliases.{}", name.as_str());
        let path = unset_persisted_value(&key)?;
        reload_runtime_config()?;
        if is_user_preference_key(&key) {
            services.sync_user_preferences_if_enabled()?;
        }
        render_persisted_alias(
            tokens,
            "Removed",
            name.as_str(),
            None,
            None,
            &path.display().to_string(),
        )
    }
}

fn render_persisted_alias(
    tokens: &CommandTokenizer,
    action: &str,
    name: &str,
    command: Option<&str>,
    description: Option<&str>,
    path: &str,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_line(to_string_pretty(&json!({
            "action": action.to_lowercase(),
            "name": name,
            "description": description,
            "command": command,
            "path": path,
        }))?)?,
        OutputFormat::Text => append_line(format!(
            "{action} command alias '{name}' in {path} and reloaded the current session."
        ))?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::write;

    use tempfile::tempdir;

    use super::AliasSet;
    use crate::commands::command_options;
    use crate::tokenizer::CommandTokenizer;

    #[test]
    fn alias_command_can_be_loaded_from_a_file_value_source() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("command.hubuum");
        write(&path, "object list --class Hosts | P Name\n").expect("command file");
        let line = format!(
            "alias set --name hosts --description 'List known hosts' --command file://{}",
            path.display()
        );
        let tokens = CommandTokenizer::new(&line, "set", &command_options::<AliasSet>())
            .expect("alias command should tokenize");
        let parsed = AliasSet::parse_tokens(&tokens).expect("alias options should parse");

        assert_eq!(parsed.command, "object list --class Hosts | P Name");
        assert_eq!(parsed.description.as_deref(), Some("List known hosts"));
    }
}
