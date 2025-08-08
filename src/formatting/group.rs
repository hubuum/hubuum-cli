use hubuum_client::{client::sync::Handle, Group};

use super::{append_key_value, OutputFormatter};
use crate::config::get_config;
use crate::errors::AppError;

impl OutputFormatter for Group {
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        append_key_value("Name", &self.groupname, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(self.clone())
    }
}

impl OutputFormatter for Handle<Group> {
    fn format(&self) -> Result<Self, AppError> {
        self.resource().format()?;
        Ok(self.clone())
    }
}
