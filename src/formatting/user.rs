use hubuum_client::User;

use super::{append_key_value, append_some_key_value, OutputFormatter};
use crate::config::get_config;
use crate::errors::AppError;

impl OutputFormatter for User {
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        append_key_value("Username", &self.username, padding)?;
        append_some_key_value("Email", &self.email, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(self.clone())
    }
}
