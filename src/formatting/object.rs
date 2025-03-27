use std::collections::HashMap;

use tabled::Tabled;
use serde::{Deserialize, Serialize};

use hubuum_client::{
    resources::{tabled_display, tabled_display_option},
    Class, Namespace, Object,
};

use super::{append_key_value, OutputFormatterWithPadding};
use crate::errors::AppError;

// A wrapper for objects that can be outputted where class_ids and namespace_ids are replaced with their names.
#[derive(Debug, Tabled, Clone, Serialize, Deserialize)]
pub struct FormattedObject {
    pub id: i32,
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Description")]
    pub description: String,
    #[tabled(rename = "Namespace")]
    pub namespace: String,
    #[tabled(rename = "Class")]
    pub class: String,
    #[tabled(display_with = "tabled_display_option", rename = "Data")]
    pub data: Option<serde_json::Value>,
    #[tabled(display_with = "tabled_display", rename = "Created")]
    pub created_at: chrono::NaiveDateTime,
    #[tabled(display_with = "tabled_display", rename = "Updated")]
    pub updated_at: chrono::NaiveDateTime,
}

impl FormattedObject {
    pub fn new(
        object: &Object,
        classmap: &HashMap<i32, Class>,
        namespacemap: &HashMap<i32, Namespace>,
    ) -> Self {
        let namespace = if namespacemap.get(&object.namespace_id).is_some() {
            namespacemap.get(&object.namespace_id).unwrap().name.clone()
        } else {
            "<unknown>".to_string()
        };

        let class = if classmap.get(&object.hubuum_class_id).is_some() {
            classmap.get(&object.hubuum_class_id).unwrap().name.clone()
        } else {
            "<unknown>".to_string()
        };

        Self {
            id: object.id,
            name: object.name.clone(),
            description: object.description.clone(),
            namespace,
            class,
            data: object.data.clone(),
            created_at: object.created_at,
            updated_at: object.updated_at,
        }
    }
}

impl OutputFormatterWithPadding for FormattedObject {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("Name", &self.name, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Namespace", &self.namespace, padding)?;
        append_key_value("Class", &self.class, padding)?;

        let data = &self.data;

        let size = if data.is_some() {
            data.as_ref().unwrap().to_string().len()
        } else {
            0
        };

        append_key_value("Data", size, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(())
    }
}
