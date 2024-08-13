use hubuum_client::User;

use super::{append_key_value, append_some_key_value, OutputFormatter};
use crate::errors::AppError;

impl OutputFormatter for User {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("Username", &self.username, padding)?;
        append_some_key_value("Email", &self.email, padding)?;
        append_key_value("Created At", &self.created_at, padding)?;
        append_key_value("Updated At", &self.updated_at, padding)?;
        Ok(())
    }
}
