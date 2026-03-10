use crate::domain::ImportResultRecord;

use super::TableRenderable;

impl TableRenderable for ImportResultRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Task",
            "Entity",
            "Action",
            "Identifier",
            "Outcome",
            "Error",
            "Created",
        ]
    }

    fn row(&self) -> Vec<String> {
        let result = &self.0;
        vec![
            result.id.to_string(),
            result.task_id.to_string(),
            result.entity_kind.clone(),
            result.action.clone(),
            result.identifier.clone().unwrap_or_default(),
            result.outcome.clone(),
            result.error.clone().unwrap_or_default(),
            result.created_at.to_string(),
        ]
    }
}
