use hubuum_client::Namespace;

use super::{append_key_value, OutputFormatterWithPadding};
use crate::errors::AppError;

impl OutputFormatterWithPadding for Namespace {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("Name", &self.name, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Created At", &self.created_at, padding)?;
        append_key_value("Updated At", &self.updated_at, padding)?;
        Ok(())
    }
}
