use crate::formatting::DetailRenderable;

// Auth secrets are write-only on the wire (server never returns them), so a
// fetched RemoteTarget carries no real secret values. Text output redacts
// defensively anyway for defense-in-depth and clearer UX.
transparent_record!(RemoteTargetRecord, hubuum_client::RemoteTarget);

impl DetailRenderable for RemoteTargetRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let subject_types = self.0.allowed_subject_types
            .iter()
            .map(|st| format!("{:?}", st))
            .collect::<Vec<_>>()
            .join(", ");

        let mut rows = vec![
            ("ID", self.0.id.to_string()),
            ("Name", self.0.name.clone()),
            ("Namespace ID", self.0.namespace_id.to_string()),
            ("Description", self.0.description.clone()),
            ("Method", format!("{:?}", self.0.method)),
            ("URL", self.0.url_template.clone()),
            ("Enabled", self.0.enabled.to_string()),
            ("Timeout (ms)", self.0.timeout_ms.to_string()),
            ("Allowed subject types", subject_types),
        ];

        if let Some(class_id) = self.0.class_id {
            rows.push(("Class ID", class_id.to_string()));
        }

        if let Some(ref headers) = self.0.headers_template {
            rows.push(("Headers template", serde_json::to_string(headers).unwrap_or_default()));
        }

        if let Some(ref body) = self.0.body_template {
            rows.push(("Body template", body.clone()));
        }

        let auth_display = match &self.0.auth_config {
            hubuum_client::RemoteAuthConfig::None => "None".to_string(),
            hubuum_client::RemoteAuthConfig::BearerSecret { .. } => {
                "Bearer <redacted>".to_string()
            }
            hubuum_client::RemoteAuthConfig::BasicSecret { username, .. } => {
                format!("Basic username={username}, secret <redacted>")
            }
            hubuum_client::RemoteAuthConfig::ApiKeySecret { header, .. } => {
                format!("ApiKey header={header}, secret <redacted>")
            }
        };
        rows.push(("Auth config", auth_display));
        rows.push(("Created at", self.0.created_at.to_string()));
        rows.push(("Updated at", self.0.updated_at.to_string()));

        rows
    }
}

impl crate::formatting::TableRenderable for RemoteTargetRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "ID",
            "Name",
            "Namespace ID",
            "Method",
            "URL",
            "Enabled",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.0.id.to_string(),
            self.0.name.clone(),
            self.0.namespace_id.to_string(),
            format!("{:?}", self.0.method),
            self.0.url_template.clone(),
            self.0.enabled.to_string(),
        ]
    }
}
