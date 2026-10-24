use std::collections::{HashMap, HashSet};

use hubuum_client::{
    client::sync::HistoryRequest, types::SortDirection, HistoryId, HistoryPageOptions,
    HistoryTimelineOptions, HubuumDateTime,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::from_value;

use super::{HistoricalQuery, HubuumGateway, ResourceScope};
use crate::diagnostics::client_page;
use crate::domain::{HistoryComparison, HistoryPageRecord, HistoryRecord, TimelineChanges};
use crate::errors::AppError;
use crate::list_query::PageSelection;

impl HubuumGateway {
    pub(super) fn timeline_changes(
        &self,
        scope: ResourceScope,
        page: HistoryPageRecord,
        mode: HistoryComparison,
    ) -> Result<HistoryPageRecord, AppError> {
        let rows = page.rows()?.items;
        if rows.is_empty() {
            return Ok(page);
        }
        let mut positions = rows
            .iter()
            .map(|row| {
                Ok((
                    from_value::<HubuumDateTime>(row.snapshot()["valid_from"].clone())?.0,
                    row.history_id()?,
                ))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        positions.sort_unstable();
        positions.dedup();
        let oldest = positions[0].1;
        let options = match mode {
            HistoryComparison::Previous
                if rows.iter().any(|row| {
                    row.snapshot()["history_id"] == oldest.get()
                        && row.is_creation()
                        && row.snapshot()["revision"] == 1
                }) =>
            {
                None
            }
            HistoryComparison::Previous => Some(HistoryTimelineOptions::default().anchor(oldest)?),
            HistoryComparison::Latest => Some(HistoryTimelineOptions::default()),
        };
        // Only the boundary reference needs a marker lookup. All other previous
        // entries are adjacent in the contiguous, time-ordered timeline selection.
        let reference = options
            .map(|options| self.timeline_reference(scope.clone(), options))
            .transpose()?
            .flatten();
        let mut previous = positions
            .windows(2)
            .map(|pair| (pair[1].1, Some(pair[0].1)))
            .collect::<HashMap<_, _>>();
        previous.insert(oldest, reference);
        let mut wanted = positions.iter().map(|(_, id)| *id).collect::<HashSet<_>>();
        wanted.extend(reference);
        let snapshots = self.history_snapshots(scope, wanted)?;
        page.with_timeline_changes(|marker| {
            let id = marker.history_id()?;
            let against = match mode {
                HistoryComparison::Previous => previous.get(&id).copied().flatten(),
                HistoryComparison::Latest => reference,
            };
            let summary = match snapshots.get(&id) {
                Some(entry) => mode.summarize(entry, against.and_then(|id| snapshots.get(&id))),
                None => "Snapshot unavailable".into(),
            };
            Ok(TimelineChanges::new(mode, against, summary))
        })
    }

    fn timeline_reference(
        &self,
        scope: ResourceScope,
        options: HistoryTimelineOptions,
    ) -> Result<Option<HistoryId>, AppError> {
        let mut page = HistoryPageOptions::default().limit(1)?;
        let mut cursors = HashSet::new();
        for _ in 0..1_000 {
            let result = self.historical_view(
                scope.clone(),
                HistoricalQuery::Timeline(options.clone()),
                page.clone(),
                PageSelection::Single,
            )?;
            if let Some(record) = result.rows()?.items.first() {
                return Ok(Some(record.history_id()?));
            }
            let Some(cursor) = result.next_cursor() else {
                return Ok(None);
            };
            if !cursors.insert(cursor.to_owned()) {
                return Err(AppError::InvalidOption(
                    "History reference pagination returned a repeated cursor; restart the query"
                        .into(),
                ));
            }
            page = page.cursor(cursor);
        }
        Err(AppError::InvalidOption(
            "History reference exceeds the 1,000-page scan limit".into(),
        ))
    }

    pub(super) fn history_snapshots(
        &self,
        scope: ResourceScope,
        wanted: HashSet<HistoryId>,
    ) -> Result<HashMap<HistoryId, HistoryRecord>, AppError> {
        if wanted.is_empty() {
            return Ok(HashMap::new());
        }
        let (class, object) = self.resolve_resource_scope(scope)?;
        match object {
            Some(object) => collect_snapshots(self.client().object_history(class, object), wanted),
            None => collect_snapshots(self.client().class_history(class), wanted),
        }
    }
}

fn collect_snapshots<T: DeserializeOwned + Serialize>(
    request: HistoryRequest<T>,
    mut wanted: HashSet<HistoryId>,
) -> Result<HashMap<HistoryId, HistoryRecord>, AppError> {
    let mut result = HashMap::new();
    let mut cursors = HashSet::new();
    let mut count = 0;
    let mut pages = request
        .sort("history_id", SortDirection::Desc)
        .limit(250)
        .pages();
    let mut page_count = 0;
    while let Some(page) = client_page(&mut pages) {
        let page = page?;
        page_count += 1;
        count += page.items.len();
        if count > 100_000 {
            return Err(AppError::InvalidOption(
                "History comparison exceeds the 100,000-snapshot scan limit".into(),
            ));
        }
        for item in page.items {
            let record = HistoryRecord::new(item)?;
            let id = record.history_id()?;
            if wanted.remove(&id) {
                result.insert(id, record);
            }
        }
        if wanted.is_empty() || page.next_cursor.is_none() {
            return Ok(result);
        }
        if !cursors.insert(page.next_cursor.expect("checked cursor")) {
            return Err(AppError::InvalidOption(
                "History snapshot pagination returned a repeated cursor; restart the query".into(),
            ));
        }
        if page_count >= 1_000 {
            return Err(AppError::InvalidOption(
                "History comparison exceeds the 1,000-page scan limit".into(),
            ));
        }
    }
    Ok(result)
}
