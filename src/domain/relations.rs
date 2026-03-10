use std::collections::HashMap;

use hubuum_client::{Class, ClassRelation, Object, ObjectRelation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedClassRelationRecord {
    pub id: i32,
    pub from_class: String,
    pub to_class: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedClassRelationRecord {
    pub fn new(class_relation: &ClassRelation, classmap: &HashMap<i32, Class>) -> Self {
        let from_class = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let to_class = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();

        Self {
            id: class_relation.id,
            from_class,
            to_class,
            created_at: class_relation.created_at.to_string(),
            updated_at: class_relation.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedObjectRelationRecord {
    pub id: i32,
    pub from_class: String,
    pub to_class: String,
    pub from_object: String,
    pub to_object: String,
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
        let from_class = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let to_class = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let from_object = objectmap
            .get(&object_relation.from_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        let to_object = objectmap
            .get(&object_relation.to_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();

        Self {
            id: object_relation.id,
            from_class,
            to_class,
            from_object,
            to_object,
            created_at: object_relation.created_at.to_string(),
            updated_at: object_relation.updated_at.to_string(),
        }
    }
}
