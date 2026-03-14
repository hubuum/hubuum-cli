use std::collections::HashMap;

use hubuum_client::{
    Class, ClassRelation, ClassWithPath, Namespace, Object, ObjectRelation, ObjectWithPath,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedClassRelationRecord {
    pub id: i32,
    pub class_a: String,
    pub class_b: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedClassRelationRecord {
    pub fn new(class_relation: &ClassRelation, classmap: &HashMap<i32, Class>) -> Self {
        let class_a = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let class_b = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();

        Self {
            id: class_relation.id,
            class_a,
            class_b,
            created_at: class_relation.created_at.to_string(),
            updated_at: class_relation.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedObjectRelationRecord {
    pub id: i32,
    pub class_a: String,
    pub class_b: String,
    pub object_a: String,
    pub object_b: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedObjectRelationRecord {
    pub fn new(
        object_relation: &ObjectRelation,
        class_relation: &ClassRelation,
        objectmap: &HashMap<i32, Object>,
        classmap: &HashMap<i32, Class>,
    ) -> Self {
        let class_a = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let class_b = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let object_a = objectmap
            .get(&object_relation.from_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        let object_b = objectmap
            .get(&object_relation.to_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();

        Self {
            id: object_relation.id,
            class_a,
            class_b,
            object_a,
            object_b,
            created_at: object_relation.created_at.to_string(),
            updated_at: object_relation.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedClassRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub depth: usize,
    pub path: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedRelatedClassRecord {
    pub fn new(
        class: &ClassWithPath,
        namespacemap: &HashMap<i32, Namespace>,
        path_labels: Vec<String>,
    ) -> Self {
        let namespace = namespacemap
            .get(&class.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| class.namespace_id.to_string());

        Self {
            id: class.id,
            name: class.name.clone(),
            description: class.description.clone(),
            namespace,
            depth: class.path.len().saturating_sub(1),
            path: path_labels,
            created_at: class.created_at.to_string(),
            updated_at: class.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedObjectRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub class: String,
    pub depth: usize,
    pub path: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedRelatedObjectRecord {
    pub fn new(
        object: &ObjectWithPath,
        classmap: &HashMap<i32, Class>,
        namespacemap: &HashMap<i32, Namespace>,
        path_labels: Vec<String>,
    ) -> Self {
        let namespace = namespacemap
            .get(&object.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| object.namespace_id.to_string());
        let class = classmap
            .get(&object.hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_else(|| object.hubuum_class_id.to_string());

        Self {
            id: object.id,
            name: object.name.clone(),
            description: object.description.clone(),
            namespace,
            class,
            depth: object.path.len().saturating_sub(1),
            path: path_labels,
            created_at: object.created_at.to_string(),
            updated_at: object.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedObjectGraph {
    pub objects: Vec<ResolvedRelatedObjectRecord>,
    pub relations: Vec<ResolvedObjectRelationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedClassGraph {
    pub classes: Vec<ResolvedRelatedClassRecord>,
    pub relations: Vec<ResolvedClassRelationRecord>,
}
