use hubuum_client::{
    ClassSchemaResponse, SchemaActivationResponse, SchemaCompliancePage, SchemaProfileReport,
    SchemaRevisionResponse, SchemaWorkResponse, TaskId, TaskResponse,
};
use serde::Serialize;

/// Keep the complete response for structured output while selecting a concise text view.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SchemaOutput {
    State(ClassSchemaResponse),
    ProfileTask(Box<TaskResponse>),
    Suggestion {
        task_id: TaskId,
        #[serde(flatten)]
        report: Box<SchemaProfileReport>,
    },
    Revision(SchemaRevisionResponse),
    Revisions(Vec<SchemaRevisionResponse>),
    Compliance(SchemaCompliancePage),
    Activation(SchemaActivationResponse),
    Work(Box<SchemaWorkResponse>),
}
