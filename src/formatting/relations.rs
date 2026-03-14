use crate::domain::{
    ResolvedClassRelationRecord, ResolvedObjectRelationRecord, ResolvedRelatedClassRecord,
    ResolvedRelatedObjectRecord,
};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ResolvedClassRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassA", self.class_a.clone()),
            ("ClassB", self.class_b.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedClassRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "ClassA", "ClassB", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.class_a.clone(),
            self.class_b.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedObjectRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassA", self.class_a.clone()),
            ("ClassB", self.class_b.clone()),
            ("ObjectA", self.object_a.clone()),
            ("ObjectB", self.object_b.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedObjectRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id", "ClassA", "ClassB", "ObjectA", "ObjectB", "Created", "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.class_a.clone(),
            self.class_b.clone(),
            self.object_a.clone(),
            self.object_b.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedRelatedClassRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Depth", self.depth.to_string()),
            ("Path", self.path.join(" -> ")),
            ("Created", self.created_at.clone()),
            ("Updated", self.updated_at.clone()),
        ]
    }
}

impl TableRenderable for ResolvedRelatedClassRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Namespace",
            "Depth",
            "Path",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.namespace.clone(),
            self.depth.to_string(),
            self.path.join(" -> "),
            self.created_at.clone(),
            self.updated_at.clone(),
        ]
    }
}

impl DetailRenderable for ResolvedRelatedObjectRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Class", self.class.clone()),
            ("Depth", self.depth.to_string()),
            ("Path", self.path.join(" -> ")),
            ("Created", self.created_at.clone()),
            ("Updated", self.updated_at.clone()),
        ]
    }
}

impl TableRenderable for ResolvedRelatedObjectRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Class",
            "Namespace",
            "Depth",
            "Path",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.class.clone(),
            self.namespace.clone(),
            self.depth.to_string(),
            self.path.join(" -> "),
            self.created_at.clone(),
            self.updated_at.clone(),
        ]
    }
}
