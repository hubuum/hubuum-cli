use std::collections::HashMap;

use hubuum_client::{Class, Namespace, Object};
use serde::{Deserialize, Serialize};

use super::RelatedObjectTreeNode;

transparent_record!(ObjectRecord, Object);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedObjectRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub class: String,
    pub data: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedObjectRecord {
    pub fn new(
        object: &Object,
        classmap: &HashMap<i32, Class>,
        namespacemap: &HashMap<i32, Namespace>,
    ) -> Self {
        let namespace = namespacemap
            .get(&object.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        let class = classmap
            .get(&object.hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        Self {
            id: object.id,
            name: object.name.clone(),
            description: object.description.clone(),
            namespace,
            class,
            data: object.data.clone(),
            created_at: object.created_at.to_string(),
            updated_at: object.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectShowRecord {
    #[serde(flatten)]
    pub object: ResolvedObjectRecord,
    pub related_objects: Vec<RelatedObjectTreeNode>,
}
