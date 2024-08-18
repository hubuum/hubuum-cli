use hubuum_client::Class;

use super::{append_key_value, OutputFormatter};
use crate::errors::AppError;

impl OutputFormatter for Class {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("Name", &self.name, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Namespace", &self.namespace_id, padding)?;

        let schema = Some(&self.json_schema);

        // The schema might be a large JSON object, so we'll just print the $id field
        if schema.is_none() {
            append_key_value("Schema", "<no schema>", padding)?;
        } else {
            let schema = schema.and_then(|s| s.as_object()?.get("$id"));
            if schema.is_none() {
                append_key_value("Schema", "<no $id>", padding)?;
            } else {
                append_key_value("Schema", &schema.unwrap(), padding)?;
            }
        }
        append_key_value("Validate", &self.validate_schema, padding)?;
        append_key_value("Created At", &self.created_at, padding)?;
        append_key_value("Updated At", &self.updated_at, padding)?;
        Ok(())
    }
}
