use serde::{Deserialize, Serialize};

use super::{ClassRecord, CollectionRecord, ResolvedObjectRecord};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchCursorSet {
    pub collections: Option<String>,
    pub classes: Option<String>,
    pub objects: Option<String>,
}

impl SearchCursorSet {
    pub fn is_empty(&self) -> bool {
        self.collections.is_none() && self.classes.is_none() && self.objects.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResultsRecord {
    pub collections: Vec<CollectionRecord>,
    pub classes: Vec<ClassRecord>,
    pub objects: Vec<ResolvedObjectRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponseRecord {
    pub query: String,
    pub results: SearchResultsRecord,
    pub next: SearchCursorSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchBatchRecord {
    pub kind: String,
    pub collections: Vec<CollectionRecord>,
    pub classes: Vec<ClassRecord>,
    pub objects: Vec<ResolvedObjectRecord>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueryEvent {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchErrorEvent {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum SearchStreamEvent {
    Started(SearchQueryEvent),
    Batch(SearchBatchRecord),
    Done(SearchQueryEvent),
    Error(SearchErrorEvent),
}
