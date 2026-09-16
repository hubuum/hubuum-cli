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
    StageValidation {
        validate_schema: bool,
    },
    StageFromRevision {
        revision: SchemaRevision,
        validate_schema: Option<bool>,
    },
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
            SchemaOperation::StageValidation { validate_schema } => {
                let active = schema.get()?.active;
                if validate_schema && active.json_schema.as_ref().is_none_or(Value::is_null) {
                    return Err(AppError::InvalidOption(
                        "The active policy has no schema; provide --schema with --validate true"
                            .into(),
                    ));
                }
                to_value(schema.stage(SchemaStageRequest {
                    json_schema: active.json_schema,
                    validate_schema,
                })?)?
            }
            SchemaOperation::StageFromRevision {
                revision,
                validate_schema,
            } => {
                let source = schema.revision(revision)?;
                let validate_schema = validate_schema.unwrap_or(source.validate_schema);
                if validate_schema && source.json_schema.as_ref().is_none_or(Value::is_null) {
                    return Err(AppError::InvalidOption("The source revision has no schema; provide --schema instead of --from-revision to enable validation".into()));
                }
                to_value(schema.stage(SchemaStageRequest {
                    json_schema: source.json_schema,
                    validate_schema,
                })?)?
            }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{Method, StatusCode};
    use serde_json::{from_slice, json};

    use super::*;

    fn gateway(transport: &MockTransport, schema: Value) -> HubuumGateway {
        gateway_with_response(
            transport,
            json!({"active": revision(schema, false), "counts": {
            "valid": 0, "invalid": 0, "pending": 0, "not_required": 1
        }, "object_epoch": 1}),
        )
    }

    fn gateway_with_response(transport: &MockTransport, response: Value) -> HubuumGateway {
        let class = json!({
            "id": 9, "name": "Hosts", "description": "", "revision": 1,
            "created_at": "2026-09-16T00:00:00Z", "updated_at": "2026-09-16T00:00:00Z",
            "collection": {"id": 7, "name": "Inventory", "description": "", "revision": 1,
                "created_at": "2026-09-16T00:00:00Z", "updated_at": "2026-09-16T00:00:00Z"}
        });
        for response in [class, response] {
            transport.push_response(TransportResponse::json(StatusCode::OK, &response).unwrap());
        }
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("test-token"));
        HubuumGateway::new(Arc::new(client))
    }

    fn revision(schema: Value, validate: bool) -> Value {
        json!({"class_id": 9, "revision": 2, "json_schema": schema,
            "validate_schema": validate, "status": "staged", "created_at": "2026-09-16T00:00:00Z"})
    }

    #[test]
    fn validation_toggle_stages_the_exact_active_schema_without_activation() {
        // Boolean false is a real JSON schema, not an absent schema.
        for schema in [json!({"type":"object","required":["serial"]}), json!(false)] {
            for validate in [true, false] {
                let transport = MockTransport::default();
                let gateway = gateway(&transport, schema.clone());
                transport.push_response(
                    TransportResponse::json(StatusCode::OK, &revision(schema.clone(), validate))
                        .unwrap(),
                );
                gateway
                    .schema_operation(
                        "Hosts",
                        SchemaOperation::StageValidation {
                            validate_schema: validate,
                        },
                    )
                    .unwrap();
                let requests = transport.requests();
                assert_eq!(requests.len(), 3);
                assert_eq!(requests[1].method, Method::GET);
                assert_eq!(requests[1].url.path(), "/api/v1/classes/9/schema");
                assert_eq!(requests[2].method, Method::POST);
                assert_eq!(requests[2].url.path(), "/api/v1/classes/9/schema/revisions");
                assert_eq!(
                    from_slice::<Value>(requests[2].body()).unwrap(),
                    json!({"json_schema": schema, "validate_schema": validate})
                );
            }
        }
    }

    #[test]
    fn restoring_a_revision_copies_its_policy_into_a_new_proposal() {
        for override_validation in [None, Some(false)] {
            let transport = MockTransport::default();
            let policy = json!({"type":"object","required":["old_field"]});
            let mut old = revision(policy.clone(), true);
            old["revision"] = json!(4);
            old["status"] = json!("superseded");
            let gateway = gateway_with_response(&transport, old);
            transport.push_response(
                TransportResponse::json(
                    StatusCode::OK,
                    &revision(policy.clone(), override_validation.unwrap_or(true)),
                )
                .unwrap(),
            );
            gateway
                .schema_operation(
                    "Hosts",
                    SchemaOperation::StageFromRevision {
                        revision: SchemaRevision::new(4).unwrap(),
                        validate_schema: override_validation,
                    },
                )
                .unwrap();
            let requests = transport.requests();
            assert_eq!(requests.len(), 3);
            assert_eq!(
                requests[1].url.path(),
                "/api/v1/classes/9/schema/revisions/4"
            );
            assert_eq!(requests[2].url.path(), "/api/v1/classes/9/schema/revisions");
            assert_eq!(
                from_slice::<Value>(requests[2].body()).unwrap(),
                json!({
                    "json_schema": policy, "validate_schema": override_validation.unwrap_or(true)
                })
            );
        }
    }

    #[test]
    fn enabling_validation_without_an_active_schema_explains_how_to_supply_one() {
        let transport = MockTransport::default();
        let gateway = gateway(&transport, Value::Null);
        let error = gateway
            .schema_operation(
                "Hosts",
                SchemaOperation::StageValidation {
                    validate_schema: true,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("provide --schema"));
        assert_eq!(transport.requests().len(), 2);
    }
}
