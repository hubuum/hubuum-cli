use std::collections::HashMap;

use hubuum_client::{Class, Namespace, Object, UnifiedSearchBatchResponse, UnifiedSearchKind};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::domain::{
    ClassRecord, NamespaceRecord, ResolvedObjectRecord, SearchBatchRecord, SearchErrorEvent,
    SearchQueryEvent, SearchResponseRecord, SearchResultsRecord, SearchStreamEvent,
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

impl HubuumGateway {
    pub fn search(&self, input: &SearchInput) -> Result<SearchResponseRecord, AppError> {
        let raw = self.build_search_request(input).execute()?;
        Ok(SearchResponseRecord {
            query: raw.query,
            results: self.map_search_results(raw.results)?,
            next: raw.next.into(),
        })
    }

    pub fn search_stream(&self, input: &SearchInput) -> Result<Vec<SearchStreamEvent>, AppError> {
        let mut mapped = Vec::new();

        for event in self.build_search_request(input).stream()? {
            match event {
                hubuum_client::UnifiedSearchEvent::Started(payload) => {
                    mapped.push(SearchStreamEvent::Started(SearchQueryEvent {
                        query: payload.query,
                    }))
                }
                hubuum_client::UnifiedSearchEvent::Batch(batch) => {
                    mapped.push(SearchStreamEvent::Batch(self.map_search_batch(batch)?))
                }
                hubuum_client::UnifiedSearchEvent::Done(payload) => {
                    mapped.push(SearchStreamEvent::Done(SearchQueryEvent {
                        query: payload.query,
                    }))
                }
                hubuum_client::UnifiedSearchEvent::Error(payload) => {
                    mapped.push(SearchStreamEvent::Error(SearchErrorEvent {
                        message: payload.message,
                    }))
                }
            }
        }

        Ok(mapped)
    }

    fn build_search_request(
        &self,
        input: &SearchInput,
    ) -> hubuum_client::client::sync::UnifiedSearchRequest {
        let mut request = self.client.search(input.query.clone());

        if !input.kinds.is_empty() {
            request = request.kinds(input.kinds.iter().copied().map(Into::into));
        }
        if let Some(limit) = input.limit_per_kind {
            request = request.limit_per_kind(limit);
        }
        if let Some(cursor) = &input.cursor_namespaces {
            request = request.cursor_namespaces(cursor.clone());
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
        raw: hubuum_client::UnifiedSearchResults,
    ) -> Result<SearchResultsRecord, AppError> {
        let objects = self.resolve_search_objects(&raw.objects, &raw.classes, &raw.namespaces)?;
        Ok(SearchResultsRecord {
            namespaces: raw
                .namespaces
                .into_iter()
                .map(NamespaceRecord::from)
                .collect(),
            classes: raw.classes.into_iter().map(ClassRecord::from).collect(),
            objects,
        })
    }

    fn map_search_batch(
        &self,
        raw: UnifiedSearchBatchResponse,
    ) -> Result<SearchBatchRecord, AppError> {
        let objects = self.resolve_search_objects(&raw.objects, &raw.classes, &raw.namespaces)?;
        Ok(SearchBatchRecord {
            kind: raw.kind,
            namespaces: raw
                .namespaces
                .into_iter()
                .map(NamespaceRecord::from)
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
            class_map.extend(find_entities_by_ids(
                &self.client.classes(),
                objects.iter(),
                |object| object.hubuum_class_id,
            )?);
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

impl From<SearchKind> for UnifiedSearchKind {
    fn from(value: SearchKind) -> Self {
        match value {
            SearchKind::Namespace => UnifiedSearchKind::Namespace,
            SearchKind::Class => UnifiedSearchKind::Class,
            SearchKind::Object => UnifiedSearchKind::Object,
        }
    }
}

impl From<hubuum_client::UnifiedSearchNext> for crate::domain::SearchCursorSet {
    fn from(value: hubuum_client::UnifiedSearchNext) -> Self {
        Self {
            namespaces: value.namespaces,
            classes: value.classes,
            objects: value.objects,
        }
    }
}

#[cfg(test)]
mod tests {
    use hubuum_client::UnifiedSearchKind;

    use super::SearchKind;

    #[test]
    fn search_kind_maps_to_client_search_kind() {
        assert_eq!(
            UnifiedSearchKind::from(SearchKind::Namespace),
            UnifiedSearchKind::Namespace
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
}
