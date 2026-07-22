use std::collections::HashMap;

use hubuum_client::{Class, Collection, Object};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::RelatedObjectTreeNode;

transparent_record!(ObjectRecord, Object);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectDataMutationOutcome {
    Patched,
    Created,
}

impl ObjectDataMutationOutcome {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Patched => "Patched",
            Self::Created => "Created",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectDataMutationRecord {
    pub outcome: ObjectDataMutationOutcome,
    pub class: String,
    pub object: Object,
}

impl ObjectDataMutationRecord {
    pub fn new(
        outcome: ObjectDataMutationOutcome,
        class: impl Into<String>,
        object: Object,
    ) -> Self {
        Self {
            outcome,
            class: class.into(),
            object,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedObjectRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub collection: String,
    pub class: String,
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedObjectRecord {
    pub fn new(
        object: &Object,
        classmap: &HashMap<i32, Class>,
        collectionmap: &HashMap<i32, Collection>,
    ) -> Self {
        let collection = collectionmap
            .get(&object.collection_id.into())
            .map(|collection| collection.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        let class = classmap
            .get(&object.hubuum_class_id.into())
            .map(|class| class.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        Self {
            id: object.id.into(),
            name: object.name.clone(),
            description: object.description.clone(),
            collection,
            class,
            data: object.data.clone(),
            computed: None,
            created_at: object.created_at.to_string(),
            updated_at: object.updated_at.to_string(),
        }
    }

    pub fn with_computed(mut self, computed: Value) -> Self {
        self.computed = Some(computed);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectShowRecord {
    #[serde(flatten)]
    pub object: ResolvedObjectRecord,
    pub related_objects: Vec<RelatedObjectTreeNode>,
}
