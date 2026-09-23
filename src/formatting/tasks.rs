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
            ("Unattempted", task.unattempted_items.to_string()),
            (
                "Cancel Requested",
                display_or_empty(task.cancel_requested_at.as_ref()),
            ),
            (
                "Cancel Requested By",
                display_or_empty(task.cancel_requested_by),
            ),
            (
                "Cancel Reason",
                task.cancel_reason.clone().unwrap_or_default(),
            ),
            (
                "Execution Deadline",
                display_or_empty(task.execution_deadline_at.as_ref()),
            ),
            (
                "Terminal Reason",
                task.terminal_reason.clone().unwrap_or_default(),
            ),
            (
                "Remote Side Effects",
                task.remote_side_effect_state
                    .as_ref()
                    .map(|state| format!("{state:?}"))
                    .unwrap_or_default(),
            ),
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

        if let Some(details) = &task.details {
            if let Some(retained) = details
                .import_details
                .as_ref()
                .and_then(|d| d.retained.as_ref())
            {
                if let Some(value) = retained.dry_run {
                    rows.push(("Import Dry Run", value.to_string()));
                }
                if let Some(value) = &retained.atomicity {
                    rows.push(("Import Atomicity", value.to_string()));
                }
                if let Some(value) = &retained.collision_policy {
                    rows.push(("Import Collision Policy", value.to_string()));
                }
                if let Some(value) = &retained.permission_policy {
                    rows.push(("Import Permission Policy", value.to_string()));
                }
                if let Some(value) = retained.has_failed_items {
                    rows.push(("Import Has Failed Items", value.to_string()));
                }
            }
            if let Some(retained) = details.export.as_ref().and_then(|d| d.retained.as_ref()) {
                rows.push(("Output State", retained.output_state.to_string()));
                if let Some(value) = &retained.target {
                    rows.push((
                        "Target",
                        serde_json::to_string(value).expect("task target serializes"),
                    ));
                }
                if let Some(value) = retained.template_id {
                    rows.push(("Export Template", value.to_string()));
                }
                if let Some(value) = retained.scope_kind {
                    rows.push(("Export Scope", value.to_string()));
                }
                if let Some(value) = retained.warning_count {
                    rows.push(("Export Warnings", value.to_string()));
                }
                if let Some(value) = retained.truncated {
                    rows.push(("Export Truncated", value.to_string()));
                }
            }
            if let Some(retained) = details.backup.as_ref().and_then(|d| d.retained.as_ref()) {
                rows.push(("Output State", retained.output_state.to_string()));
                if let Some(value) = retained.include_history {
                    rows.push(("Backup Includes History", value.to_string()));
                }
            }
            if let Some(rebuild) = &details.reindex {
                if let Some(value) = rebuild.class_id {
                    rows.push(("Class ID", value.to_string()));
                }
                if let Some(value) = rebuild.computation_revision {
                    rows.push(("Computation Revision", value.to_string()));
                }
            }
            if let Some(remote) = &details.remote_call {
                if let Some(value) = remote.remote_target_id {
                    rows.push(("Remote Target ID", value.to_string()));
                }
                if let Some(value) = &remote.target {
                    rows.push((
                        "Target",
                        serde_json::to_string(value).expect("task target serializes"),
                    ));
                }
            }
            if let Some(schema) = &details.schema_validation {
                if let Some(value) = schema.class_id {
                    rows.push(("Class ID", value.to_string()));
                }
                if let Some(value) = schema.schema_revision {
                    rows.push(("Schema Revision", value.to_string()));
                }
                if let Some(value) = schema.work_kind {
                    rows.push(("Schema Work Kind", value.to_string()));
                }
                if let Some(value) = schema.work_status {
                    rows.push(("Schema Work Status", value.to_string()));
                }
                if let Some(value) = &schema.results_url {
                    rows.push(("Schema Results URL", value.clone()));
                }
            }
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
