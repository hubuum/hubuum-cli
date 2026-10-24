use std::collections::{BTreeSet, HashMap};
use std::fmt;

use hubuum_client::{HistoryId, HistoryPage, Page};
use serde::Serialize;
use serde_json::{from_value, json, to_value, Map, Value};

mod comparison;
pub(crate) use comparison::{HistoryComparison, TimelineChanges};

use crate::errors::AppError;
use crate::list_query::PagedResult;

/// Retains the complete snapshot for JSON while giving tables history-specific columns.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct HistoryRecord {
    snapshot: Value,
    #[serde(skip)]
    changes: Option<String>,
}

impl HistoryRecord {
    pub(crate) fn new(value: impl Serialize) -> Result<Self, AppError> {
        Ok(Self {
            snapshot: to_value(value)?,
            changes: None,
        })
    }

    pub(crate) fn snapshot(&self) -> &Value {
        &self.snapshot
    }

    pub(crate) fn history_id(&self) -> Result<HistoryId, AppError> {
        Ok(from_value(self.snapshot["history_id"].clone())?)
    }

    pub(crate) fn is_creation(&self) -> bool {
        matches!(
            self.operation().as_str(),
            "i" | "insert" | "create" | "created"
        )
    }

    pub(crate) fn is_deleted(&self) -> bool {
        matches!(self.operation().as_str(), "d" | "delete" | "deleted")
    }

    fn operation(&self) -> String {
        self.snapshot["op"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
    }

    pub(crate) fn changes(&self) -> Option<&str> {
        self.changes.as_deref()
    }

    pub(crate) fn with_changes(mut records: Vec<Self>) -> Vec<Self> {
        let previous = records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                Some((
                    (
                        record.snapshot["id"].as_i64()?,
                        record.snapshot["revision"].as_i64()?,
                    ),
                    index,
                ))
            })
            .collect::<HashMap<_, _>>();
        for index in 0..records.len() {
            let snapshot = &records[index].snapshot;
            let op = snapshot["op"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let baseline = snapshot["id"]
                .as_i64()
                .zip(snapshot["revision"].as_i64())
                .and_then(|(id, revision)| previous.get(&(id, revision - 1)))
                .map(|index| &records[*index].snapshot);
            let changes = if matches!(op.as_str(), "d" | "delete" | "deleted") {
                "Deleted".into()
            } else if let Some(before) = baseline {
                let mut changes = Vec::new();
                compare_fields(
                    "",
                    Some(&snapshot_fields(before)),
                    Some(&snapshot_fields(snapshot)),
                    &mut changes,
                );
                if changes.is_empty() {
                    "No field changes".into()
                } else {
                    format_changes(changes)
                }
            } else if matches!(op.as_str(), "i" | "insert" | "create" | "created") {
                let mut changes = vec!["Created".into()];
                compare_fields(
                    "",
                    Some(&json!({})),
                    Some(&snapshot_fields(snapshot)),
                    &mut changes,
                );
                format_changes(changes)
            } else {
                "Previous revision unavailable in these results".into()
            };
            records[index].changes = Some(changes);
        }
        records
    }
}

fn snapshot_fields(snapshot: &Value) -> Value {
    let fields = [
        "name",
        "description",
        "collection_id",
        "hubuum_class_id",
        "data",
        "validate_schema",
        "json_schema",
    ];
    Value::Object(
        fields
            .into_iter()
            .filter_map(|key| {
                snapshot
                    .get(key)
                    .map(|value| (key.to_owned(), value.clone()))
            })
            .collect(),
    )
}

fn format_changes(mut changes: Vec<String>) -> String {
    if changes
        .iter()
        .any(|change| change.starts_with("+ ") || change.starts_with("- "))
    {
        for change in &mut changes {
            if change.starts_with('/') {
                change.insert_str(0, "  ");
            }
        }
    }
    changes.join("\n")
}

fn compare_fields(
    path: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    changes: &mut Vec<String>,
) {
    if before == after {
        return;
    }
    let empty = Map::new();
    let before_object = before
        .and_then(Value::as_object)
        .or_else(|| before.is_none().then_some(&empty));
    let after_object = after
        .and_then(Value::as_object)
        .or_else(|| after.is_none().then_some(&empty));
    if let (Some(before), Some(after)) = (before_object, after_object) {
        let keys = before.keys().chain(after.keys()).collect::<BTreeSet<_>>();
        for key in keys {
            let key_path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            compare_fields(&key_path, before.get(key), after.get(key), changes);
        }
        if !before.is_empty() || !after.is_empty() {
            return;
        }
    }
    changes.push(match (before, after) {
        (None, Some(after)) => format!("+ {path}: {after}"),
        (Some(before), None) => format!("- {path}: {before}"),
        (Some(before), Some(after)) => format!("{path}: {before} → {after}"),
        (None, None) => return,
    });
}

impl fmt::Debug for HistoryRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryRecord").finish_non_exhaustive()
    }
}

/// Historical envelopes must survive pagination and pipelines without losing context.
#[derive(Clone, Serialize)]
pub(crate) struct HistoryPageRecord {
    #[serde(flatten)]
    body: Value,
    next_cursor: Option<String>,
    total_count: Option<u64>,
    page_limit: Option<usize>,
    returned_count: usize,
    #[serde(skip)]
    row_key: &'static str,
}

impl HistoryPageRecord {
    pub(crate) fn with_timeline_changes(
        mut self,
        mut compare: impl FnMut(&HistoryRecord) -> Result<TimelineChanges, AppError>,
    ) -> Result<Self, AppError> {
        for row in self.body[self.row_key]
            .as_array_mut()
            .expect("validated rows")
        {
            row["changes"] = to_value(compare(&HistoryRecord::new(&*row)?)?)?;
        }
        Ok(self)
    }

    pub(crate) fn timeline_rows(&self) -> Result<Vec<HistoryTimelineRecord>, AppError> {
        Ok(self
            .rows()?
            .items
            .into_iter()
            .map(HistoryTimelineRecord)
            .collect())
    }

    pub(crate) fn records<T: Serialize>(page: Page<T>) -> Result<Self, AppError> {
        let returned_count = page.items.len();
        Ok(Self {
            body: json!({"items": page.items}),
            next_cursor: page.next_cursor,
            total_count: page.total_count,
            page_limit: page.page_limit,
            returned_count,
            row_key: "items",
        })
    }

    pub(crate) fn envelope<T: Serialize>(
        page: HistoryPage<T>,
        row_key: &'static str,
    ) -> Result<Self, AppError> {
        let body = to_value(page.data)?;
        let returned_count = body[row_key]
            .as_array()
            .ok_or_else(|| AppError::ParseError("Historical response is missing its rows".into()))?
            .len();
        Ok(Self {
            body,
            next_cursor: page.next_cursor,
            total_count: page.total_count,
            page_limit: page.page_limit,
            returned_count,
            row_key,
        })
    }

    pub(crate) fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
    pub(crate) fn len(&self) -> usize {
        self.returned_count
    }
    pub(crate) fn context(&self) -> Value {
        Value::Object(
            self.body
                .as_object()
                .expect("historical envelope")
                .iter()
                .filter(|(key, _)| key.as_str() != self.row_key)
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }

    pub(crate) fn relation_rows(&self) -> Option<Vec<HistoricalRelationRecord>> {
        (self.row_key == "relations").then(|| {
            self.body[self.row_key]
                .as_array()
                .expect("validated rows")
                .iter()
                .cloned()
                .map(|snapshot| {
                    HistoricalRelationRecord(HistoryRecord {
                        snapshot,
                        changes: None,
                    })
                })
                .collect()
        })
    }

    pub(crate) fn append(&mut self, mut next: Self) -> Result<(), AppError> {
        if self.row_key != next.row_key
            || self.context() != next.context()
            || self.total_count != next.total_count
            || self.page_limit != next.page_limit
        {
            return Err(AppError::InvalidOption(
                "Historical context changed between pages; restart the query".into(),
            ));
        }
        self.body[self.row_key]
            .as_array_mut()
            .expect("validated rows")
            .append(
                next.body[next.row_key]
                    .as_array_mut()
                    .expect("validated rows"),
            );
        self.returned_count += next.returned_count;
        self.next_cursor = next.next_cursor;
        Ok(())
    }

    pub(crate) fn rows(&self) -> Result<PagedResult<HistoryRecord>, AppError> {
        Ok(PagedResult {
            items: self.body[self.row_key]
                .as_array()
                .expect("validated rows")
                .iter()
                .map(HistoryRecord::new)
                .collect::<Result<_, _>>()?,
            next_cursor: self.next_cursor.clone(),
            returned_count: self.returned_count,
            total_count: self.total_count,
        })
    }
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct HistoricalRelationRecord(HistoryRecord);

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct HistoryTimelineRecord(HistoryRecord);

impl HistoryTimelineRecord {
    pub(crate) fn snapshot(&self) -> &Value {
        self.0.snapshot()
    }
}

impl fmt::Debug for HistoryPageRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryPageRecord")
            .field("returned_count", &self.returned_count)
            .finish_non_exhaustive()
    }
}

impl HistoricalRelationRecord {
    pub(crate) fn snapshot(&self) -> &Value {
        self.0.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_compare_adjacent_revisions_in_any_order_without_metadata_noise() {
        let before = json!({"id": 2, "revision": 1, "op": "insert", "name": "old", "valid_from": "old time",
            "data": {"memory": {"total": 580}, "gone": null, "keep": true}});
        let after = json!({"id": 2, "revision": 2, "op": "update", "name": "new", "valid_from": "new time",
            "data": {"memory": {"total": 581}, "new/key~": null, "keep": true}});
        let records = HistoryRecord::with_changes(vec![
            HistoryRecord::new(&after).unwrap(),
            HistoryRecord::new(&before).unwrap(),
        ]);
        let changes = records[0].changes().unwrap();
        assert!(changes.contains("  /name: \"old\" → \"new\""), "{changes}");
        assert!(
            changes.contains("  /data/memory/total: 580 → 581"),
            "{changes}"
        );
        assert!(changes.contains("- /data/gone: null"), "{changes}");
        assert!(changes.contains("+ /data/new~1key~0: null"), "{changes}");
        assert!(!changes.contains("valid_from"));
        assert!(!changes.contains("keep"));
        assert!(records[1].changes().unwrap().starts_with("Created\n"));
        assert_eq!(to_value(&records[0]).unwrap(), after);
    }

    #[test]
    fn changes_do_not_compare_across_gaps_or_resource_identities() {
        let records = HistoryRecord::with_changes(
            [
                json!({"id": 1, "revision": 1, "op": "update", "data": 1}),
                json!({"id": 1, "revision": 3, "op": "update", "data": 3}),
                json!({"id": 2, "revision": 2, "op": "update", "data": 2}),
                json!({"id": 1, "revision": 4, "op": "delete", "data": 3}),
            ]
            .into_iter()
            .map(|record| HistoryRecord::new(record).unwrap())
            .collect(),
        );
        for record in &records[..3] {
            assert_eq!(
                record.changes().unwrap(),
                "Previous revision unavailable in these results"
            );
        }
        assert_eq!(records[3].changes().unwrap(), "Deleted");
    }

    #[test]
    fn changes_distinguish_missing_null_empty_and_array_values() {
        let mut changes = Vec::new();
        compare_fields(
            "",
            Some(&json!({"empty": {}, "null": null, "list": [1, 2]})),
            Some(&json!({"list": [1, 3]})),
            &mut changes,
        );
        assert_eq!(
            changes,
            ["- /empty: {}", "/list: [1,2] → [1,3]", "- /null: null"]
        );
    }
}
