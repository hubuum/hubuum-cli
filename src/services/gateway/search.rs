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

impl SearchInput {
    fn for_next_page(&self, cursors: &SearchCursorSet) -> Self {
        let mut kinds = Vec::with_capacity(3);
        if cursors.collections.is_some() {
            kinds.push(SearchKind::Collection);
        }
        if cursors.classes.is_some() {
            kinds.push(SearchKind::Class);
        }
        if cursors.objects.is_some() {
            kinds.push(SearchKind::Object);
        }

        Self {
            kinds,
            cursor_collections: cursors.collections.clone(),
            cursor_classes: cursors.classes.clone(),
            cursor_objects: cursors.objects.clone(),
            ..self.clone()
        }
    }
}

#[derive(Debug)]
struct SearchCursorTracker {
    collections: HashSet<String>,
    classes: HashSet<String>,
    objects: HashSet<String>,
}

impl SearchCursorTracker {
    fn new(input: &SearchInput) -> Self {
        Self {
            collections: input.cursor_collections.iter().cloned().collect(),
            classes: input.cursor_classes.iter().cloned().collect(),
            objects: input.cursor_objects.iter().cloned().collect(),
        }
    }

    fn record(&mut self, cursors: &SearchCursorSet) -> Result<(), AppError> {
        Self::record_cursor(
            &mut self.collections,
            cursors.collections.as_ref(),
            "collections",
        )?;
        Self::record_cursor(&mut self.classes, cursors.classes.as_ref(), "classes")?;
        Self::record_cursor(&mut self.objects, cursors.objects.as_ref(), "objects")
    }

    fn record_cursor(
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
}

#[derive(Debug)]
struct SearchPaginationGuard {
    pages: usize,
    items: usize,
}

impl SearchPaginationGuard {
    fn new(initial_items: usize) -> Result<Self, AppError> {
        let guard = Self {
            pages: 1,
            items: initial_items,
        };
        guard.enforce_limits()?;
        Ok(guard)
    }

    fn ensure_can_fetch_next(&self) -> Result<(), AppError> {
        if self.pages >= MAX_AUTO_SEARCH_PAGES || self.items >= MAX_AUTO_SEARCH_ITEMS {
            return Err(search_pagination_limit_error());
        }
        Ok(())
    }

    fn record_page(&mut self, item_count: usize) -> Result<(), AppError> {
        self.pages = self
            .pages
            .checked_add(1)
            .ok_or_else(search_pagination_limit_error)?;
        self.items = self
            .items
            .checked_add(item_count)
            .ok_or_else(search_pagination_limit_error)?;
        self.enforce_limits()
    }

    fn enforce_limits(&self) -> Result<(), AppError> {
        if self.pages > MAX_AUTO_SEARCH_PAGES || self.items > MAX_AUTO_SEARCH_ITEMS {
            return Err(search_pagination_limit_error());
        }
        Ok(())
    }
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
        let mut cursor_tracker = SearchCursorTracker::new(input);
        cursor_tracker.record(&next)?;
        let mut pagination_guard = SearchPaginationGuard::new(response.results.item_count())?;

        while !next.is_empty() {
            pagination_guard.ensure_can_fetch_next()?;
            let active = next;
            let page_input = input.for_next_page(&active);
            let page = self.search(&page_input)?;
            pagination_guard.record_page(page.results.item_count())?;
            response.results.extend(page.results);

            next = page.next;
            next.retain_active(&active);
            cursor_tracker.record(&next)?;
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

    use super::{
        HubuumGateway, SearchInput, SearchKind, SearchPaginationGuard, MAX_AUTO_SEARCH_ITEMS,
    };
    use crate::domain::SearchCursorSet;

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
    fn next_page_input_and_cursors_keep_only_active_kinds() {
        let input = SearchInput {
            query: "needle".to_string(),
            kinds: vec![
                SearchKind::Collection,
                SearchKind::Class,
                SearchKind::Object,
            ],
            ..SearchInput::default()
        };
        let active = SearchCursorSet {
            collections: None,
            classes: Some("class-page-2".to_string()),
            objects: Some("object-page-2".to_string()),
        };

        let next_input = input.for_next_page(&active);

        assert_eq!(
            next_input.kinds,
            vec![SearchKind::Class, SearchKind::Object]
        );
        assert_eq!(next_input.cursor_classes.as_deref(), Some("class-page-2"));
        assert_eq!(next_input.cursor_objects.as_deref(), Some("object-page-2"));
        assert!(next_input.cursor_collections.is_none());

        let mut returned = SearchCursorSet {
            collections: Some("unexpected".to_string()),
            classes: Some("class-page-3".to_string()),
            objects: None,
        };
        returned.retain_active(&active);
        assert_eq!(
            returned,
            SearchCursorSet {
                collections: None,
                classes: Some("class-page-3".to_string()),
                objects: None,
            }
        );
    }

    #[test]
    fn search_pagination_guard_stops_before_fetching_past_item_limit() {
        let guard = SearchPaginationGuard::new(MAX_AUTO_SEARCH_ITEMS)
            .expect("the current page may reach the item limit");

        let error = guard
            .ensure_can_fetch_next()
            .expect_err("another page would exceed the safety limit");

        assert!(error.to_string().contains("safety limit"));
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
                        "revision": 1,
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
