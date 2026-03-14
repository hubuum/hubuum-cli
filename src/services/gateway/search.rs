use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

use hubuum_client::{ApiError, Class, Namespace, Object};
use reqwest::blocking::Response;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::config::get_config;
use crate::domain::{
    ClassRecord, NamespaceRecord, ResolvedObjectRecord, SearchBatchRecord, SearchCursorSet,
    SearchResponseRecord, SearchResultsRecord, SearchStreamEvent,
};
use crate::errors::AppError;

use super::{shared::find_entities_by_ids, HubuumGateway};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum SearchKind {
    Namespace,
    Class,
    Object,
}

#[derive(Debug, Clone, Default)]
pub struct SearchInput {
    pub query: String,
    pub kinds: Vec<SearchKind>,
    pub limit_per_kind: Option<usize>,
    pub cursor_namespaces: Option<String>,
    pub cursor_classes: Option<String>,
    pub cursor_objects: Option<String>,
    pub search_class_schema: bool,
    pub search_object_data: bool,
}

#[derive(Debug, Deserialize)]
struct RawSearchResponse {
    query: String,
    results: RawSearchResults,
    next: SearchCursorSet,
}

#[derive(Debug, Deserialize)]
struct RawSearchResults {
    namespaces: Vec<Namespace>,
    classes: Vec<Class>,
    objects: Vec<Object>,
}

#[derive(Debug, Deserialize)]
struct RawSearchBatch {
    kind: String,
    namespaces: Vec<Namespace>,
    classes: Vec<Class>,
    objects: Vec<Object>,
    next: Option<String>,
}

#[derive(Debug)]
struct RawSseEvent {
    event: String,
    data: String,
}

impl HubuumGateway {
    pub fn search(&self, input: &SearchInput) -> Result<SearchResponseRecord, AppError> {
        let response = self.search_request("search", input)?;
        let raw: RawSearchResponse = response
            .json()
            .map_err(|error| AppError::HttpError(error.to_string()))?;
        Ok(SearchResponseRecord {
            query: raw.query,
            results: self.map_search_results(raw.results)?,
            next: raw.next,
        })
    }

    pub fn search_stream(&self, input: &SearchInput) -> Result<Vec<SearchStreamEvent>, AppError> {
        let response = self.search_request("search/stream", input)?;
        let events = parse_sse_events(response)?;
        let mut mapped = Vec::new();

        for event in events {
            match event.event.as_str() {
                "started" => mapped.push(SearchStreamEvent::Started(serde_json::from_str(
                    &event.data,
                )?)),
                "done" => mapped.push(SearchStreamEvent::Done(serde_json::from_str(&event.data)?)),
                "error" => {
                    mapped.push(SearchStreamEvent::Error(serde_json::from_str(&event.data)?))
                }
                "batch" => {
                    let raw_batch: RawSearchBatch = serde_json::from_str(&event.data)?;
                    mapped.push(SearchStreamEvent::Batch(self.map_search_batch(raw_batch)?));
                }
                _ => {}
            }
        }

        Ok(mapped)
    }

    fn search_request(
        &self,
        path_suffix: &str,
        input: &SearchInput,
    ) -> Result<Response, AppError> {
        let config = get_config();
        let base_url = format!(
            "{}://{}:{}/api/v1/{path_suffix}",
            config.server.protocol, config.server.hostname, config.server.port
        );
        let mut url =
            reqwest::Url::parse(&base_url).map_err(|error| AppError::HttpError(error.to_string()))?;

        let mut query = vec![("q", input.query.clone())];
        if !input.kinds.is_empty() {
            query.push((
                "kinds",
                input
                    .kinds
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if let Some(limit) = input.limit_per_kind {
            query.push(("limit_per_kind", limit.to_string()));
        }
        if let Some(cursor) = &input.cursor_namespaces {
            query.push(("cursor_namespaces", cursor.clone()));
        }
        if let Some(cursor) = &input.cursor_classes {
            query.push(("cursor_classes", cursor.clone()));
        }
        if let Some(cursor) = &input.cursor_objects {
            query.push(("cursor_objects", cursor.clone()));
        }
        if input.search_class_schema {
            query.push(("search_class_schema", "true".to_string()));
        }
        if input.search_object_data {
            query.push(("search_object_data", "true".to_string()));
        }

        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in &query {
                pairs.append_pair(key, value);
            }
        }

        let response = self
            .client
            .http_client
            .get(url)
            .bearer_auth(self.client.get_token())
            .send()
            .map_err(|error| AppError::HttpError(error.to_string()))?;

        ensure_success(response)
    }

    fn map_search_results(&self, raw: RawSearchResults) -> Result<SearchResultsRecord, AppError> {
        let objects = self.resolve_search_objects(&raw.objects, &raw.classes, &raw.namespaces)?;
        Ok(SearchResultsRecord {
            namespaces: raw.namespaces.into_iter().map(NamespaceRecord::from).collect(),
            classes: raw.classes.into_iter().map(ClassRecord::from).collect(),
            objects,
        })
    }

    fn map_search_batch(&self, raw: RawSearchBatch) -> Result<SearchBatchRecord, AppError> {
        let objects = self.resolve_search_objects(&raw.objects, &raw.classes, &raw.namespaces)?;
        Ok(SearchBatchRecord {
            kind: raw.kind,
            namespaces: raw.namespaces.into_iter().map(NamespaceRecord::from).collect(),
            classes: raw.classes.into_iter().map(ClassRecord::from).collect(),
            objects,
            next: raw.next,
        })
    }

    fn resolve_search_objects(
        &self,
        objects: &[Object],
        classes: &[Class],
        namespaces: &[Namespace],
    ) -> Result<Vec<ResolvedObjectRecord>, AppError> {
        if objects.is_empty() {
            return Ok(Vec::new());
        }

        let mut class_map = classes
            .iter()
            .map(|class| (class.id, class.clone()))
            .collect::<HashMap<_, _>>();
        let mut namespace_map = namespaces
            .iter()
            .map(|namespace| (namespace.id, namespace.clone()))
            .collect::<HashMap<_, _>>();

        let missing_class_ids = objects
            .iter()
            .filter(|object| !class_map.contains_key(&object.hubuum_class_id))
            .count();
        if missing_class_ids > 0 {
            class_map.extend(find_entities_by_ids(&self.client.classes(), objects.iter(), |object| {
                object.hubuum_class_id
            })?);
        }

        let missing_namespace_ids = objects
            .iter()
            .filter(|object| !namespace_map.contains_key(&object.namespace_id))
            .count();
        if missing_namespace_ids > 0 {
            namespace_map.extend(find_entities_by_ids(
                &self.client.namespaces(),
                objects.iter(),
                |object| object.namespace_id,
            )?);
        }

        Ok(objects
            .iter()
            .map(|object| ResolvedObjectRecord::new(object, &class_map, &namespace_map))
            .collect())
    }
}

fn ensure_success(response: Response) -> Result<Response, AppError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .text()
        .map_err(|error| AppError::HttpError(error.to_string()))?;
    let message = parse_error_message(status, &body);
    Err(AppError::ApiError(ApiError::HttpWithBody { status, message }))
}

fn parse_error_message(status: StatusCode, body: &str) -> String {
    #[derive(Deserialize)]
    struct ApiErrorBody {
        message: Option<String>,
        error: Option<String>,
        detail: Option<String>,
    }

    if let Ok(parsed) = serde_json::from_str::<ApiErrorBody>(body) {
        if let Some(message) = parsed.message.or(parsed.error).or(parsed.detail) {
            return message;
        }
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        status.to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_sse_events(response: Response) -> Result<Vec<RawSseEvent>, AppError> {
    parse_sse_reader(BufReader::new(response))
}

fn parse_sse_reader<R>(reader: R) -> Result<Vec<RawSseEvent>, AppError>
where
    R: BufRead + Read,
{
    let mut events = Vec::new();
    let mut event_name: Option<String> = None;
    let mut data_lines = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim_end_matches('\r');

        if line.is_empty() {
            flush_sse_event(&mut events, &mut event_name, &mut data_lines);
            continue;
        }

        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim().to_string());
            continue;
        }

        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    flush_sse_event(&mut events, &mut event_name, &mut data_lines);
    Ok(events)
}

fn flush_sse_event(
    events: &mut Vec<RawSseEvent>,
    event_name: &mut Option<String>,
    data_lines: &mut Vec<String>,
) {
    let Some(event) = event_name.take() else {
        data_lines.clear();
        return;
    };

    events.push(RawSseEvent {
        event,
        data: data_lines.join("\n"),
    });
    data_lines.clear();
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{parse_error_message, parse_sse_reader};
    use reqwest::StatusCode;

    #[test]
    fn parse_sse_reader_collects_named_events() {
        let input = Cursor::new(
            "event: started\ndata: {\"query\":\"server\"}\n\n\
             event: batch\ndata: {\"kind\":\"classes\"}\n\n\
             event: done\ndata: {\"query\":\"server\"}\n\n",
        );

        let events = parse_sse_reader(input).expect("SSE parsing should succeed");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event, "started");
        assert_eq!(events[1].event, "batch");
        assert_eq!(events[2].event, "done");
    }

    #[test]
    fn parse_error_message_prefers_structured_message() {
        let message = parse_error_message(
            StatusCode::BAD_REQUEST,
            r#"{"message":"q must not be empty"}"#,
        );
        assert_eq!(message, "q must not be empty");
    }
}
