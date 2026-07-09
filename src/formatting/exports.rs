use crate::domain::ExportTemplateRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ExportTemplateRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Collection", self.collection.clone()),
            ("Content-Type", self.content_type.clone()),
            ("Template", self.template.clone()),
            ("Created", self.created_at.clone()),
            ("Updated", self.updated_at.clone()),
        ]
    }
}

impl TableRenderable for ExportTemplateRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Collection",
            "Content-Type",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.description.clone(),
            self.collection.clone(),
            self.content_type.clone(),
            self.created_at.clone(),
            self.updated_at.clone(),
        ]
    }
}
