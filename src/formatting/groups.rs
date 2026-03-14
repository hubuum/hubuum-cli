use crate::domain::GroupRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for GroupRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let group = &self.0;
        vec![
            ("Name", group.groupname.clone()),
            ("Description", group.description.clone()),
            ("Created", group.created_at.to_string()),
            ("Updated", group.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for GroupRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Name", "Description", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let group = &self.0;
        vec![
            group.id.to_string(),
            group.groupname.clone(),
            group.description.clone(),
            group.created_at.to_string(),
            group.updated_at.to_string(),
        ]
    }
}
