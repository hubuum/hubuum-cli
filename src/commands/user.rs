use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use rand::distr::Alphanumeric;
use rand::{rng, RngExt};

use crate::autocomplete::{user_sort, user_where, users};
use crate::catalog::CommandCatalogBuilder;
use crate::domain::CreatedUser;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::list_query::filter_clause;
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::{AppServices, CreateUserInput, NewTokenInput, UserFilter, UserUpdateInput};
use crate::tokenizer::CommandTokenizer;

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, render_list_page, required_option_or_pos,
    CliCommand,
};

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["user"],
            catalog_command(
                "create",
                UserNew::default(),
                CommandDocs {
                    about: Some("Create a user"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user"],
            catalog_command(
                "list",
                UserList::default(),
                CommandDocs {
                    about: Some("List users"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user"],
            catalog_command(
                "delete",
                UserDelete::default(),
                CommandDocs {
                    about: Some("Delete a user"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user"],
            catalog_command(
                "show",
                UserInfo::default(),
                CommandDocs {
                    about: Some("Show user details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user"],
            catalog_command(
                "modify",
                UserModify::default(),
                CommandDocs {
                    about: Some("Modify a user"),
                    long_about: Some("Update an existing user by username."),
                    examples: Some(
                        r#"modify alice --rename alice2
modify --username alice --email alice@example.com"#,
                    ),
                },
            ),
        )
        .add_command(
            &["user"],
            catalog_command(
                "set-password",
                UserSetPassword::default(),
                CommandDocs {
                    about: Some("Set a user's password"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user", "token"],
            catalog_command(
                "list",
                UserTokenList::default(),
                CommandDocs {
                    about: Some("List tokens for a user"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user", "token"],
            catalog_command(
                "create",
                UserTokenCreate::default(),
                CommandDocs {
                    about: Some("Create a token for a user"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["user", "token"],
            catalog_command(
                "revoke",
                UserTokenRevoke::default(),
                CommandDocs {
                    about: Some("Revoke a user token"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserNew {
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: String,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
}

impl CliCommand for UserNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let password = generate_random_password(20);
        let created: CreatedUser = services.gateway().create_user(CreateUserInput {
            username: new.username,
            email: new.email,
            password: password.clone(),
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(serde_json::to_string_pretty(&created)?)?;
            }
            OutputFormat::Text => {
                created.user.format_noreturn()?;
                append_key_value("Password", password, 15)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserDelete {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
}

impl CliCommand for UserDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;
        services.gateway().delete_user(&username)?;

        let message = format!("User '{}' deleted", username);

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserInfo {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
    #[option(short = "C", long = "created-at", help = "Created at timestammp")]
    pub created_at: Option<chrono::NaiveDateTime>,
    #[option(short = "U", long = "updated-at", help = "Updated at timestamp")]
    pub updated_at: Option<chrono::NaiveDateTime>,
}

impl CliCommand for UserInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.username = Some(required_option_or_pos(
            query.username,
            tokens,
            0,
            "username",
        )?);

        let user = services.gateway().find_user(UserFilter {
            username: query.username,
            email: query.email,
            created_at: query.created_at,
            updated_at: query.updated_at,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => user.format_json_noreturn()?,
            OutputFormat::Text => user.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserList {
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: Option<String>,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
    #[option(short = "C", long = "created-at", help = "Created at timestammp")]
    pub created_at: Option<chrono::NaiveDateTime>,
    #[option(short = "U", long = "updated-at", help = "Updated at timestamp")]
    pub updated_at: Option<chrono::NaiveDateTime>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "user_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "user_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for UserList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [
                query
                    .username
                    .map(|value| contains_clause("username", value)),
                query.email.map(|value| contains_clause("email", value)),
                query.created_at.map(|value| {
                    filter_clause(
                        "created_at",
                        hubuum_client::FilterOperator::Equals { is_negated: false },
                        value.to_string(),
                    )
                }),
                query.updated_at.map(|value| {
                    filter_clause(
                        "updated_at",
                        hubuum_client::FilterOperator::Equals { is_negated: false },
                        value.to_string(),
                    )
                }),
            ]
            .into_iter()
            .flatten(),
        )?;
        let users = services.gateway().list_users(&list_query)?;
        render_list_page(tokens, &users)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserModify {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the user")]
    pub rename: Option<String>,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
}

impl CliCommand for UserModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;
        let user = services.gateway().update_user(UserUpdateInput {
            username,
            rename: query.rename,
            email: query.email,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => user.format_json_noreturn()?,
            OutputFormat::Text => user.format_noreturn()?,
        }

        Ok(())
    }
}

pub fn generate_random_password(length: usize) -> String {
    let mut rng = rng();
    std::iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(length)
        .collect()
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserSetPassword {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
    #[option(short = "p", long = "password", help = "New password")]
    pub password: String,
}

impl CliCommand for UserSetPassword {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;

        services
            .gateway()
            .set_user_password(&username, &query.password)?;

        let message = format!("Password updated for user '{}'", username);
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserTokenList {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
}

impl CliCommand for UserTokenList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;

        let token_list = services.gateway().user_tokens(&username)?;

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
pub struct UserTokenCreate {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
    #[option(short = "n", long = "name", help = "Token name")]
    pub name: Option<String>,
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

impl CliCommand for UserTokenCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;

        let raw_token = services.gateway().user_token_create(
            &username,
            NewTokenInput {
                name: query.name,
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
                append_line(format!("\nToken created for user '{}':", username))?;
                append_line(format!("  {}", raw_token))?;
                append_line("\n⚠️  This token will not be shown again. Store it securely.\n")?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserTokenRevoke {
    #[option(
        short = "u",
        long = "username",
        help = "Username of the user",
        autocomplete = "users"
    )]
    pub username: Option<String>,
    #[option(short = "t", long = "token-id", help = "Token ID to revoke")]
    pub token_id: i32,
}

impl CliCommand for UserTokenRevoke {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;

        services
            .gateway()
            .user_token_revoke(&username, query.token_id)?;

        let message = format!("Token {} revoked for user '{}'", query.token_id, username);
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}
