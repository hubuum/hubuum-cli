use std::collections::HashSet;

use hubuum_search::{PageOptions, ResourceKind, SearchRequest};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StructuredSearchPage {
    version: u8,
    kind: ResourceKind,
    results: Vec<SearchResult>,
    next: Option<String>,
    total: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    kind: ResourceKind,
    resource: Value,
}

impl StructuredSearchPage {
    pub(crate) fn next(&self) -> Option<&str> {
        self.next.as_deref()
    }
    pub(crate) fn total(&self) -> Option<u64> {
        self.total
    }
    pub(crate) fn rows(&self) -> Vec<Value> {
        self.results
            .iter()
            .map(|result| result.resource.clone())
            .collect()
    }
    pub(crate) fn kind(&self) -> ResourceKind {
        self.kind
    }

    fn validate(&self, request: &SearchRequest) -> Result<(), AppError> {
        if self.version != 1
            || self.kind != request.kind()
            || self
                .results
                .iter()
                .any(|result| result.kind != self.kind || !result.resource.is_object())
            || self.next.as_ref().is_some_and(String::is_empty)
        {
            return Err(AppError::CommandExecutionError(
                "Structured search returned an invalid response or a different resource kind"
                    .into(),
            ));
        }
        Ok(())
    }
}

impl HubuumGateway {
    pub(crate) fn structured_search(
        &self,
        mut request: SearchRequest,
        all: bool,
    ) -> Result<StructuredSearchPage, AppError> {
        let client = self.client();
        let mut cursors: HashSet<String> =
            request.cursor().map(str::to_string).into_iter().collect();
        let mut result: Option<StructuredSearchPage> = None;
        for _ in 0..10_000 {
            let page: StructuredSearchPage = client
                .raw(Method::POST, "/api/v1/search")
                .json(&request)?
                .send()?;
            page.validate(&request)?;
            if let Some(combined) = &mut result {
                if combined.results.len().saturating_add(page.results.len()) > 1_000_000 {
                    return Err(AppError::CommandExecutionError(
                        "Automatic structured search exceeded one million results".into(),
                    ));
                }
                combined.results.extend(page.results);
                combined.next = page.next;
            } else {
                result = Some(page);
            }
            let combined = result.as_ref().expect("first search page was assigned");
            let Some(cursor) = combined.next().filter(|_| all) else {
                return Ok(result.unwrap());
            };
            if !cursors.insert(cursor.to_string()) {
                return Err(AppError::CommandExecutionError(
                    "Structured search returned a repeated pagination cursor".into(),
                ));
            }
            request = request
                .with_page(PageOptions::new().cursor(cursor))
                .map_err(|error| AppError::InvalidOption(error.to_string()))?;
        }
        Err(AppError::CommandExecutionError(
            "Automatic structured search exceeded 10000 pages".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::StatusCode;
    use serde_json::{from_slice, json};
    use std::sync::Arc;

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

    fn page(transport: &MockTransport, id: i32, next: Option<&str>) {
        transport.push_response(TransportResponse::json(StatusCode::OK, &json!({
            "version":1,"kind":"object","results":[{"kind":"object","resource":{"id":id,"name":format!("host-{id}"),"data":{"cores":8}}}],"next":next,"total":2
        })).unwrap());
    }

    #[test]
    fn structured_pagination_preserves_the_request_and_combines_rows() {
        let transport = MockTransport::default();
        page(&transport, 1, Some("page-two"));
        page(&transport, 2, None);
        let request = SearchRequest::new(ResourceKind::Object)
            .in_class("Hosts")
            .unwrap()
            .with_predicate("data.cores >= 8")
            .unwrap()
            .with_page(PageOptions::new().limit(1).include_total(true))
            .unwrap();
        let result = gateway(&transport)
            .structured_search(request, true)
            .unwrap();
        assert_eq!(result.rows().len(), 2);
        assert_eq!(result.total(), Some(2));
        assert!(result.next().is_none());
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert_eq!(request.method, Method::POST);
            assert_eq!(request.url.path(), "/api/v1/search");
            let body: Value = from_slice(request.body()).unwrap();
            assert_eq!(body["target"]["class"]["name"], "Hosts");
            assert_eq!(body["filter"]["predicate"]["value"], 8);
            assert_eq!(body["limit"], 1);
            assert_eq!(body["include_total"], true);
        }
        assert_eq!(
            from_slice::<Value>(requests[1].body()).unwrap()["cursor"],
            "page-two"
        );
    }

    #[test]
    fn rejects_repeated_cursors_and_mismatched_response_kinds() {
        let transport = MockTransport::default();
        page(&transport, 1, Some("loop"));
        page(&transport, 2, Some("loop"));
        assert!(gateway(&transport)
            .structured_search(SearchRequest::new(ResourceKind::Object), true)
            .unwrap_err()
            .to_string()
            .contains("repeated"));
        let transport = MockTransport::default();
        page(&transport, 1, None);
        assert!(gateway(&transport)
            .structured_search(SearchRequest::new(ResourceKind::User), false)
            .is_err());
    }
}
