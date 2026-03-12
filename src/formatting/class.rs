use hubuum_client::{Class, HubuumDateTime};
use serde::Serialize;
use tabled::Tabled;

use super::{
    append_key_value, append_some_key_value, tabled_display, tabled_display_option, OutputFormatter,
};
use crate::config::get_config;
use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Tabled)]
pub struct FormattedClass {
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Namespace")]
    pub namespace: String,
    #[tabled(rename = "Description")]
    pub description: String,
    #[tabled(display = "tabled_display_option", rename = "Validate")]
    pub validate_schema: Option<bool>,
    #[tabled(display = "tabled_display", rename = "Created")]
    pub created_at: HubuumDateTime,
    #[tabled(display = "tabled_display", rename = "Updated")]
    pub updated_at: HubuumDateTime,
}

impl From<&Class> for FormattedClass {
    fn from(class: &Class) -> Self {
        Self {
            name: class.name.clone(),
            namespace: class.namespace.name.clone(),
            description: class.description.clone(),
            validate_schema: class.validate_schema,
            created_at: class.created_at.clone(),
            updated_at: class.updated_at.clone(),
        }
    }
}

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
        append_key_value("Created", &self.created_at, padding)?;
        append_key_value("Updated", &self.updated_at, padding)?;
        Ok(self.clone())
    }
}
