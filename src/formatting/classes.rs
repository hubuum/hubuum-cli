use crate::domain::ClassRecord;

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ClassRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let class = &self.0;
        let schema = schema_label(class.json_schema.as_ref());

        vec![
            ("Name", class.name.clone()),
            ("Description", class.description.clone()),
            ("Namespace", class.namespace.name.clone()),
            ("Schema", schema),
            (
                "Validate",
                class
                    .validate_schema
                    .map_or_else(|| "<none>".to_string(), |value| value.to_string()),
            ),
            ("Created", class.created_at.to_string()),
            ("Updated", class.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ClassRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Namespace",
            "Schema",
            "Validate",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        let class = &self.0;
        vec![
            class.id.to_string(),
            class.name.clone(),
            class.description.clone(),
            class.namespace.name.clone(),
            schema_label(class.json_schema.as_ref()),
            class
                .validate_schema
                .map_or_else(|| "<none>".to_string(), |value| value.to_string()),
            class.created_at.to_string(),
            class.updated_at.to_string(),
        ]
    }
}

fn schema_label(schema: Option<&serde_json::Value>) -> String {
    let schema_id = schema
        .and_then(|value| value.as_object())
        .and_then(|value| value.get("$id"))
        .and_then(|value| value.as_str());

    match (schema, schema_id) {
        (_, Some(id)) => id.to_string(),
        (Some(_), None) => "<schema without $id>".to_string(),
        (None, _) => "<no schema>".to_string(),
    }
}
