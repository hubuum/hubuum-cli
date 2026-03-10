use crate::domain::ResolvedObjectRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ResolvedObjectRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Class", self.class.clone()),
            ("Data", self.data_size().to_string()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedObjectRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Namespace",
            "Class",
            "Data",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.description.clone(),
            self.namespace.clone(),
            self.class.clone(),
            data_preview(self.data.as_ref()),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl ResolvedObjectRecord {
    fn data_size(&self) -> usize {
        self.data
            .as_ref()
            .map_or(0, |value| value.to_string().len())
    }
}

fn data_preview(data: Option<&serde_json::Value>) -> String {
    match data {
        Some(value) => {
            let compact = value.to_string();
            if compact.len() > 48 {
                format!("{}...", &compact[..45])
            } else {
                compact
            }
        }
        None => String::new(),
    }
}
