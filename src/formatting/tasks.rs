use crate::domain::{TaskEventRecord, TaskQueueStateRecord, TaskRecord};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for TaskRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let task = &self.0;
        vec![
            ("ID", task.id.to_string()),
            ("Kind", task.kind.to_string()),
            ("Status", task.status.to_string()),
            (
                "Submitted By",
                task.submitted_by
                    .map_or_else(String::new, |value| value.to_string()),
            ),
            ("Summary", task.summary.clone().unwrap_or_default()),
            ("Created", task.created_at.to_string()),
            (
                "Started",
                task.started_at
                    .as_ref()
                    .map_or_else(String::new, |value| value.to_string()),
            ),
            (
                "Finished",
                task.finished_at
                    .as_ref()
                    .map_or_else(String::new, |value| value.to_string()),
            ),
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
        ]
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
            ("Report Tasks", state.report_tasks.to_string()),
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
