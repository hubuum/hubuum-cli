use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use rand::distr::Alphanumeric;
use rand::{rng, RngExt};

use crate::catalog::CommandCatalogBuilder;
use crate::domain::CreatedUser;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::{AppServices, CreateUserInput, UserFilter, UserUpdateInput};
use crate::tokenizer::CommandTokenizer;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};

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
        );
}

trait GetUsername {
    fn username(&self) -> Option<String>;
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
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: Option<String>,
}

impl GetUsername for &UserDelete {
    fn username(&self) -> Option<String> {
        self.username.clone()
    }
}

impl CliCommand for UserDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;

        query.username = username_or_pos(&query, tokens, 0)?;

        let username = query
            .username
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["username".to_string()]))?;
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
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: Option<String>,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
    #[option(short = "C", long = "created-at", help = "Created at timestammp")]
    pub created_at: Option<chrono::NaiveDateTime>,
    #[option(short = "U", long = "updated-at", help = "Updated at timestamp")]
    pub updated_at: Option<chrono::NaiveDateTime>,
}

impl GetUsername for &UserInfo {
    fn username(&self) -> Option<String> {
        self.username.clone()
    }
}

impl CliCommand for UserInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;

        query.username = username_or_pos(&query, tokens, 0)?;

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
}

impl CliCommand for UserList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let users = services.gateway().list_users(UserFilter {
            username: query.username,
            email: query.email,
            created_at: query.created_at,
            updated_at: query.updated_at,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => users.format_json_noreturn()?,
            OutputFormat::Text => users.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct UserModify {
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the user")]
    pub rename: Option<String>,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
}

impl GetUsername for &UserModify {
    fn username(&self) -> Option<String> {
        self.username.clone()
    }
}

impl CliCommand for UserModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.username = username_or_pos(&query, tokens, 0)?;

        let username = query
            .username
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["username".to_string()]))?;
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

fn username_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<String>, AppError>
where
    U: GetUsername,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.username().is_none() {
        if pos0.is_none() {
            return Err(AppError::MissingOptions(vec!["username".to_string()]));
        }
        return Ok(pos0.cloned());
    };
    Ok(query.username().clone())
}
