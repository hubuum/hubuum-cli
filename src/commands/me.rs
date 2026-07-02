use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    // Register commands under the "me" prefix
    builder
        .add_command(
            &["me"],
            catalog_command(
                "show",
                MeShow::default(),
                CommandDocs {
                    about: Some("Show current identity"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["me"],
            catalog_command(
                "groups",
                MeGroups::default(),
                CommandDocs {
                    about: Some("List groups the current principal belongs to"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["me"],
            catalog_command(
                "tokens",
                MeTokens::default(),
                CommandDocs {
                    about: Some("List all tokens for the current principal"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["me"],
            catalog_command(
                "permissions",
                MePermissions::default(),
                CommandDocs {
                    about: Some("Show effective permissions for the current principal"),
                    ..CommandDocs::default()
                },
            ),
        );

    // Also register "whoami" as a top-level alias for "me show"
    builder.add_command(
        &[],
        catalog_command(
            "whoami",
            MeShow::default(),
            CommandDocs {
                about: Some("Show current identity (alias for 'me show')"),
                ..CommandDocs::default()
            },
        ),
    );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct MeShow {}

impl CliCommand for MeShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let me = services.gateway().me()?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&me)?)?,
            OutputFormat::Text => me.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct MeGroups {}

impl CliCommand for MeGroups {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let groups = services.gateway().me_groups()?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&groups)?)?,
            OutputFormat::Text => groups.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct MeTokens {}

impl CliCommand for MeTokens {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let token_list = services.gateway().me_tokens()?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&token_list)?)?,
            OutputFormat::Text => token_list.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct MePermissions {}

impl CliCommand for MePermissions {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let permissions = services.gateway().me_permissions()?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&permissions)?)?,
            OutputFormat::Text => permissions.format_noreturn()?,
        }

        Ok(())
    }
}
