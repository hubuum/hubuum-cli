use hubuum_client::HistoryId;
use serde::Serialize;
use serde_json::json;

use super::{compare_fields, format_changes, snapshot_fields, HistoryRecord};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HistoryComparison {
    Previous,
    Latest,
}

/// The reference ID makes comparisons across page boundaries and retention gaps explicit.
#[derive(Serialize)]
pub(crate) struct TimelineChanges {
    mode: HistoryComparison,
    against_history_id: Option<HistoryId>,
    summary: String,
}

impl TimelineChanges {
    pub(crate) fn new(
        mode: HistoryComparison,
        against_history_id: Option<HistoryId>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            mode,
            against_history_id,
            summary: summary.into(),
        }
    }
}

impl HistoryComparison {
    pub(crate) fn summarize(
        self,
        entry: &HistoryRecord,
        reference: Option<&HistoryRecord>,
    ) -> String {
        let (before, after) = match self {
            Self::Previous => {
                if entry.is_deleted() {
                    return "Deleted".into();
                }
                if reference.is_none() && !entry.is_creation() {
                    return "Previous retained entry unavailable".into();
                }
                (reference, entry)
            }
            Self::Latest => {
                let Some(latest) = reference else {
                    return "Latest retained entry unavailable".into();
                };
                if entry.snapshot()["history_id"] == latest.snapshot()["history_id"] {
                    return "Latest retained entry".into();
                }
                if latest.is_deleted() {
                    return if entry.is_deleted() {
                        "Both entries are deleted"
                    } else {
                        "Deleted in latest retained entry"
                    }
                    .into();
                }
                (Some(entry), latest)
            }
        };
        let mut changes = Vec::new();
        if before.is_none() {
            changes.push("Created".into());
        }
        if before.is_some_and(HistoryRecord::is_deleted) {
            changes.push("Restored".into());
        }
        let before = before
            .filter(|record| !record.is_deleted())
            .map(|record| snapshot_fields(record.snapshot()))
            .unwrap_or_else(|| json!({}));
        compare_fields(
            "",
            Some(&before),
            Some(&snapshot_fields(after.snapshot())),
            &mut changes,
        );
        if changes.is_empty() {
            "No field changes".into()
        } else {
            format_changes(changes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i64, op: &str, data: serde_json::Value) -> HistoryRecord {
        HistoryRecord::new(json!({"history_id": id, "op": op, "data": data})).unwrap()
    }

    #[test]
    fn previous_and_latest_comparisons_have_explicit_value_directions() {
        let old = record(1, "insert", json!({"memory": 580, "gone": null}));
        let new = record(2, "update", json!({"memory": 581, "added": true}));
        let expected = "+ /data/added: true\n- /data/gone: null\n  /data/memory: 580 → 581";
        assert_eq!(
            HistoryComparison::Previous.summarize(&new, Some(&old)),
            expected
        );
        assert_eq!(
            HistoryComparison::Latest.summarize(&old, Some(&new)),
            expected
        );
        assert_eq!(
            HistoryComparison::Latest.summarize(&new, Some(&new)),
            "Latest retained entry"
        );
        let unchanged = record(3, "update", new.snapshot()["data"].clone());
        assert_eq!(
            HistoryComparison::Previous.summarize(&unchanged, Some(&new)),
            "No field changes"
        );
    }

    #[test]
    fn creation_missing_baselines_deletion_and_restoration_are_distinct() {
        let created = record(1, "insert", json!({"memory": 580}));
        let deleted = record(2, "delete", json!({"memory": 580}));
        let restored = record(3, "update", json!({"memory": 581}));
        assert_eq!(
            HistoryComparison::Previous.summarize(&created, None),
            "Created\n+ /data/memory: 580"
        );
        assert_eq!(
            HistoryComparison::Previous.summarize(&restored, None),
            "Previous retained entry unavailable"
        );
        assert_eq!(
            HistoryComparison::Latest.summarize(&created, None),
            "Latest retained entry unavailable"
        );
        assert_eq!(
            HistoryComparison::Previous.summarize(&deleted, Some(&created)),
            "Deleted"
        );
        assert_eq!(
            HistoryComparison::Latest.summarize(&created, Some(&deleted)),
            "Deleted in latest retained entry"
        );
        assert_eq!(
            HistoryComparison::Latest.summarize(&deleted, Some(&restored)),
            "Restored\n+ /data/memory: 581"
        );
    }
}
