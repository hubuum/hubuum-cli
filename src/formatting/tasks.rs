use crate::domain::{TaskEventRecord, TaskQueueStateRecord, TaskRecord};

use super::{core::display_or_empty, DetailRenderable, TableRenderable};

impl DetailRenderable for TaskRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let task = &self.0;
        let mut rows = vec![
            ("ID", task.id.to_string()),
            ("Kind", task.kind.to_string()),
            ("Status", task.status.to_string()),
            ("Submitted By", display_or_empty(task.submitted_by)),
            ("Summary", task.summary.clone().unwrap_or_default()),
            ("Created", task.created_at.to_string()),
            ("Started", display_or_empty(task.started_at.as_ref())),
            ("Finished", display_or_empty(task.finished_at.as_ref())),
            ("Total Items", task.progress.total_items.to_string()),
            ("Processed", task.progress.processed_items.to_string()),
            ("Succeeded", task.progress.success_items.to_string()),
            ("Failed", task.progress.failed_items.to_string()),
            ("Task URL", task.links.task.clone()),
            ("Events URL", task.links.events.clone()),
            (
                "Import URL",
                task.links.import_url.clone().unwrap_or_default(),
            ),
            (
                "Import Results",
                task.links.import_results.clone().unwrap_or_default(),
            ),
        ];

        if let Some(export) = task
            .details
            .as_ref()
            .and_then(|details| details.export.as_ref())
        {
            rows.extend([
                (
                    "Total Duration (ms)",
                    display_or_empty(export.total_duration_ms),
                ),
                (
                    "Query Duration (ms)",
                    display_or_empty(export.query_duration_ms),
                ),
                (
                    "Hydration Duration (ms)",
                    display_or_empty(export.hydration_duration_ms),
                ),
                (
                    "Render Duration (ms)",
                    display_or_empty(export.render_duration_ms),
                ),
            ]);
        }

        rows
    }
}

impl DetailRenderable for TaskQueueStateRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let state = &self.0;
        vec![
            ("Actix Workers", state.actix_workers.to_string()),
            ("Task Workers", state.configured_task_workers.to_string()),
            (
                "Poll Interval (ms)",
                state.task_poll_interval_ms.to_string(),
            ),
            ("Total Tasks", state.total_tasks.to_string()),
            ("Queued", state.queued_tasks.to_string()),
            ("Validating", state.validating_tasks.to_string()),
            ("Running", state.running_tasks.to_string()),
            ("Active", state.active_tasks.to_string()),
            ("Succeeded", state.succeeded_tasks.to_string()),
            ("Failed", state.failed_tasks.to_string()),
            (
                "Partially Succeeded",
                state.partially_succeeded_tasks.to_string(),
            ),
            ("Cancelled", state.cancelled_tasks.to_string()),
            ("Import Tasks", state.import_tasks.to_string()),
            ("Export Tasks", state.export_tasks.to_string()),
            ("Reindex Tasks", state.reindex_tasks.to_string()),
            ("Task Events", state.total_task_events.to_string()),
            (
                "Import Result Rows",
                state.total_import_result_rows.to_string(),
            ),
            (
                "Oldest Queued",
                state.oldest_queued_at.clone().unwrap_or_default(),
            ),
            (
                "Oldest Active",
                state.oldest_active_at.clone().unwrap_or_default(),
            ),
        ]
    }
}

impl TableRenderable for TaskRecord {
    fn headers() -> Vec<&'static str> {
        vec!["ID", "Kind", "Status", "Progress", "Summary"]
    }

    fn row(&self) -> Vec<String> {
        let task = &self.0;
        let progress = format!(
            "{}/{}",
            task.progress.processed_items, task.progress.total_items
        );
        vec![
            task.id.to_string(),
            task.kind.to_string(),
            task.status.to_string(),
            progress,
            task.summary.clone().unwrap_or_default(),
        ]
    }
}

impl TableRenderable for TaskEventRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Task", "Type", "Message", "Created"]
    }

    fn row(&self) -> Vec<String> {
        let event = &self.0;
        vec![
            event.id.to_string(),
            event.task_id.to_string(),
            event.event_type.clone(),
            event.message.clone(),
            event.created_at.to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use hubuum_client::TaskResponse;
    use serde_json::{from_value, json};

    use super::{DetailRenderable, TaskRecord};

    #[test]
    fn export_task_details_include_stage_timings() {
        let task = TaskRecord(
            from_value::<TaskResponse>(json!({
                "id": 5,
                "kind": "export",
                "status": "succeeded",
                "created_at": "2026-08-05T12:00:00Z",
                "progress": {
                    "total_items": 1,
                    "processed_items": 1,
                    "success_items": 1,
                    "failed_items": 0
                },
                "links": {
                    "task": "/api/v1/tasks/5",
                    "events": "/api/v1/tasks/5/events"
                },
                "details": {
                    "export": {
                        "output_url": "/api/v1/exports/5/output",
                        "output_available": true,
                        "output_expired": false,
                        "total_duration_ms": 12,
                        "query_duration_ms": 3,
                        "hydration_duration_ms": 4,
                        "render_duration_ms": 5
                    }
                }
            }))
            .expect("task fixture should deserialize"),
        );

        let rows = task.detail_rows();
        assert!(rows.contains(&("Total Duration (ms)", "12".to_string())));
        assert!(rows.contains(&("Query Duration (ms)", "3".to_string())));
        assert!(rows.contains(&("Hydration Duration (ms)", "4".to_string())));
        assert!(rows.contains(&("Render Duration (ms)", "5".to_string())));
    }
}
