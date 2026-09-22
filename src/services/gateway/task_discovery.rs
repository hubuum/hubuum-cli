use crate::errors::AppError;
use crate::list_query::{
    parse_where_clause, FilterFieldSpec, FilterOperatorProfile, FilterValueProfile,
};
use chrono::DateTime;
use hubuum_client::{
    client::sync::TaskListRequest, ClassId, ClassRelationId, CollectionId, ComputationRevision,
    ExportScopeKind, ExportTemplateId, FilterOperator, HubuumDateTime, ImportAtomicity,
    ImportCollisionPolicy, ImportPermissionPolicy, ObjectId, ObjectRelationId, PrincipalId,
    RemoteTargetId, SchemaRevision, SchemaWorkKind, SchemaWorkStatus, TaskOutputDiscoveryState,
    TaskRemoteSideEffectState, TaskTerminalReason, TaskTraceId,
};
use serde::de::DeserializeOwned;
use serde_json::{from_value, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub(crate) struct TaskDiscovery {
    filters: Vec<TaskFilter>,
}

#[derive(Debug, Clone)]
enum TaskFilter {
    ClassId(ClassId),
    ObjectId(ObjectId),
    CollectionId(CollectionId),
    ClassRelation(ClassRelationId),
    ObjectRelation(ObjectRelationId),
    SchemaRevision(SchemaRevision),
    ComputationRevision(ComputationRevision),
    SchemaWorkKind(SchemaWorkKind),
    SchemaWorkStatus(SchemaWorkStatus),
    RemoteTargetId(RemoteTargetId),
    RemoteSideEffectState(TaskRemoteSideEffectState),
    ExportScopeKind(ExportScopeKind),
    ExportTemplateId(ExportTemplateId),
    ExportHasWarnings(bool),
    ExportTruncated(bool),
    ImportDryRun(bool),
    ImportAtomicity(ImportAtomicity),
    ImportCollisionPolicy(ImportCollisionPolicy),
    ImportPermissionPolicy(ImportPermissionPolicy),
    ImportHasFailedItems(bool),
    BackupIncludeHistory(bool),
    OutputState(TaskOutputDiscoveryState),
    Terminal(bool),
    CancelRequested(bool),
    TerminalReason(TaskTerminalReason),
    TraceId(TaskTraceId),
    SubmittedBy(PrincipalId),
    CreatedAfter(HubuumDateTime),
    CreatedBefore(HubuumDateTime),
    StartedAfter(HubuumDateTime),
    StartedBefore(HubuumDateTime),
    FinishedAfter(HubuumDateTime),
    FinishedBefore(HubuumDateTime),
}

impl TaskDiscovery {
    pub(crate) fn parse(clauses: &[String]) -> Result<Self, AppError> {
        let mut filters = Vec::new();
        let mut seen = HashSet::new();
        for clause in clauses {
            let clause = parse_where_clause(clause)?;
            if clause.operator != (FilterOperator::Equals { is_negated: false }) {
                return Err(AppError::InvalidOption("Task discovery uses equals; timestamp bounds use created_after, created_before, started_after, started_before, finished_after, or finished_before".into()));
            }
            if !seen.insert(clause.field.clone()) {
                return Err(AppError::InvalidOption(format!(
                    "Duplicate task discovery field '{}'",
                    clause.field
                )));
            }
            let value = clause.value.as_str();
            filters.push(match clause.field.as_str() {
                "class_id" => TaskFilter::ClassId(ClassId::from(positive_id(value)?)),
                "object_id" => TaskFilter::ObjectId(ObjectId::from(positive_id(value)?)),
                "collection_id" => {
                    TaskFilter::CollectionId(CollectionId::from(positive_id(value)?))
                }
                "class_relation" => {
                    TaskFilter::ClassRelation(ClassRelationId::from(positive_id(value)?))
                }
                "object_relation" => {
                    TaskFilter::ObjectRelation(ObjectRelationId::from(positive_id(value)?))
                }
                "schema_revision" => {
                    TaskFilter::SchemaRevision(SchemaRevision::new(value.parse()?)?)
                }
                "computation_revision" => {
                    TaskFilter::ComputationRevision(ComputationRevision::new(value.parse()?)?)
                }
                "schema_work_kind" => TaskFilter::SchemaWorkKind(parse_enum(value, &clause.field)?),
                "schema_work_status" => {
                    TaskFilter::SchemaWorkStatus(parse_enum(value, &clause.field)?)
                }
                "remote_target_id" => {
                    TaskFilter::RemoteTargetId(RemoteTargetId::from(positive_id(value)?))
                }
                "remote_side_effect_state" => {
                    TaskFilter::RemoteSideEffectState(parse_enum(value, &clause.field)?)
                }
                "export_scope_kind" => {
                    TaskFilter::ExportScopeKind(parse_enum(value, &clause.field)?)
                }
                "export_template_id" => {
                    TaskFilter::ExportTemplateId(ExportTemplateId::from(positive_id(value)?))
                }
                "export_has_warnings" => TaskFilter::ExportHasWarnings(value.parse()?),
                "export_truncated" => TaskFilter::ExportTruncated(value.parse()?),
                "import_dry_run" => TaskFilter::ImportDryRun(value.parse()?),
                "import_atomicity" => {
                    TaskFilter::ImportAtomicity(parse_enum(value, &clause.field)?)
                }
                "import_collision_policy" => {
                    TaskFilter::ImportCollisionPolicy(parse_enum(value, &clause.field)?)
                }
                "import_permission_policy" => {
                    TaskFilter::ImportPermissionPolicy(parse_enum(value, &clause.field)?)
                }
                "import_has_failed_items" => TaskFilter::ImportHasFailedItems(value.parse()?),
                "backup_include_history" => TaskFilter::BackupIncludeHistory(value.parse()?),
                "output_state" => TaskFilter::OutputState(parse_enum(value, &clause.field)?),
                "terminal" => TaskFilter::Terminal(value.parse()?),
                "cancel_requested" => TaskFilter::CancelRequested(value.parse()?),
                "terminal_reason" => TaskFilter::TerminalReason(parse_enum(value, &clause.field)?),
                "trace_id" => TaskFilter::TraceId(TaskTraceId::new(value)?),
                "submitted_by" => TaskFilter::SubmittedBy(PrincipalId::from(positive_id(value)?)),
                "created_after" => TaskFilter::CreatedAfter(HubuumDateTime(
                    DateTime::parse_from_rfc3339(value)
                        .map_err(|_| {
                            AppError::InvalidOption(format!(
                                "Expected an RFC3339 timestamp for {}",
                                clause.field
                            ))
                        })?
                        .to_utc(),
                )),
                "created_before" => TaskFilter::CreatedBefore(HubuumDateTime(
                    DateTime::parse_from_rfc3339(value)
                        .map_err(|_| {
                            AppError::InvalidOption(format!(
                                "Expected an RFC3339 timestamp for {}",
                                clause.field
                            ))
                        })?
                        .to_utc(),
                )),
                "started_after" => TaskFilter::StartedAfter(HubuumDateTime(
                    DateTime::parse_from_rfc3339(value)
                        .map_err(|_| {
                            AppError::InvalidOption(format!(
                                "Expected an RFC3339 timestamp for {}",
                                clause.field
                            ))
                        })?
                        .to_utc(),
                )),
                "started_before" => TaskFilter::StartedBefore(HubuumDateTime(
                    DateTime::parse_from_rfc3339(value)
                        .map_err(|_| {
                            AppError::InvalidOption(format!(
                                "Expected an RFC3339 timestamp for {}",
                                clause.field
                            ))
                        })?
                        .to_utc(),
                )),
                "finished_after" => TaskFilter::FinishedAfter(HubuumDateTime(
                    DateTime::parse_from_rfc3339(value)
                        .map_err(|_| {
                            AppError::InvalidOption(format!(
                                "Expected an RFC3339 timestamp for {}",
                                clause.field
                            ))
                        })?
                        .to_utc(),
                )),
                "finished_before" => TaskFilter::FinishedBefore(HubuumDateTime(
                    DateTime::parse_from_rfc3339(value)
                        .map_err(|_| {
                            AppError::InvalidOption(format!(
                                "Expected an RFC3339 timestamp for {}",
                                clause.field
                            ))
                        })?
                        .to_utc(),
                )),
                _ => {
                    return Err(AppError::InvalidOption(format!(
                        "Unknown task discovery field '{}'",
                        clause.field
                    )))
                }
            });
        }
        Ok(Self { filters })
    }

    pub(super) fn apply(self, mut query: TaskListRequest) -> TaskListRequest {
        for filter in self.filters {
            query = match filter {
                TaskFilter::ClassId(value) => query.class_id(value),
                TaskFilter::ObjectId(value) => query.object_id(value),
                TaskFilter::CollectionId(value) => query.collection_id(value),
                TaskFilter::ClassRelation(value) => query.class_relation(value),
                TaskFilter::ObjectRelation(value) => query.object_relation(value),
                TaskFilter::SchemaRevision(value) => query.schema_revision(value),
                TaskFilter::ComputationRevision(value) => query.computation_revision(value),
                TaskFilter::SchemaWorkKind(value) => query.schema_work_kind(value),
                TaskFilter::SchemaWorkStatus(value) => query.schema_work_status(value),
                TaskFilter::RemoteTargetId(value) => query.remote_target_id(value),
                TaskFilter::RemoteSideEffectState(value) => query.remote_side_effect_state(value),
                TaskFilter::ExportScopeKind(value) => query.export_scope_kind(value),
                TaskFilter::ExportTemplateId(value) => query.export_template_id(value),
                TaskFilter::ExportHasWarnings(value) => query.export_has_warnings(value),
                TaskFilter::ExportTruncated(value) => query.export_truncated(value),
                TaskFilter::ImportDryRun(value) => query.import_dry_run(value),
                TaskFilter::ImportAtomicity(value) => query.import_atomicity(value),
                TaskFilter::ImportCollisionPolicy(value) => query.import_collision_policy(value),
                TaskFilter::ImportPermissionPolicy(value) => query.import_permission_policy(value),
                TaskFilter::ImportHasFailedItems(value) => query.import_has_failed_items(value),
                TaskFilter::BackupIncludeHistory(value) => query.backup_include_history(value),
                TaskFilter::OutputState(value) => query.output_state(value),
                TaskFilter::Terminal(value) => query.terminal(value),
                TaskFilter::CancelRequested(value) => query.cancel_requested(value),
                TaskFilter::TerminalReason(value) => query.terminal_reason(value),
                TaskFilter::TraceId(value) => query.trace_id(value),
                TaskFilter::SubmittedBy(value) => query.submitted_by(value),
                TaskFilter::CreatedAfter(value) => query.created_after(value),
                TaskFilter::CreatedBefore(value) => query.created_before(value),
                TaskFilter::StartedAfter(value) => query.started_after(value),
                TaskFilter::StartedBefore(value) => query.started_before(value),
                TaskFilter::FinishedAfter(value) => query.finished_after(value),
                TaskFilter::FinishedBefore(value) => query.finished_before(value),
            };
        }
        query
    }
}

fn positive_id(value: &str) -> Result<i32, AppError> {
    let id: i32 = value.parse()?;
    if id <= 0 {
        return Err(AppError::InvalidOption(
            "Task discovery resource IDs must be positive".into(),
        ));
    }
    Ok(id)
}
fn parse_enum<T: DeserializeOwned>(value: &str, field: &str) -> Result<T, AppError> {
    from_value(Value::String(value.into()))
        .map_err(|error| AppError::InvalidOption(format!("Invalid {field}: {error}")))
}

pub(crate) const TASK_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "class_id",
        "class_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "object_id",
        "object_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "collection_id",
        "collection_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "class_relation",
        "class_relation",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "object_relation",
        "object_relation",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "schema_revision",
        "schema_revision",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "computation_revision",
        "computation_revision",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "schema_work_kind",
        "schema_work_kind",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "schema_work_status",
        "schema_work_status",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "remote_target_id",
        "remote_target_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "remote_side_effect_state",
        "remote_side_effect_state",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "export_scope_kind",
        "export_scope_kind",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "export_template_id",
        "export_template_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "export_has_warnings",
        "export_has_warnings",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "export_truncated",
        "export_truncated",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "import_dry_run",
        "import_dry_run",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "import_atomicity",
        "import_atomicity",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "import_collision_policy",
        "import_collision_policy",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "import_permission_policy",
        "import_permission_policy",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "import_has_failed_items",
        "import_has_failed_items",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "backup_include_history",
        "backup_include_history",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "output_state",
        "output_state",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "terminal",
        "terminal",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "cancel_requested",
        "cancel_requested",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Boolean,
    ),
    FilterFieldSpec::new(
        "terminal_reason",
        "terminal_reason",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "trace_id",
        "trace_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "submitted_by",
        "submitted_by",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_after",
        "created_after",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "created_before",
        "created_before",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "started_after",
        "started_after",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "started_before",
        "started_before",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "finished_after",
        "finished_after",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "finished_before",
        "finished_before",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::DateTime,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list_query::PageSelection;
    use crate::services::{HubuumGateway, ListTasksInput};
    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{
        header::{HeaderName, HeaderValue},
        StatusCode,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn typed_discovery_preserves_filters_and_kind_status_sets_across_pages() {
        let transport = MockTransport::default();
        for next in [Some("next-page"), None] {
            let mut response = TransportResponse::json(StatusCode::OK, &json!([])).unwrap();
            if let Some(next) = next {
                response.headers.insert(
                    HeaderName::from_static("x-next-cursor"),
                    HeaderValue::from_static(next),
                );
            }
            transport.push_response(response);
        }
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let discovery = TaskDiscovery::parse(&[
            "submitted_by equals 7".into(),
            "output_state equals available".into(),
            "terminal equals true".into(),
        ])
        .unwrap();
        gateway
            .list_tasks(ListTasksInput {
                kind: Some("export,backup".into()),
                status: Some("succeeded,failed".into()),
                discovery,
                page_selection: PageSelection::All,
                ..ListTasksInput::default()
            })
            .unwrap();
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let query = request
                .url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(query["submitted_by"], "7");
            assert_eq!(query["output_state"], "available");
            assert_eq!(query["kind"], "export,backup");
            assert_eq!(query["status"], "succeeded,failed");
        }
        assert!(requests[1]
            .url
            .query()
            .unwrap()
            .contains("cursor=next-page"));
    }

    #[test]
    fn invalid_discovery_never_reaches_transport() {
        let transport = MockTransport::default();
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let discovery = TaskDiscovery::parse(&["schema_revision equals 2".into()]).unwrap();
        assert!(gateway
            .list_tasks(ListTasksInput {
                discovery,
                ..ListTasksInput::default()
            })
            .is_err());
        assert!(transport.requests().is_empty());
        for clause in [
            "class_id equals 0",
            "trace_id equals bad",
            "created_after equals yesterday",
            "output_state contains available",
        ] {
            assert!(TaskDiscovery::parse(&[clause.into()]).is_err(), "{clause}");
        }
    }
}
