use crate::domain::ComputedFieldRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ComputedFieldRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ID", self.id.to_string()),
            ("Class ID", self.class_id.to_string()),
            ("Visibility", self.visibility.clone()),
            ("Key", self.key.clone()),
            ("Label", self.label.clone()),
            ("Description", self.description.clone()),
            ("Operation", self.operation.clone()),
            ("Paths", self.paths.join(", ")),
            ("Result type", self.result_type.clone()),
            ("Enabled", self.enabled.to_string()),
            ("Revision", self.revision.to_string()),
            ("Created", self.created_at.clone()),
            ("Updated", self.updated_at.clone()),
        ]
    }
}

impl TableRenderable for ComputedFieldRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Key",
            "Label",
            "Visibility",
            "Operation",
            "Paths",
            "Result",
            "Enabled",
            "Revision",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.key.clone(),
            self.label.clone(),
            self.visibility.clone(),
            self.operation.clone(),
            self.paths.join(", "),
            self.result_type.clone(),
            self.enabled.to_string(),
            self.revision.to_string(),
        ]
    }
}
