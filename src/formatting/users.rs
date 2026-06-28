use crate::domain::UserRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for UserRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let user = &self.0;
        vec![
            ("Name", user.name.clone()),
            (
                "Email",
                user.email.clone().unwrap_or_else(|| "<none>".to_string()),
            ),
            ("Created", user.created_at.to_string()),
            ("Updated", user.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for UserRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Name", "Email", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let user = &self.0;
        vec![
            user.id.to_string(),
            user.name.clone(),
            user.email.clone().unwrap_or_default(),
            user.created_at.to_string(),
            user.updated_at.to_string(),
        ]
    }
}
