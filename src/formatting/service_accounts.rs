use crate::domain::ServiceAccountRecord;
use crate::formatting::{DetailRenderable, TableRenderable};

impl TableRenderable for ServiceAccountRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Owner Group",
            "Disabled",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        let sa = &self.0;
        vec![
            sa.id.to_string(),
            sa.name.clone(),
            sa.description.clone().unwrap_or_default(),
            sa.owner_group_id.to_string(),
            sa.disabled_at
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "no".to_string()),
            sa.created_at.to_string(),
            sa.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ServiceAccountRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let sa = &self.0;
        vec![
            ("ID", sa.id.to_string()),
            ("Name", sa.name.clone()),
            (
                "Description",
                sa.description
                    .clone()
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            ("Owner Group ID", sa.owner_group_id.to_string()),
            (
                "Created By",
                sa.created_by
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            (
                "Disabled At",
                sa.disabled_at
                    .as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "<never>".to_string()),
            ),
            ("Created", sa.created_at.to_string()),
            ("Updated", sa.updated_at.to_string()),
        ]
    }
}
