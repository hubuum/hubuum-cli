use hubuum_client::{
    ComplianceStatus, SchemaActivationRequest, SchemaPageOptions, SchemaRepairReportRequest,
    SchemaRevision, SchemaStageRequest, TaskId,
};
use serde_json::{to_value, Value};

use crate::errors::AppError;

use super::HubuumGateway;

pub enum SchemaOperation {
    Show,
    Revisions(SchemaPageOptions),
    Objects(SchemaPageOptions, Option<ComplianceStatus>),
    Stage(SchemaStageRequest),
    Revision(SchemaRevision),
    Abandon(SchemaRevision),
    Activate(SchemaRevision, SchemaActivationRequest),
    Impact(SchemaRevision),
    Revalidate(SchemaRevision),
    Work(TaskId),
    Cancel(TaskId),
}

impl HubuumGateway {
    pub fn schema_operation(
        &self,
        class: &str,
        operation: SchemaOperation,
    ) -> Result<Value, AppError> {
        let client = self.client();
        let class = client.classes().get_by_name(class)?;
        let schema = class.schema();
        Ok(match operation {
            SchemaOperation::Show => to_value(schema.get()?)?,
            SchemaOperation::Revisions(page) => to_value(schema.revisions(&page)?)?,
            SchemaOperation::Objects(page, status) => to_value(schema.objects(&page, status)?)?,
            SchemaOperation::Stage(request) => to_value(schema.stage(request)?)?,
            SchemaOperation::Revision(revision) => to_value(schema.revision(revision)?)?,
            SchemaOperation::Abandon(revision) => to_value(schema.abandon(revision)?)?,
            SchemaOperation::Activate(revision, request) => {
                to_value(schema.activate(revision, request)?)?
            }
            SchemaOperation::Impact(revision) => to_value(schema.impact(revision)?)?,
            SchemaOperation::Revalidate(revision) => to_value(schema.revalidate(revision)?)?,
            SchemaOperation::Work(task) => to_value(schema.work(task)?)?,
            SchemaOperation::Cancel(task) => to_value(schema.cancel_work(task)?)?,
        })
    }

    pub fn schema_report(
        &self,
        class: &str,
        task: TaskId,
        request: Option<SchemaRepairReportRequest>,
    ) -> Result<String, AppError> {
        let client = self.client();
        let class = client.classes().get_by_name(class)?;
        let schema = class.schema();
        Ok(match request {
            Some(request) => schema.generate_report(task, request)?,
            None => schema.report(task, true)?,
        })
    }
}
