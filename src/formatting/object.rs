use hubuum_client::Object;

use super::{append_key_value, append_some_key_value, OutputFormatterWithPadding};
use crate::errors::AppError;

impl OutputFormatterWithPadding for Object {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("Name", &self.name, padding)?;
        append_some_key_value("Description", &self.description, padding)?;
        append_key_value("Namespace", &self.namespace_id, padding)?;
        append_key_value("Class", &self.hubuum_class_id, padding)?;

        let data = &self.data;

        let size = if data.is_some() {
            data.as_ref().unwrap().to_string().len()
        } else {
            0
        };

        append_key_value("Data", size, padding)?;
        append_key_value("Created", &self.created_at, padding)?;
        append_key_value("Updated", &self.updated_at, padding)?;
        Ok(())
    }
}
