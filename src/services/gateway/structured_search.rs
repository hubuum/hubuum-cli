use std::collections::HashSet;

use hubuum_client::{
    StructuredSearchDoneEvent, StructuredSearchEvent, StructuredSearchRequest,
    StructuredSearchResourceKind, StructuredSearchResponse, StructuredSearchResult,
    StructuredSearchStartedEvent,
};
use hubuum_search::{ResourceKind, SearchRequest};
use serde::Serialize;
use serde_json::{from_value, to_value, Value};

use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(crate) struct StructuredSearchPage(StructuredSearchResponse);

#[derive(Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub(crate) enum StructuredSearchStreamEvent {
    Started(StructuredSearchStartedEvent),
    Result(StructuredSearchResult),
    Done(StructuredSearchDoneEvent),
}

impl StructuredSearchPage {
    pub(crate) fn next(&self) -> Option<&str> {
        self.0.next.as_deref()
    }
    pub(crate) fn total(&self) -> Option<i64> {
        self.0.total
    }
    pub(crate) fn rows(&self) -> Result<Vec<Value>, AppError> {
        self.0.results.iter().map(result_row).collect()
    }
    pub(crate) fn kind(&self) -> Result<ResourceKind, AppError> {
        Ok(from_value(to_value(self.0.kind)?)?)
    }
}

pub(crate) fn result_row(result: &StructuredSearchResult) -> Result<Value, AppError> {
    let mut value = to_value(result)?;
    Ok(value["resource"].take())
}

fn typed_request(request: &SearchRequest) -> Result<StructuredSearchRequest, AppError> {
    // Keep the CLI grammar independent of transport; validate the wire contract
    // through the pinned client before either its JSON or SSE operation is sent.
    let typed: StructuredSearchRequest = from_value(to_value(request)?)?;
    typed.validate()?;
    Ok(typed)
}

fn validate_metadata(
    version: u8,
    kind: StructuredSearchResourceKind,
    expected: StructuredSearchResourceKind,
    next: Option<&str>,
    total: Option<i64>,
) -> Result<(), AppError> {
    if version != 1 || kind != expected || next == Some("") || total.is_some_and(|total| total < 0)
    {
        return Err(AppError::CommandExecutionError(
            "Structured search returned invalid metadata or a different resource kind".into(),
        ));
    }
    Ok(())
}

fn validate_result(
    result: &StructuredSearchResult,
    expected: StructuredSearchResourceKind,
) -> Result<(), AppError> {
    let kind = match result {
        StructuredSearchResult::Collection(_) => StructuredSearchResourceKind::Collection,
        StructuredSearchResult::Class(_) => StructuredSearchResourceKind::Class,
        StructuredSearchResult::Object(_) => StructuredSearchResourceKind::Object,
        StructuredSearchResult::AuditEvent(_) => StructuredSearchResourceKind::AuditEvent,
        StructuredSearchResult::User(_) => StructuredSearchResourceKind::User,
        StructuredSearchResult::Group(_) => StructuredSearchResourceKind::Group,
        StructuredSearchResult::ServiceAccount(_) => StructuredSearchResourceKind::ServiceAccount,
        _ => {
            return Err(AppError::CommandExecutionError(
                "Unsupported structured search result kind".into(),
            ))
        }
    };
    validate_metadata(1, kind, expected, None, None)
}

impl HubuumGateway {
    pub(crate) fn structured_search(
        &self,
        request: SearchRequest,
        all: bool,
    ) -> Result<StructuredSearchPage, AppError> {
        let expected = from_value(to_value(request.kind())?)?;
        let mut cursors: HashSet<String> =
            request.cursor().map(str::to_string).into_iter().collect();
        let mut request = typed_request(&request)?;
        let mut result: Option<StructuredSearchResponse> = None;
        for _ in 0..10_000 {
            let page = self.client().structured_search(request.clone()).send()?;
            validate_metadata(
                page.version,
                page.kind,
                expected,
                page.next.as_deref(),
                page.total,
            )?;
            for item in &page.results {
                validate_result(item, expected)?;
            }
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
            let Some(cursor) = combined.next.as_deref().filter(|_| all) else {
                return Ok(StructuredSearchPage(result.unwrap()));
            };
            if !cursors.insert(cursor.to_string()) {
                return Err(AppError::CommandExecutionError(
                    "Structured search returned a repeated pagination cursor".into(),
                ));
            }
            request = request.cursor(cursor);
        }
        Err(AppError::CommandExecutionError(
            "Automatic structured search exceeded 10000 pages".into(),
        ))
    }

    pub(crate) fn structured_search_stream(
        &self,
        request: SearchRequest,
        mut emit: impl FnMut(StructuredSearchStreamEvent) -> Result<(), AppError>,
    ) -> Result<StructuredSearchDoneEvent, AppError> {
        let expected = from_value(to_value(request.kind())?)?;
        let stream = self
            .client()
            .structured_search(typed_request(&request)?)
            .stream()?;
        let mut started = false;
        for event in stream {
            // Once the stream is open, errors must never trigger command replay.
            let event = event.map_err(|error| {
                AppError::CommandExecutionError(format!("Structured search stream failed: {error}"))
            })?;
            match event {
                StructuredSearchEvent::Started(payload) if !started => {
                    validate_metadata(payload.version, payload.kind, expected, None, None)?;
                    started = true;
                    emit(StructuredSearchStreamEvent::Started(payload))?;
                }
                StructuredSearchEvent::Result(result) if started => {
                    validate_result(&result, expected)?;
                    emit(StructuredSearchStreamEvent::Result(result))?;
                }
                StructuredSearchEvent::Done(done) if started => {
                    validate_metadata(
                        done.version,
                        done.kind,
                        expected,
                        done.next.as_deref(),
                        done.total,
                    )?;
                    emit(StructuredSearchStreamEvent::Done(done.clone()))?;
                    return Ok(done);
                }
                StructuredSearchEvent::Error(error) => {
                    return Err(AppError::CommandExecutionError(format!(
                        "Structured search stream failed: {}",
                        error.message
                    )))
                }
                StructuredSearchEvent::Unknown { .. } => continue,
                _ => {
                    return Err(AppError::CommandExecutionError(
                        "Structured search stream returned events out of order".into(),
                    ))
                }
            }
        }
        Err(AppError::CommandExecutionError(
            "Structured search stream ended before its done event; results are incomplete".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use hubuum_search::PageOptions;
    use reqwest::{Method, StatusCode};
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
            "version":1,"kind":"object","results":[{"kind":"object","resource":{"id":id,"name":format!("host-{id}"),"data":{"cores":8},"hubuum_class_id":42,"collection_id":1,"description":"fixture","created_at":"2026-10-19T00:00:00Z","updated_at":"2026-10-19T00:00:00Z","revision":1}}],"next":next,"total":2
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
        assert_eq!(result.rows().unwrap().len(), 2);
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
    fn stream_response(body: &str) -> TransportResponse {
        let mut response = TransportResponse::empty(StatusCode::OK);
        response.headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("text/event-stream"),
        );
        response.body = body.as_bytes().to_vec();
        response
    }

    #[test]
    fn stream_errors_truncation_and_invalid_metadata_never_replay_requests() {
        let started = "event: started\ndata: {\"version\":1,\"kind\":\"group\"}\n\n";
        for (tail, expected) in [
            ("", "incomplete"),
            ("event: error\ndata: {\"version\":1,\"kind\":\"group\",\"message\":\"work limit exceeded\"}\n\n", "work limit exceeded"),
            ("event: done\ndata: {\"version\":1,\"kind\":\"user\",\"page_limit\":1}\n\n", "different resource kind"),
            (started, "out of order"),
        ] {
            let transport = MockTransport::default();
            transport.push_response(stream_response(&format!("{started}{tail}")));
            let mut emitted = 0;
            let error = gateway(&transport).structured_search_stream(SearchRequest::new(ResourceKind::Group), |_| { emitted += 1; Ok(()) }).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(emitted, 1);
            assert_eq!(transport.requests().len(), 1);
            assert!(error.reauthentication_retry().is_none());
        }
    }

    #[test]
    fn stream_preserves_request_and_done_metadata_and_ignores_future_events() {
        let transport = MockTransport::default();
        transport.push_response(stream_response(concat!(
            "event: future\ndata: ignored\n\n",
            "event: started\ndata: {\"version\":1,\"kind\":\"group\"}\n\n",
            "event: done\ndata: {\"version\":1,\"kind\":\"group\",\"next\":\"next-page\",\"total\":3,\"page_limit\":1}\n\n"
        )));
        let request = SearchRequest::new(ResourceKind::Group)
            .with_predicate("name == 'staff'")
            .unwrap()
            .with_page(
                PageOptions::new()
                    .limit(1)
                    .cursor("reviewed-page")
                    .include_total(true),
            )
            .unwrap();
        let mut events = Vec::new();
        let done = gateway(&transport)
            .structured_search_stream(request, |event| {
                events.push(to_value(event)?);
                Ok(())
            })
            .unwrap();
        assert_eq!(done.next.as_deref(), Some("next-page"));
        assert_eq!(done.total, Some(3));
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["event"], "done");
        let requests = transport.requests();
        assert_eq!(requests[0].method, Method::POST);
        assert_eq!(requests[0].url.path(), "/api/v1/search/stream");
        let body: Value = from_slice(requests[0].body()).unwrap();
        assert_eq!(body["filter"]["predicate"]["value"], "staff");
        assert_eq!(body["cursor"], "reviewed-page");
        assert_eq!(body["include_total"], true);
        assert_eq!(body["limit"], 1);
    }
}
