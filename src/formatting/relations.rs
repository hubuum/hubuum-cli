use crate::domain::{ResolvedClassRelationRecord, ResolvedObjectRelationRecord};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ResolvedClassRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassFrom", self.from_class.clone()),
            ("ClassTo", self.to_class.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedClassRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "FromClass", "ToClass", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.from_class.clone(),
            self.to_class.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedObjectRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassFrom", self.from_class.clone()),
            ("ClassTo", self.to_class.clone()),
            ("ObjectFrom", self.from_object.clone()),
            ("ObjectTo", self.to_object.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedObjectRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "FromClass",
            "ToClass",
            "FromObject",
            "ToObject",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.from_class.clone(),
            self.to_class.clone(),
            self.from_object.clone(),
            self.to_object.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}
