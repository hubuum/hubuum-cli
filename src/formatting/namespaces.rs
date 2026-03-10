use crate::domain::{GroupPermissionsSummary, NamespaceRecord};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for NamespaceRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let namespace = &self.0;
        vec![
            ("Name", namespace.name.clone()),
            ("Description", namespace.description.clone()),
            ("Created", namespace.created_at.to_string()),
            ("Updated", namespace.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for NamespaceRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Name", "Description", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let namespace = &self.0;
        vec![
            namespace.id.to_string(),
            namespace.name.clone(),
            namespace.description.clone(),
            namespace.created_at.to_string(),
            namespace.updated_at.to_string(),
        ]
    }
}

impl TableRenderable for GroupPermissionsSummary {
    fn headers() -> Vec<&'static str> {
        vec![
            "Group",
            "Namespace",
            "Class",
            "Object",
            "Class Relation",
            "Object Relation",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.group.clone(),
            self.namespace.clone(),
            self.class.clone(),
            self.object.clone(),
            self.class_relation.clone(),
            self.object_relation.clone(),
        ]
    }
}
