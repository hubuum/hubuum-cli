use hubuum_client::{HubuumDateTime, User};
use serde::Serialize;
use tabled::Tabled;

use super::{
    append_key_value, append_some_key_value, tabled_display, tabled_display_option, OutputFormatter,
};
use crate::config::get_config;
use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Tabled)]
pub struct FormattedUser {
    #[tabled(rename = "Username")]
    pub username: String,
    #[tabled(display = "tabled_display_option", rename = "Email")]
    pub email: Option<String>,
    #[tabled(display = "tabled_display", rename = "Created")]
    pub created_at: HubuumDateTime,
    #[tabled(display = "tabled_display", rename = "Updated")]
    pub updated_at: HubuumDateTime,
}

impl From<&User> for FormattedUser {
    fn from(user: &User) -> Self {
        Self {
            username: user.username.clone(),
            email: user.email.clone(),
            created_at: user.created_at.clone(),
            updated_at: user.updated_at.clone(),
        }
    }
}

impl OutputFormatter for User {
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        append_key_value("Username", &self.username, padding)?;
        append_some_key_value("Email", &self.email, padding)?;
        append_key_value("Created", &self.created_at, padding)?;
        append_key_value("Updated", &self.updated_at, padding)?;
        Ok(self.clone())
    }
}
