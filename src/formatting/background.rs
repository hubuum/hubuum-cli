use crate::background::BackgroundJobRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for BackgroundJobRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Local ID", self.id.to_string()),
            ("Task ID", self.task_id.to_string()),
            ("Label", self.label.clone()),
            ("State", self.state.clone()),
            ("Status", self.status.clone()),
            ("Summary", self.summary.clone().unwrap_or_default()),
            ("Created", self.created_at.clone().unwrap_or_default()),
            ("Started", self.started_at.clone().unwrap_or_default()),
            ("Finished", self.finished_at.clone().unwrap_or_default()),
            ("Last Error", self.last_error.clone().unwrap_or_default()),
        ]
    }
}

impl TableRenderable for BackgroundJobRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "task", "label", "state", "status", "error"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.task_id.to_string(),
            self.label.clone(),
            self.state.clone(),
            self.status.clone(),
            self.last_error.clone().unwrap_or_default(),
        ]
    }
}
