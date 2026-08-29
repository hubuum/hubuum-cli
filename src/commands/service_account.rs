use cli_command_derive::CommandArgs;
use hubuum_client::TokenId;
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

use crate::autocomplete::{groups, service_account_token_ids, service_accounts};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{
    AppServices, CloneTokenInput, CreateServiceAccountInput, NewTokenInput, RenewTokenInput,
    TokenStateFilter,
};
use crate::tokenizer::CommandTokenizer;

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, render_list_page, required_option_or_pos,
    CliCommand, PageSelection,
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
                "show",
                ServiceAccountTokenShow::default(),
                CommandDocs {
                    about: Some("Show a service-account token and its resolved scope"),
                    long_about: Some(
                        "Shows all token metadata returned by Hubuum, including permission and resource boundaries. Collection, class, and object IDs are resolved to names when possible. Object IDs are resolved only through classes explicitly present in the token scope and are marked unreachable otherwise.",
                    ),
                    examples: Some(
                        "automation 42\n--name automation --token-id 42\nautomation 42 --output json",
                    ),
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
                "clone",
                ServiceAccountTokenClone::default(),
                CommandDocs {
                    about: Some("Clone an active service-account token's scope"),
                    long_about: Some(
                        "Creates a replacement token for the same service account with the source token's exact permission and resource boundaries. The source token name and description are copied unless overridden. The replacement receives the server's default lifetime unless --expires-at is supplied. With --revoke, the source is revoked only after the replacement is created.",
                    ),
                    examples: Some(
                        "automation 42\n--name automation --token-id 42 --token-name replacement\n--name automation --token-id 42 --revoke",
                    ),
                },
            ),
        )
        .add_command(
            &["service-account", "token"],
            catalog_command(
                "renew",
                ServiceAccountTokenRenew::default(),
                CommandDocs {
                    about: Some("Renew a service-account token into a replacement"),
                    long_about: Some(
                        "Mints a replacement with the source token's metadata and exact scope. The source token is not modified or reactivated. The replacement receives the server's default lifetime unless --expires-at is supplied.",
                    ),
                    examples: Some(
                        "automation 42\n--name automation --token-id 42 --expires-at 2026-12-31T23:59:59Z",
                    ),
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
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
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

impl CliCommand for ServiceAccountList {
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
            [
                query.name.map(|value| contains_clause("name", value)),
                query
                    .description
                    .map(|value| contains_clause("description", value)),
            ]
            .into_iter()
            .flatten(),
        )?
        .page_selection(PageSelection::from_all(query.all.unwrap_or(false)));

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
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

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
    #[option(
        long = "state",
        help = "Lifecycle state: active, expired, revoked, or all"
    )]
    pub state: Option<TokenStateFilter>,
}

impl CliCommand for ServiceAccountTokenList {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        let token_list = services
            .gateway()
            .service_account_tokens(&name, query.state)?;

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(to_string_pretty(&token_list)?)?;
            }
            OutputFormat::Text => {
                token_list.format_noreturn()?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountTokenShow {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
    #[option(
        short = "t",
        long = "token-id",
        help = "Token ID to show",
        autocomplete = "service_account_token_ids"
    )]
    pub token_id: Option<TokenId>,
}

impl CliCommand for ServiceAccountTokenShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name_is_option = query.name.is_some();
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        let token_id = required_option_or_pos(
            query.token_id,
            tokens,
            usize::from(!name_is_option),
            "token-id",
        )?;
        let token = services.gateway().service_account_token(&name, token_id)?;

        match desired_format(tokens) {
            OutputFormat::Json => token.format_json_noreturn()?,
            OutputFormat::Text => token.format_noreturn()?,
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

        let issued_token = services.gateway().service_account_token_create(
            &name,
            NewTokenInput {
                name: query.token_name,
                description: query.description,
                expires_at: query.expires_at,
                scopes: query.scopes,
            },
        )?;

        super::render_issued_token(tokens, &format!("service account '{name}'"), &issued_token)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountTokenClone {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
    #[option(
        short = "t",
        long = "token-id",
        help = "Active source token ID",
        autocomplete = "service_account_token_ids"
    )]
    pub token_id: Option<TokenId>,
    #[option(long = "token-name", help = "Replacement token name")]
    pub token_name: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Replacement token description"
    )]
    pub description: Option<String>,
    #[option(
        long = "expires-at",
        help = "Replacement expiration, RFC3339; defaults to the server lifetime"
    )]
    pub expires_at: Option<String>,
    #[option(
        long = "revoke",
        help = "Revoke the source after creating the replacement",
        flag = "true"
    )]
    pub revoke: Option<bool>,
}

impl CliCommand for ServiceAccountTokenClone {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name_is_option = query.name.is_some();
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        let token_id = required_option_or_pos(
            query.token_id,
            tokens,
            usize::from(!name_is_option),
            "token-id",
        )?;
        let input = CloneTokenInput::new(token_id)
            .name(query.token_name)
            .description(query.description)
            .expires_at(query.expires_at)
            .revoke_source(query.revoke.unwrap_or(false));
        let outcome = services
            .gateway()
            .service_account_token_clone(&name, input)?;

        super::render_cloned_token(tokens, &format!("service account '{name}'"), &outcome)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ServiceAccountTokenRenew {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the service account",
        autocomplete = "service_accounts"
    )]
    pub name: Option<String>,
    #[option(
        short = "t",
        long = "token-id",
        help = "Source token ID",
        autocomplete = "service_account_token_ids"
    )]
    pub token_id: Option<TokenId>,
    #[option(
        long = "expires-at",
        help = "Replacement expiration, RFC3339; defaults to the server lifetime"
    )]
    pub expires_at: Option<String>,
}

impl CliCommand for ServiceAccountTokenRenew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name_is_option = query.name.is_some();
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        let token_id = required_option_or_pos(
            query.token_id,
            tokens,
            usize::from(!name_is_option),
            "token-id",
        )?;
        let issued_token = services.gateway().service_account_token_renew(
            &name,
            token_id,
            RenewTokenInput::new(query.expires_at)?,
        )?;

        super::render_issued_token(tokens, &format!("service account '{name}'"), &issued_token)
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
    #[option(
        short = "t",
        long = "token-id",
        help = "Token ID to revoke",
        autocomplete = "service_account_token_ids"
    )]
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
