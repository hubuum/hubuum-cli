use std::collections::HashMap;

use hubuum_client::{Collection, ExportJsonResponse, ExportResult, ExportTemplate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportTemplateRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub collection: String,
    pub content_type: String,
    pub template: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ExportTemplateRecord {
    pub fn new(template: &ExportTemplate, collectionmap: &HashMap<i32, Collection>) -> Self {
        let collection = collectionmap
            .get(&template.collection_id.into())
            .map(|collection| collection.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        Self {
            id: template.id.into(),
            name: template.name.clone(),
            description: template.description.clone(),
            collection,
            content_type: template.content_type.to_string(),
            template: template.template.clone(),
            created_at: template.created_at.to_string(),
            updated_at: template.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedExport {
    pub content_type: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportOutput {
    Json { body: ExportJsonResponse },
    Rendered(RenderedExport),
}

impl From<ExportResult> for ExportOutput {
    fn from(value: ExportResult) -> Self {
        match value {
            ExportResult::Json(body) => Self::Json { body },
            ExportResult::Rendered { content_type, body } => Self::Rendered(RenderedExport {
                content_type: content_type.to_string(),
                body,
            }),
        }
    }
}
