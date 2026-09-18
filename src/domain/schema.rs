use hubuum_client::{
    ClassSchemaResponse, SchemaActivationResponse, SchemaCompliancePage, SchemaRevisionResponse,
    SchemaWorkResponse,
};
use serde::Serialize;

/// Keep the complete response for structured output while selecting a concise text view.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SchemaOutput {
    State(ClassSchemaResponse),
    Revision(SchemaRevisionResponse),
    Revisions(Vec<SchemaRevisionResponse>),
    Compliance(SchemaCompliancePage),
    Activation(SchemaActivationResponse),
    Work(Box<SchemaWorkResponse>),
}
