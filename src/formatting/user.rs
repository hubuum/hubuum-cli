use hubuum_client::User;

use super::{append_key_value, append_some_key_value, OutputFormatterWithPadding};
use crate::errors::AppError;

impl OutputFormatterWithPadding for User {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("Username", &self.username, padding)?;
        append_some_key_value("Email", &self.email, padding)?;
        append_key_value("Created", &self.created_at, padding)?;
        append_key_value("Updated", &self.updated_at, padding)?;
        Ok(())
    }
}
