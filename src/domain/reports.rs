use std::collections::HashMap;

use hubuum_client::{Namespace, ReportResult, ReportTemplate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportTemplateRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub content_type: String,
    pub template: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ReportTemplateRecord {
    pub fn new(template: &ReportTemplate, namespacemap: &HashMap<i32, Namespace>) -> Self {
        let namespace = namespacemap
            .get(&template.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        Self {
            id: template.id,
            name: template.name.clone(),
            description: template.description.clone(),
            namespace,
            content_type: template.content_type.to_string(),
            template: template.template.clone(),
            created_at: template.created_at.to_string(),
            updated_at: template.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedReport {
    pub content_type: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportOutput {
    Json {
        body: hubuum_client::ReportJsonResponse,
    },
    Rendered(RenderedReport),
}

impl From<ReportResult> for ReportOutput {
    fn from(value: ReportResult) -> Self {
        match value {
            ReportResult::Json(body) => Self::Json { body },
            ReportResult::Rendered { content_type, body } => Self::Rendered(RenderedReport {
                content_type: content_type.to_string(),
                body,
            }),
        }
    }
}
