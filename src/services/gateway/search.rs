use std::collections::{HashMap, HashSet};

use hubuum_client::{
    client::sync::UnifiedSearchRequest, Class, Collection, Object, UnifiedSearchBatchResponse,
    UnifiedSearchEvent, UnifiedSearchKind, UnifiedSearchNext, UnifiedSearchResults,
};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::domain::{
    ClassRecord, CollectionRecord, ResolvedObjectRecord, SearchBatchRecord, SearchCursorSet,
    SearchErrorEvent, SearchQueryEvent, SearchResponseRecord, SearchResultsRecord,
    SearchStreamEvent,
};
use crate::errors::AppError;

use super::{shared::find_entities_by_ids, HubuumGateway};

const MAX_AUTO_SEARCH_PAGES: usize = 10_000;
const MAX_AUTO_SEARCH_ITEMS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum SearchKind {
    Collection,
    Class,
    Object,
}

#[derive(Debug, Clone, Default)]
pub struct SearchInput {
    pub query: String,
    pub kinds: Vec<SearchKind>,
    pub limit_per_kind: Option<usize>,
    pub cursor_collections: Option<String>,
    pub cursor_classes: Option<String>,
    pub cursor_objects: Option<String>,
    pub search_class_schema: bool,
    pub search_object_data: bool,
}

impl HubuumGateway {
    pub fn search(&self, input: &SearchInput) -> Result<SearchResponseRecord, AppError> {
        let raw = self.build_search_request(input).send()?;
        Ok(SearchResponseRecord {
            query: raw.query,
            results: self.map_search_results(raw.results)?,
            next: raw.next.into(),
        })
    }

    pub fn search_all(&self, input: &SearchInput) -> Result<SearchResponseRecord, AppError> {
        let mut response = self.search(input)?;
        let mut next = response.next.clone();
        let mut seen_collections = seeded_cursors(input.cursor_collections.as_ref());
        let mut seen_classes = seeded_cursors(input.cursor_classes.as_ref());
        let mut seen_objects = seeded_cursors(input.cursor_objects.as_ref());
        record_search_cursor(
            &mut seen_collections,
            next.collections.as_ref(),
            "collections",
        )?;
        record_search_cursor(&mut seen_classes, next.classes.as_ref(), "classes")?;
        record_search_cursor(&mut seen_objects, next.objects.as_ref(), "objects")?;

        let mut pages = 1;
        let mut items = response.results.item_count();
        enforce_search_pagination_limits(pages, items)?;

        while !next.is_empty() {
            if pages >= MAX_AUTO_SEARCH_PAGES || items >= MAX_AUTO_SEARCH_ITEMS {
                return Err(search_pagination_limit_error());
            }

            let active_collections = next.collections.is_some();
            let active_classes = next.classes.is_some();
            let active_objects = next.objects.is_some();
            let page_input = SearchInput {
                kinds: active_search_kinds(active_collections, active_classes, active_objects),
                cursor_collections: next.collections.clone(),
                cursor_classes: next.classes.clone(),
                cursor_objects: next.objects.clone(),
                ..input.clone()
            };
            let page = self.search(&page_input)?;
            pages += 1;
            items += page.results.item_count();
            enforce_search_pagination_limits(pages, items)?;
            response.results.extend(page.results);

            next = SearchCursorSet {
                collections: active_collections
                    .then_some(page.next.collections)
                    .flatten(),
                classes: active_classes.then_some(page.next.classes).flatten(),
                objects: active_objects.then_some(page.next.objects).flatten(),
            };
            record_search_cursor(
                &mut seen_collections,
                next.collections.as_ref(),
                "collections",
            )?;
            record_search_cursor(&mut seen_classes, next.classes.as_ref(), "classes")?;
            record_search_cursor(&mut seen_objects, next.objects.as_ref(), "objects")?;
        }

        response.next = SearchCursorSet::default();
        Ok(response)
    }

    pub fn search_stream(&self, input: &SearchInput) -> Result<Vec<SearchStreamEvent>, AppError> {
        let mut mapped = Vec::new();

        for event in self.build_search_request(input).stream()? {
            match event? {
                UnifiedSearchEvent::Started(payload) => {
                    mapped.push(SearchStreamEvent::Started(SearchQueryEvent {
                        query: payload.query,
                    }))
                }
                UnifiedSearchEvent::Batch(batch) => {
                    mapped.push(SearchStreamEvent::Batch(self.map_search_batch(batch)?))
                }
                UnifiedSearchEvent::Done(payload) => {
                    mapped.push(SearchStreamEvent::Done(SearchQueryEvent {
                        query: payload.query,
                    }))
                }
                UnifiedSearchEvent::Error(payload) => {
                    mapped.push(SearchStreamEvent::Error(SearchErrorEvent {
                        message: payload.message,
                    }))
                }
                UnifiedSearchEvent::Unknown { .. } => {}
                _ => {}
            }
        }

        Ok(mapped)
    }

    fn build_search_request(&self, input: &SearchInput) -> UnifiedSearchRequest {
        let mut request = self.client.search(input.query.clone());

        if !input.kinds.is_empty() {
            request = request.kinds(input.kinds.iter().copied().map(Into::into));
        }
        if let Some(limit) = input.limit_per_kind {
            request = request.limit_per_kind(limit);
        }
        if let Some(cursor) = &input.cursor_collections {
            request = request.cursor_collections(cursor.clone());
        }
        if let Some(cursor) = &input.cursor_classes {
            request = request.cursor_classes(cursor.clone());
        }
        if let Some(cursor) = &input.cursor_objects {
            request = request.cursor_objects(cursor.clone());
        }
        if input.search_class_schema {
            request = request.search_class_schema(true);
        }
        if input.search_object_data {
            request = request.search_object_data(true);
        }

        request
    }

    fn map_search_results(
        &self,
        raw: UnifiedSearchResults,
    ) -> Result<SearchResultsRecord, AppError> {
        let objects = self.resolve_search_objects(&raw.objects, &raw.classes, &raw.collections)?;
        Ok(SearchResultsRecord {
            collections: raw
                .collections
                .into_iter()
                .map(CollectionRecord::from)
                .collect(),
            classes: raw.classes.into_iter().map(ClassRecord::from).collect(),
            objects,
        })
    }

    fn map_search_batch(
        &self,
        raw: UnifiedSearchBatchResponse,
    ) -> Result<SearchBatchRecord, AppError> {
        let objects = self.resolve_search_objects(&raw.objects, &raw.classes, &raw.collections)?;
        Ok(SearchBatchRecord {
            kind: raw.kind,
            collections: raw
                .collections
                .into_iter()
                .map(CollectionRecord::from)
                .collect(),
            classes: raw.classes.into_iter().map(ClassRecord::from).collect(),
            objects,
            next: raw.next,
        })
    }

    fn resolve_search_objects(
        &self,
        objects: &[Object],
        classes: &[Class],
        collections: &[Collection],
    ) -> Result<Vec<ResolvedObjectRecord>, AppError> {
        if objects.is_empty() {
            return Ok(Vec::new());
        }

        let mut class_map = classes
            .iter()
            .map(|class| (class.id.into(), class.clone()))
            .collect::<HashMap<_, _>>();
        let mut collection_map = collections
            .iter()
            .map(|collection| (collection.id.into(), collection.clone()))
            .collect::<HashMap<_, _>>();

        let missing_class_ids = objects
            .iter()
            .filter(|object| !class_map.contains_key(&object.hubuum_class_id.into()))
            .count();
        if missing_class_ids > 0 {
            class_map.extend(find_entities_by_ids(
                &self.client.classes(),
                objects.iter(),
                |object| object.hubuum_class_id,
            )?);
        }

        let missing_collection_ids = objects
            .iter()
            .filter(|object| !collection_map.contains_key(&object.collection_id.into()))
            .count();
        if missing_collection_ids > 0 {
            collection_map.extend(find_entities_by_ids(
                &self.client.collections(),
                objects.iter(),
                |object| object.collection_id,
            )?);
        }

        Ok(objects
            .iter()
            .map(|object| ResolvedObjectRecord::new(object, &class_map, &collection_map))
            .collect())
    }
}

fn active_search_kinds(collections: bool, classes: bool, objects: bool) -> Vec<SearchKind> {
    [
        collections.then_some(SearchKind::Collection),
        classes.then_some(SearchKind::Class),
        objects.then_some(SearchKind::Object),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn seeded_cursors(cursor: Option<&String>) -> HashSet<String> {
    cursor.cloned().into_iter().collect()
}

fn record_search_cursor(
    seen: &mut HashSet<String>,
    cursor: Option<&String>,
    kind: &str,
) -> Result<(), AppError> {
    if let Some(cursor) = cursor {
        if !seen.insert(cursor.clone()) {
            return Err(AppError::CommandExecutionError(format!(
                "Automatic search pagination received repeated {kind} cursor '{cursor}'"
            )));
        }
    }
    Ok(())
}

fn enforce_search_pagination_limits(pages: usize, items: usize) -> Result<(), AppError> {
    if pages > MAX_AUTO_SEARCH_PAGES || items > MAX_AUTO_SEARCH_ITEMS {
        return Err(search_pagination_limit_error());
    }
    Ok(())
}

fn search_pagination_limit_error() -> AppError {
    AppError::CommandExecutionError(format!(
        "Automatic search pagination exceeded its safety limit of {MAX_AUTO_SEARCH_PAGES} pages or {MAX_AUTO_SEARCH_ITEMS} items"
    ))
}

impl From<SearchKind> for UnifiedSearchKind {
    fn from(value: SearchKind) -> Self {
        match value {
            SearchKind::Collection => UnifiedSearchKind::Collection,
            SearchKind::Class => UnifiedSearchKind::Class,
            SearchKind::Object => UnifiedSearchKind::Object,
        }
    }
}

impl From<UnifiedSearchNext> for SearchCursorSet {
    fn from(value: UnifiedSearchNext) -> Self {
        Self {
            collections: value.collections,
            classes: value.classes,
            objects: value.objects,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{
        blocking::Client, MockTransport, Token, TransportResponse, UnifiedSearchKind,
    };
    use reqwest::StatusCode;
    use serde_json::json;

    use super::{HubuumGateway, SearchInput, SearchKind};

    #[test]
    fn search_kind_maps_to_client_search_kind() {
        assert_eq!(
            UnifiedSearchKind::from(SearchKind::Collection),
            UnifiedSearchKind::Collection
        );
        assert_eq!(
            UnifiedSearchKind::from(SearchKind::Class),
            UnifiedSearchKind::Class
        );
        assert_eq!(
            UnifiedSearchKind::from(SearchKind::Object),
            UnifiedSearchKind::Object
        );
    }

    #[test]
    fn search_all_follows_each_active_cursor() {
        let transport = MockTransport::default();
        transport.push_response(search_response(1, "First", Some("page-2")));
        transport.push_response(search_response(2, "Second", None));
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let response = gateway
            .search_all(&SearchInput {
                query: "needle".to_string(),
                kinds: vec![SearchKind::Collection],
                limit_per_kind: Some(1),
                ..SearchInput::default()
            })
            .expect("all search pages should load");

        assert_eq!(
            response
                .results
                .collections
                .iter()
                .map(|collection| collection.0.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert!(response.next.is_empty());
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        let second_query = requests[1]
            .url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        assert!(second_query.contains(&("cursor_collections".to_string(), "page-2".to_string())));
        assert!(second_query.contains(&("kinds".to_string(), "collection".to_string())));
    }

    #[test]
    fn search_all_rejects_repeated_cursors() {
        let transport = MockTransport::default();
        transport.push_response(search_response(1, "First", Some("page-2")));
        transport.push_response(search_response(2, "Second", Some("page-2")));
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let error = gateway
            .search_all(&SearchInput {
                query: "needle".to_string(),
                kinds: vec![SearchKind::Collection],
                ..SearchInput::default()
            })
            .expect_err("repeated cursors should fail");

        assert!(error.to_string().contains("repeated collections cursor"));
    }

    fn search_response(id: i32, name: &str, next: Option<&str>) -> TransportResponse {
        TransportResponse::json(
            StatusCode::OK,
            &json!({
                "query": "needle",
                "results": {
                    "collections": [{
                        "id": id,
                        "name": name,
                        "description": "",
                        "parent_collection_id": null,
                        "created_at": "2026-07-25T12:00:00Z",
                        "updated_at": "2026-07-25T12:00:00Z"
                    }],
                    "classes": [],
                    "objects": []
                },
                "next": {
                    "collections": next,
                    "classes": null,
                    "objects": null
                }
            }),
        )
        .expect("search response should serialize")
    }
}
