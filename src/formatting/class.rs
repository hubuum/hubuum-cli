use hubuum_client::{client::sync::Handle, Class};

use super::{append_key_value, append_some_key_value, OutputFormatter};
use crate::config::get_config;
use crate::errors::AppError;

impl OutputFormatter for Class {
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        append_key_value("Name", &self.name, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Namespace", &self.namespace.name, padding)?;

        let schema = &self.json_schema;
        let schema_id = schema
            .as_ref()
            .and_then(|s| s.as_object())
            .and_then(|o| o.get("$id").and_then(|v| v.as_str().map(|s| s.to_string())));

        if let Some(id) = schema_id {
            append_key_value("Schema", &id, padding)?;
        } else if schema.is_some() {
            append_key_value("Schema", "<schema without $id>", padding)?;
        } else {
            append_key_value("Schema", "<no schema>", padding)?;
        }

        append_some_key_value("Validate", &self.validate_schema, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(self.clone())
    }
}

impl OutputFormatter for Handle<Class> {
    fn format(&self) -> Result<Self, AppError> {
        self.resource().format()?;
        Ok(self.clone())
    }
}
