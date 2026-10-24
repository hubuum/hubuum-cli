use hubuum_client::{
    ApiError, ClassId, QueryUsageAnalysisRequest, QueryUsageCreateRequest, QueryUsageDeclarationId,
    QueryUsagePattern, QueryUsageReplaceRequest, ResourceRevision,
};
use reqwest::StatusCode;

use super::HubuumGateway;
use crate::diagnostics::client_call;
use crate::domain::QueryUsageOutput;
use crate::errors::AppError;

pub enum QueryUsageCommand {
    List,
    Create(QueryUsagePattern),
    Replace {
        id: QueryUsageDeclarationId,
        request: QueryUsageReplaceRequest,
    },
    Delete {
        id: QueryUsageDeclarationId,
        expected_revision: ResourceRevision,
    },
    Analyze(QueryUsageAnalysisRequest),
}

impl HubuumGateway {
    pub fn query_usage(
        &self,
        class: &str,
        command: QueryUsageCommand,
    ) -> Result<QueryUsageOutput, AppError> {
        let class = self.class_handle_by_name(class)?.id();
        self.query_usage_for_class(class, command)
    }

    fn query_usage_for_class(
        &self,
        class: ClassId,
        command: QueryUsageCommand,
    ) -> Result<QueryUsageOutput, AppError> {
        let usage = self.client().class_query_usage(class);
        let result: Result<_, ApiError> = (|| {
            Ok(match command {
                QueryUsageCommand::List => {
                    QueryUsageOutput::List(client_call("query_usage.list", || usage.list())?)
                }
                QueryUsageCommand::Create(pattern) => QueryUsageOutput::Declaration(Box::new(
                    client_call("query_usage.create", || {
                        usage.create(QueryUsageCreateRequest { pattern })
                    })?,
                )),
                QueryUsageCommand::Replace { id, request } => QueryUsageOutput::Declaration(
                    Box::new(client_call("query_usage.replace", || {
                        usage.replace(id, request)
                    })?),
                ),
                QueryUsageCommand::Delete {
                    id,
                    expected_revision,
                } => {
                    client_call("query_usage.delete", || usage.delete(id, expected_revision))?;
                    QueryUsageOutput::Deleted { deleted: id }
                }
                QueryUsageCommand::Analyze(request) => QueryUsageOutput::Analysis(Box::new(
                    client_call("query_usage.analyze", || usage.analyze(request))?,
                )),
            })
        })();
        result.map_err(|error| {
            if error.is_status(StatusCode::CONFLICT) {
                AppError::CommandExecutionError("Query-usage conflict: list the class declarations, review the current pattern and revision, and retry with the reviewed --expected-revision. Duplicate path/type declarations also conflict; no automatic retry was made.".into())
            } else { error.into() }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{
        blocking::Client, JsonPath, MockTransport, QueryUsageOperation, QueryUsageValueType, Token,
        TransportResponse,
    };
    use reqwest::Method;
    use serde_json::{from_slice, json, Value};

    use super::*;

    fn gateway(transport: &MockTransport) -> HubuumGateway {
        HubuumGateway::new(Arc::new(
            Client::builder_from_url("https://example.invalid")
                .unwrap()
                .with_transport(Arc::new(transport.clone()))
                .build()
                .unwrap()
                .authenticate(Token::new("secret")),
        ))
    }

    fn pattern() -> QueryUsagePattern {
        QueryUsagePattern::new(
            JsonPath::new(["cpu", "cores"]).unwrap(),
            QueryUsageValueType::Numeric,
            [QueryUsageOperation::Equals],
        )
        .unwrap()
    }

    fn declaration(revision: i64) -> Value {
        json!({"id":7,"class_id":42,"pattern":{"path":"cpu,cores","value_type":"numeric","operations":["equals"]},
            "revision":revision,"created_at":"2026-10-19T00:00:00Z","updated_at":"2026-10-19T00:00:00Z",
            "created_by":1,"updated_by":1,"schema_compatibility":"unknown"})
    }

    #[test]
    fn declaration_mutations_preserve_the_reviewed_revision_on_the_wire() {
        let transport = MockTransport::default();
        for response in [declaration(1), declaration(2)] {
            transport.push_response(TransportResponse::json(StatusCode::OK, &response).unwrap());
        }
        transport
            .push_response(TransportResponse::json(StatusCode::NO_CONTENT, &Value::Null).unwrap());
        let gateway = gateway(&transport);
        let id = QueryUsageDeclarationId::new(7).unwrap();
        gateway
            .query_usage_for_class(42.try_into().unwrap(), QueryUsageCommand::Create(pattern()))
            .unwrap();
        gateway
            .query_usage_for_class(
                42.try_into().unwrap(),
                QueryUsageCommand::Replace {
                    id,
                    request: QueryUsageReplaceRequest {
                        pattern: pattern(),
                        expected_revision: ResourceRevision::new(1).unwrap(),
                    },
                },
            )
            .unwrap();
        gateway
            .query_usage_for_class(
                42.try_into().unwrap(),
                QueryUsageCommand::Delete {
                    id,
                    expected_revision: ResourceRevision::new(2).unwrap(),
                },
            )
            .unwrap();
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, Method::POST);
        assert_eq!(requests[0].url.path(), "/api/v1/classes/42/query-usage");
        assert_eq!(requests[1].method, Method::PUT);
        let body: Value = from_slice(requests[1].body()).unwrap();
        assert_eq!(body["expected_revision"], 1);
        assert_eq!(body["pattern"]["path"], "cpu,cores");
        assert_eq!(requests[2].method, Method::DELETE);
        assert_eq!(requests[2].url.path(), "/api/v1/classes/42/query-usage/7");
        assert_eq!(requests[2].url.query(), Some("expected_revision=2"));
    }

    #[test]
    fn stale_revision_is_actionable_and_never_automatically_retried() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(StatusCode::CONFLICT, &json!({"message":"stale"})).unwrap(),
        );
        let error = gateway(&transport)
            .query_usage_for_class(
                42.try_into().unwrap(),
                QueryUsageCommand::Delete {
                    id: QueryUsageDeclarationId::new(7).unwrap(),
                    expected_revision: ResourceRevision::INITIAL,
                },
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("review") && error.contains("--expected-revision"));
        assert_eq!(transport.requests().len(), 1);
    }
}
