use std::collections::HashSet;

mod comparisons;

use hubuum_client::types::SortDirection;
use hubuum_client::{
    client::sync::HistoryRequest, HistoryId, HistoryPageOptions, HistoryTimelineOptions,
    HubuumDateTime, ObjectHistoryOptions, RelationHistoryOptions,
};
use serde::{de::DeserializeOwned, Serialize};

use super::shared::parse_single_sort;
use super::{HubuumGateway, ObjectScope, ResourceScope};
use crate::diagnostics::client_call;
use crate::domain::{HistoryComparison, HistoryPageRecord, HistoryRecord, HistoryTime, JsonRecord};
use crate::errors::AppError;
use crate::list_query::{PageSelection, PagedResult, ServerPageSize};

pub enum HistoryInput {
    At {
        at: HistoryTime,
        include_total: bool,
    },
    List(HistoryListInput),
}

#[derive(Clone, Default)]
pub struct HistoryListInput {
    limit: Option<ServerPageSize>,
    sort: Option<(String, SortDirection)>,
    cursor: Option<String>,
    include_total: bool,
    page_selection: PageSelection,
}

impl HistoryListInput {
    pub fn limit(mut self, limit: Option<ServerPageSize>) -> Self {
        self.limit = limit;
        self
    }
    pub fn sort(mut self, sort: Option<&str>) -> Result<Self, AppError> {
        self.sort =
            parse_single_sort(sort)?.map(|(field, direction)| (field.to_owned(), direction));
        Ok(self)
    }
    pub fn cursor(mut self, cursor: Option<String>) -> Self {
        self.cursor = cursor;
        self
    }
    pub fn include_total(mut self, include: bool) -> Self {
        self.include_total = include;
        self
    }
    pub fn page_selection(mut self, selection: PageSelection) -> Self {
        self.page_selection = selection;
        self
    }
}

pub enum HistoricalQuery {
    Timeline(HistoryTimelineOptions),
    TimelineWithChanges {
        options: HistoryTimelineOptions,
        comparison: HistoryComparison,
    },
    ObjectName {
        name: String,
        options: ObjectHistoryOptions,
    },
    Objects {
        at: HubuumDateTime,
        options: ObjectHistoryOptions,
    },
    Relations {
        at: Option<HubuumDateTime>,
        options: RelationHistoryOptions,
    },
}

impl HubuumGateway {
    pub fn history(
        &self,
        scope: ResourceScope,
        input: HistoryInput,
    ) -> Result<PagedResult<HistoryRecord>, AppError> {
        let input = match input {
            HistoryInput::At { at, include_total } => {
                if at.end().is_some() {
                    return self.history_in_window(scope, at, include_total);
                }
                let record = self.history_record_at(scope, at.start())?;
                return Ok(PagedResult {
                    items: vec![HistoryRecord::new(record)?],
                    next_cursor: None,
                    returned_count: 1,
                    total_count: include_total.then_some(1),
                });
            }
            HistoryInput::List(input) => input,
        };
        let (class, object) = self.resolve_resource_scope(scope)?;
        match object {
            Some(object) => history_results(self.client().object_history(class, object), &input),
            None => history_results(self.client().class_history(class), &input),
        }
    }

    fn history_in_window(
        &self,
        scope: ResourceScope,
        at: HistoryTime,
        include_total: bool,
    ) -> Result<PagedResult<HistoryRecord>, AppError> {
        // Resolve live names once so every entry belongs to the same identity.
        let (class, object) = self.resolve_resource_scope(scope)?;
        let scope = ResourceScope::class(None, Some(class))?.with_object(None, object)?;
        let markers = self
            .historical_view(
                scope.clone(),
                HistoricalQuery::Timeline(
                    HistoryTimelineOptions::default().window(Some(at.start()), at.end())?,
                ),
                HistoryPageOptions::default().limit(250)?,
                PageSelection::All,
            )?
            .rows()?;
        let ids = markers
            .items
            .iter()
            .map(HistoryRecord::history_id)
            .collect::<Result<Vec<_>, _>>()?;
        let mut snapshots = self.history_snapshots(scope, ids.iter().copied().collect())?;
        let items = ids
            .into_iter()
            .map(|id| {
                snapshots.remove(&id).ok_or_else(|| {
                    AppError::InvalidOption(format!(
                        "History entry {id} became unavailable; restart the query"
                    ))
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let count = items.len();
        Ok(PagedResult {
            items: HistoryRecord::with_changes(items),
            next_cursor: None,
            returned_count: count,
            total_count: include_total.then_some(count as u64),
        })
    }

    pub fn history_record_by_id(
        &self,
        scope: ResourceScope,
        id: HistoryId,
    ) -> Result<JsonRecord, AppError> {
        if id.get() <= 0 {
            return Err(AppError::InvalidOption(
                "History ID must be positive".into(),
            ));
        }
        let (class, object) = self.resolve_resource_scope(scope)?;
        Ok(match object {
            Some(object) => {
                JsonRecord::from_serializable(client_call("client.object_history_entry", || {
                    self.client().object_history_entry(class, object, id)
                })?)?
            }
            None => {
                JsonRecord::from_serializable(client_call("client.class_history_entry", || {
                    self.client().class_history_entry(class, id)
                })?)?
            }
        })
    }

    pub fn history_record_at(
        &self,
        scope: ResourceScope,
        at: HubuumDateTime,
    ) -> Result<JsonRecord, AppError> {
        let (class, object) = self.resolve_resource_scope(scope)?;
        Ok(match object {
            Some(object) => {
                JsonRecord::from_serializable(client_call("client.object_history_as_of", || {
                    self.client().object_history_as_of(class, object, at)
                })?)?
            }
            None => {
                JsonRecord::from_serializable(client_call("client.class_history_as_of", || {
                    self.client().class_history_as_of(class, at)
                })?)?
            }
        })
    }

    pub fn historical_computed(
        &self,
        scope: ObjectScope,
        at: HubuumDateTime,
    ) -> Result<JsonRecord, AppError> {
        let (class, object) = self.resolve_object_scope(scope)?;
        Ok(JsonRecord::from_serializable(client_call(
            "client.object_computed_as_of",
            || self.client().object_computed_as_of(class, object, at),
        )?)?)
    }

    pub fn historical_view(
        &self,
        scope: ResourceScope,
        query: HistoricalQuery,
        page: HistoryPageOptions,
        selection: PageSelection,
    ) -> Result<HistoryPageRecord, AppError> {
        let (class, object) = self.resolve_resource_scope(scope)?;
        let result = collect_history_pages(page, selection, |page| match &query {
            HistoricalQuery::Timeline(options)
            | HistoricalQuery::TimelineWithChanges { options, .. } => match object {
                Some(object) => HistoryPageRecord::records(client_call(
                    "client.object_history_timeline",
                    || {
                        self.client().object_history_timeline(
                            class,
                            object,
                            options.clone().page(page),
                        )
                    },
                )?),
                None => HistoryPageRecord::records(client_call(
                    "client.class_history_timeline",
                    || {
                        self.client()
                            .class_history_timeline(class, options.clone().page(page))
                    },
                )?),
            },
            HistoricalQuery::ObjectName { name, options } => {
                HistoryPageRecord::records(client_call("client.object_history_by_name", || {
                    self.client().object_history_by_name(
                        class,
                        name.clone(),
                        options.clone().page(page),
                    )
                })?)
            }
            HistoricalQuery::Objects { at, options } => HistoryPageRecord::envelope(
                client_call("client.class_objects_as_of", || {
                    self.client()
                        .class_objects_as_of(class, at.clone(), options.clone().page(page))
                })?,
                "objects",
            ),
            HistoricalQuery::Relations { at, options } => {
                let options = options.clone().page(page);
                let response = match (object, at) {
                    (Some(object), Some(at)) => {
                        client_call("client.object_relations_as_of", || {
                            self.client()
                                .object_relations_as_of(class, object, at.clone(), options)
                        })
                    }
                    (Some(object), None) => client_call("client.object_relation_history", || {
                        self.client()
                            .object_relation_history(class, object, options)
                    }),
                    (None, Some(at)) => client_call("client.class_relations_as_of", || {
                        self.client()
                            .class_relations_as_of(class, at.clone(), options)
                    }),
                    (None, None) => client_call("client.class_relation_history", || {
                        self.client().class_relation_history(class, options)
                    }),
                }?;
                HistoryPageRecord::envelope(response, "relations")
            }
        })?;
        if let HistoricalQuery::TimelineWithChanges { comparison, .. } = query {
            let scope = ResourceScope::class(None, Some(class))?.with_object(None, object)?;
            self.timeline_changes(scope, result, comparison)
        } else {
            Ok(result)
        }
    }
}

fn collect_history_pages(
    mut options: HistoryPageOptions,
    selection: PageSelection,
    mut fetch: impl FnMut(HistoryPageOptions) -> Result<HistoryPageRecord, AppError>,
) -> Result<HistoryPageRecord, AppError> {
    const MAX_PAGES: usize = 1_000;
    const MAX_ITEMS: usize = 100_000;
    let mut result = fetch(options.clone())?;
    if !matches!(selection, PageSelection::All) {
        return Ok(result);
    }
    let mut seen = HashSet::new();
    let mut pages = 1;
    while let Some(cursor) = result.next_cursor().map(str::to_owned) {
        if !seen.insert(cursor.clone()) {
            return Err(AppError::InvalidOption(
                "History pagination returned a repeated cursor; restart the query".into(),
            ));
        }
        if pages >= MAX_PAGES || result.len() >= MAX_ITEMS {
            return Err(AppError::InvalidOption(
                "History --all exceeds the 1,000-page or 100,000-record limit; narrow the query"
                    .into(),
            ));
        }
        options = options.cursor(cursor);
        let next = fetch(options.clone())?;
        if result.len() + next.len() > MAX_ITEMS {
            return Err(AppError::InvalidOption(
                "History --all exceeds the 100,000-record limit; narrow the query".into(),
            ));
        }
        result.append(next)?;
        pages += 1;
    }
    Ok(result)
}

pub(crate) fn parse_timestamp(value: &str) -> Result<HubuumDateTime, AppError> {
    Ok(HistoryTime::parse(value)?.start())
}

fn history_results<T: DeserializeOwned + Serialize>(
    mut request: HistoryRequest<T>,
    input: &HistoryListInput,
) -> Result<PagedResult<HistoryRecord>, AppError> {
    if let Some(limit) = input.limit {
        request = request.limit(limit.effective());
    }
    request = request.include_total(input.include_total);
    if let Some((field, direction)) = &input.sort {
        request = request.sort(field, *direction);
    }
    if let Some(cursor) = &input.cursor {
        request = request.cursor(cursor);
    }
    let page = if matches!(input.page_selection, PageSelection::All) {
        PagedResult::from_pages(request.pages())?
    } else {
        PagedResult::from_page(client_call("history.page", || request.page())?, |item| item)
    };
    Ok(PagedResult {
        items: HistoryRecord::with_changes(
            page.items
                .into_iter()
                .map(HistoryRecord::new)
                .collect::<Result<_, _>>()?,
        ),
        next_cursor: page.next_cursor,
        returned_count: page.returned_count,
        total_count: page.total_count,
    })
}
