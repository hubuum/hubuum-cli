use hubuum_client::Class;

use super::{append_key_value, append_some_key_value, OutputFormatterWithPadding};
use crate::errors::AppError;

impl OutputFormatterWithPadding for Class {
    fn format(&self, padding: usize) -> Result<(), AppError> {
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
        Ok(())
    }
}
