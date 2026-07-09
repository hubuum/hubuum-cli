use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use crate::autocomplete::{groups, service_accounts};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, CreateServiceAccountInput, NewTokenInput};
use crate::tokenizer::CommandTokenizer;

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, render_list_page, required_option_or_pos,
    CliCommand,
};

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["service-account"],
            catalog_command(
                "create",
                ServiceAccountCreate::default(),
                CommandDocs {
                    about: Some("Create a service account"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account"],
            catalog_command(
                "list",
                ServiceAccountList::default(),
                CommandDocs {
                    about: Some("List service accounts"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account"],
            catalog_command(
                "show",
                ServiceAccountShow::default(),
                CommandDocs {
                    about: Some("Show service account details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account"],
            catalog_command(
                "delete",
                ServiceAccountDelete::default(),
                CommandDocs {
                    about: Some("Delete a service account"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account"],
            catalog_command(
                "disable",
                ServiceAccountDisable::default(),
                CommandDocs {
                    about: Some("Disable a service account"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account", "token"],
            catalog_command(
                "list",
                ServiceAccountTokenList::default(),
                CommandDocs {
                    about: Some("List tokens for a service account"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account", "token"],
            catalog_command(
                "create",
                ServiceAccountTokenCreate::default(),
                CommandDocs {
                    about: Some("Create a token for a service account"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["service-account", "token"],
            catalog_command(
                "revoke",
                ServiceAccountTokenRevoke::default(),
                CommandDocs {
                    about: Some("Revoke a service account token"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountCreate {
    #[option(short = "n", long = "name", help = "Name of the service account")]
    pub name: String,
    #[option(short = "d", long = "description", help = "Description")]
    pub description: Option<String>,
    #[option(
        short = "o",
        long = "owner-group",
        help = "Owner group name",
        autocomplete = "groups"
    )]
    pub owner_group: String,
}

impl CliCommand for ServiceAccountCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;

        let sa = services
            .gateway()
            .create_service_account(CreateServiceAccountInput {
                name: query.name,
                description: query.description,
                owner_group_id: services.gateway().group_id_by_name(&query.owner_group)?,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => sa.format_json_noreturn()?,
            OutputFormat::Text => sa.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountList {
    #[option(short = "n", long = "name", help = "Name filter")]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description filter")]
    pub description: Option<String>,
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for ServiceAccountList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [
                query.name.map(|value| contains_clause("name", value)),
                query
                    .description
                    .map(|value| contains_clause("description", value)),
            ]
            .into_iter()
            .flatten(),
        )?;

        let service_accounts = services.gateway().list_service_accounts(&list_query)?;
        render_list_page(tokens, &service_accounts)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountShow {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
}

impl CliCommand for ServiceAccountShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        let sa = services.gateway().service_account(&name)?;

        match desired_format(tokens) {
            OutputFormat::Json => sa.format_json_noreturn()?,
            OutputFormat::Text => sa.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
}

impl CliCommand for ServiceAccountDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        services.gateway().delete_service_account(&name)?;

        let message = format!("Service account '{}' deleted", name);
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountDisable {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
}

impl CliCommand for ServiceAccountDisable {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        let sa = services.gateway().disable_service_account(&name)?;

        match desired_format(tokens) {
            OutputFormat::Json => sa.format_json_noreturn()?,
            OutputFormat::Text => {
                append_line(format!("Service account '{}' disabled", name))?;
                sa.format_noreturn()?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountTokenList {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
}

impl CliCommand for ServiceAccountTokenList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        let token_list = services.gateway().service_account_tokens(&name)?;

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(serde_json::to_string_pretty(&token_list)?)?;
            }
            OutputFormat::Text => {
                token_list.format_noreturn()?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountTokenCreate {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
    #[option(long = "token-name", help = "Token name")]
    pub token_name: Option<String>,
    #[option(short = "d", long = "description", help = "Token description")]
    pub description: Option<String>,
    #[option(
        short = "s",
        long = "scope",
        help = "Permission scope (repeatable)",
        nargs = 1
    )]
    pub scopes: Vec<String>,
    #[option(
        long = "expires-at",
        help = "Token expiration, RFC3339 (e.g. 2026-12-31T23:59:59Z)"
    )]
    pub expires_at: Option<String>,
}

impl CliCommand for ServiceAccountTokenCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        let raw_token = services.gateway().service_account_token_create(
            &name,
            NewTokenInput {
                name: query.token_name,
                description: query.description,
                expires_at: query.expires_at,
                scopes: query.scopes,
            },
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(serde_json::to_string_pretty(&serde_json::json!({
                    "token": raw_token,
                    "warning": "This token will not be shown again. Store it securely."
                }))?)?;
            }
            OutputFormat::Text => {
                append_line(format!("\nToken created for service account '{}':", name))?;
                append_line(format!("  {}", raw_token))?;
                append_line("\n⚠️  This token will not be shown again. Store it securely.\n")?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountTokenRevoke {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
    #[option(short = "t", long = "token-id", help = "Token ID to revoke")]
    pub token_id: i32,
}

impl CliCommand for ServiceAccountTokenRevoke {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        services
            .gateway()
            .service_account_token_revoke(&name, query.token_id)?;

        let message = format!(
            "Token {} revoked for service account '{}'",
            query.token_id, name
        );
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}
