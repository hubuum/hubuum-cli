use chrono::NaiveDateTime;
use cli_command_derive::CommandArgs;
use hubuum_client::FilterOperator;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_string_pretty};
use std::fs::read_to_string;
use std::iter::repeat;
use std::path::Path;

use rand::distr::Alphanumeric;
use rand::{rng, RngExt};
use rpassword::prompt_password;

use crate::autocomplete::{file_paths, user_sort, user_where, users};
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
                    long_about: Some(
                        "Prompt and confirm a new password, or read one from a file for automation.",
                    ),
                    examples: Some(
                        r#"set-password alice
set-password alice --password-file /run/secrets/alice-password"#,
                    ),
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
                append_line(to_string_pretty(&created)?)?;
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
    pub created_at: Option<NaiveDateTime>,
    #[option(short = "U", long = "updated-at", help = "Updated at timestamp")]
    pub updated_at: Option<NaiveDateTime>,
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
    pub created_at: Option<NaiveDateTime>,
    #[option(short = "U", long = "updated-at", help = "Updated at timestamp")]
    pub updated_at: Option<NaiveDateTime>,
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
}

impl CliCommand for UserList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [
                query
                    .username
                    .map(|value| contains_clause("username", value)),
                query.email.map(|value| contains_clause("email", value)),
                query.created_at.map(|value| {
                    filter_clause(
                        "created_at",
                        FilterOperator::Equals { is_negated: false },
                        value.to_string(),
                    )
                }),
                query.updated_at.map(|value| {
                    filter_clause(
                        "updated_at",
                        FilterOperator::Equals { is_negated: false },
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
    repeat(())
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
    #[option(
        long = "password-file",
        help = "Read the new password from a file instead of prompting",
        autocomplete = "file_paths"
    )]
    pub password_file: Option<String>,
}

impl CliCommand for UserSetPassword {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let username = required_option_or_pos(query.username, tokens, 0, "username")?;
        let password = match query.password_file {
            Some(path) => NewPassword::from_file(&path)?,
            None => NewPassword::prompt(&username)?,
        };

        services
            .gateway()
            .set_user_password(&username, password.as_str())?;

        let message = format!("Password updated for user '{}'", username);
        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

struct NewPassword(String);

impl NewPassword {
    fn new(password: String) -> Result<Self, AppError> {
        if password.is_empty() {
            return Err(AppError::InvalidOption(
                "New password cannot be empty".to_string(),
            ));
        }
        Ok(Self(password))
    }

    fn prompt(username: &str) -> Result<Self, AppError> {
        let password = prompt_password(format!("New password for {username}: "))?;
        let confirmation = prompt_password("Confirm new password: ")?;
        if password != confirmation {
            return Err(AppError::InvalidOption(
                "Password confirmation does not match".to_string(),
            ));
        }
        Self::new(password)
    }

    fn from_file(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(AppError::InvalidOption(
                "Password file path cannot be empty".to_string(),
            ));
        }
        let contents = read_to_string(path)?;
        let password = contents.trim_end_matches(['\r', '\n']).to_string();
        Self::new(password)
    }

    fn as_str(&self) -> &str {
        &self.0
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
                append_line(to_string_pretty(&json!({
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

#[cfg(test)]
mod tests {
    use std::fs::write;

    use tempfile::tempdir;

    use super::{NewPassword, UserSetPassword};
    use crate::commands::CommandArgs;

    #[test]
    fn password_file_removes_only_line_endings() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("password");
        write(&path, " leading and trailing spaces \r\n")
            .expect("password fixture should be written");

        let password = NewPassword::from_file(path).expect("password should be loaded");

        assert_eq!(password.as_str(), " leading and trailing spaces ");
    }

    #[test]
    fn password_file_rejects_empty_passwords() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("password");
        write(&path, "\n").expect("password fixture should be written");

        let error = match NewPassword::from_file(path) {
            Ok(_) => panic!("empty password should be rejected"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("cannot be empty"));
    }

    #[test]
    fn set_password_accepts_only_prompt_or_file_input() {
        let options = UserSetPassword::options();

        assert!(options
            .iter()
            .any(|option| option.long.as_deref() == Some("--password-file")));
        assert!(!options
            .iter()
            .any(|option| option.long.as_deref() == Some("--password")));
    }
}
