use crate::domain::{CollectionRecord, GroupPermissionsSummary};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for CollectionRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let collection = &self.0;
        vec![
            ("Name", collection.name.clone()),
            ("Description", collection.description.clone()),
            ("Created", collection.created_at.to_string()),
            ("Updated", collection.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for CollectionRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Name", "Description", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let collection = &self.0;
        vec![
            collection.id.to_string(),
            collection.name.clone(),
            collection.description.clone(),
            collection.created_at.to_string(),
            collection.updated_at.to_string(),
        ]
    }
}

impl TableRenderable for GroupPermissionsSummary {
    fn headers() -> Vec<&'static str> {
        vec![
            "Group",
            "Collection",
            "Class",
            "Object",
            "Class Relation",
            "Object Relation",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.group.clone(),
            self.collection.clone(),
            self.class.clone(),
            self.object.clone(),
            self.class_relation.clone(),
            self.object_relation.clone(),
        ]
    }
}
