use cli_command_derive::CliCommand;
use hubuum_client::{
    Authenticated, FilterOperator, IntoResourceFilter, QueryFilter, SyncClient, User, UserPost,
};
use serde::{Deserialize, Serialize};

use rand::distr::Alphanumeric;
use rand::{rng, Rng};

use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};

use crate::tokenizer::CommandTokenizer;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

trait GetUsername {
    fn username(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct UserNew {
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: String,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
}

impl UserNew {
    fn into_post(self) -> UserPost {
        UserPost {
            username: self.username.clone(),
            email: self.email.clone(),
            password: generate_random_password(20),
        }
    }
}

impl CliCommand for UserNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?.into_post();
        let password = new.password.clone();

        let user = client.users().create(new)?;

        match self.desired_format(tokens) {
            OutputFormat::Json => {
                let mut json = serde_json::to_value(&user)?;
                json["password"] = serde_json::to_value(password)?;
                append_line(serde_json::to_string_pretty(&json)?)?;
            }
            OutputFormat::Text => {
                user.format_noreturn()?;
                append_key_value("Password", password, 15)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct UserDelete {
    #[option(short = "u", long = "username", help = "Username of the user")]
    pub username: Option<String>,
}

impl IntoResourceFilter<User> for &UserDelete {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];

        if let Some(username) = &self.username {
            filters.push(QueryFilter {
                key: "username".to_string(),
                value: username.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        filters
    }
}

impl GetUsername for &UserDelete {
    fn username(&self) -> Option<String> {
        self.username.clone()
    }
}

impl CliCommand for UserDelete {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut query = self.new_from_tokens(tokens)?;

        query.username = username_or_pos(&query, tokens, 0)?;

        let user = client.users().filter_expecting_single_result(&query)?;

        client.users().delete(user.id)?;

        let message = format!("User '{}' deleted", user.username);

        match self.desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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

impl IntoResourceFilter<User> for &UserInfo {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];

        if let Some(username) = &self.username {
            filters.push(QueryFilter {
                key: "username".to_string(),
                value: username.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        if let Some(email) = &self.email {
            filters.push(QueryFilter {
                key: "email".to_string(),
                value: email.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        if let Some(created_at) = &self.created_at {
            filters.push(QueryFilter {
                key: "created_at".to_string(),
                value: created_at.to_string(),
                operator: FilterOperator::Equals { is_negated: false },
            });
        }

        if let Some(updated_at) = &self.updated_at {
            filters.push(QueryFilter {
                key: "updated_at".to_string(),
                value: updated_at.to_string(),
                operator: FilterOperator::Equals { is_negated: false },
            });
        }

        filters
    }
}

impl GetUsername for &UserInfo {
    fn username(&self) -> Option<String> {
        self.username.clone()
    }
}

impl CliCommand for UserInfo {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut query = self.new_from_tokens(tokens)?;

        query.username = username_or_pos(&query, tokens, 0)?;

        let user = client.users().filter_expecting_single_result(&query)?;

        match self.desired_format(tokens) {
            OutputFormat::Json => user.format_json_noreturn()?,
            OutputFormat::Text => user.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let _ = self.new_from_tokens(tokens)?;
        let users = client.users().find().execute()?;

        match self.desired_format(tokens) {
            OutputFormat::Json => users.format_json_noreturn()?,
            OutputFormat::Text => users.format_noreturn()?,
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
