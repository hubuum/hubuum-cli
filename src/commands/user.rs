use cli_command_derive::CliCommand;
use hubuum_client::{Authenticated, SyncClient, UserParams};
use log::trace;
use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::output::{add_warning, append_line};
use crate::tokenizer::CommandTokenizer;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct UserNew {
    #[option(short = "n", long = "name", help = "Name of the user")]
    pub name: String,
    #[option(short = "e", long = "email", help = "Email address for the user")]
    pub email: Option<String>,
}

impl CliCommand for UserNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
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

impl CliCommand for UserInfo {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let query = self.new_from_tokens(tokens)?;

        let pos0 = tokens.get_positionals().get(0);
        let username = if query.username.is_none() {
            if pos0.is_none() {
                return Err(AppError::MissingOptions(vec!["username".to_string()]));
            }
            Some(pos0.unwrap().clone())
        } else {
            query.username.clone()
        };

        let user_query = UserParams {
            username,
            id: None,
            email: self.email.clone(),
            created_at: query.created_at.clone(),
            updated_at: query.updated_at.clone(),
        };

        let now = chrono::Utc::now();
        let users = client.get(hubuum_client::User::default(), user_query)?;
        let elapsed = chrono::Utc::now().signed_duration_since(now);
        trace!("Query time: {:?}ms", elapsed.num_milliseconds());

        if users.is_empty() {
            add_warning("User not found")?;
        } else if users.len() > 1 {
            add_warning("Multiple users found.")?;
        } else {
            for user in users {
                append_line(&format!("Username: {}", user.username))?;
                append_line(&format!("Email: {}", user.email.unwrap_or_default()))?;
                append_line(&format!("Created at: {}", user.created_at))?;
                append_line(&format!("Updated at: {}", user.updated_at))?;
            }
        }

        Ok(())
    }
}
