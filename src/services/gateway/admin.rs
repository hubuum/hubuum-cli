use hubuum_client::{QueryObservationUpdateRequest, ResourceRevision};
use reqwest::{Method, StatusCode};
use serde_json::{json, to_value, Value};

use crate::diagnostics::client_call;
use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub fn query_observations(&self) -> Result<Value, AppError> {
        if self.runtime_settings_available()? {
            return self.runtime_settings_group("query_observations");
        }
        Ok(to_value(client_call("client.query_observations", || {
            self.client().query_observations()
        })?)?)
    }

    pub fn set_query_observations(&self, enabled: bool) -> Result<Value, AppError> {
        if self.runtime_settings_available()? {
            let state = self.runtime_settings_group("query_observations")?;
            let revision: ResourceRevision = serde_json::from_value(state["revision"].clone())?;
            return self.patch_runtime_settings(
                "query_observations",
                revision,
                json!({"enabled": enabled}),
            );
        }
        Ok(to_value(client_call(
            "client.set_query_observations",
            || {
                self.client()
                    .set_query_observations(QueryObservationUpdateRequest { enabled })
            },
        )?)?)
    }
    pub fn server_config(&self) -> Result<Value, AppError> {
        Ok(to_value(client_call("client.admin_config", || {
            self.client().admin_config()
        })?)?)
    }
}

// These additive routes are optional on the pinned server/client release. Keep
// them behind the client's constrained authenticated transport until released.
impl HubuumGateway {
    fn runtime_settings_available(&self) -> Result<bool, AppError> {
        match self.runtime_settings_schema() {
            Ok(catalogue) if catalogue["schema_version"] == 1 => Ok(true),
            Ok(_) => Err(AppError::InvalidOption(
                "This server uses an unsupported settings discovery version".into(),
            )),
            Err(AppError::ApiError(error)) if error.is_status(StatusCode::NOT_FOUND) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn runtime_settings_schema(&self) -> Result<Value, AppError> {
        Ok(client_call("client.runtime_settings_schema", || {
            self.client()
                .raw(Method::GET, "/api/v1/admin/runtime-settings/schema")
                .send()
        })?)
    }

    pub fn runtime_settings_group(&self, group: &str) -> Result<Value, AppError> {
        let path = runtime_group_path(group)?;
        Ok(client_call("client.runtime_settings_group", || {
            self.client().raw(Method::GET, &path).send()
        })?)
    }

    pub fn patch_runtime_settings(
        &self,
        group: &str,
        expected_revision: ResourceRevision,
        changes: Value,
    ) -> Result<Value, AppError> {
        let path = runtime_group_path(group)?;
        let fields = changes
            .as_object()
            .ok_or_else(|| AppError::InvalidOption("changes must be a JSON object".into()))?;
        if fields.is_empty()
            || fields.values().any(Value::is_null)
            || fields.contains_key("expected_revision")
        {
            return Err(AppError::InvalidOption(
                "changes must contain concrete setting values, without revision metadata".into(),
            ));
        }
        let catalogue = self.runtime_settings_schema()?;
        if catalogue["schema_version"] != 1 {
            return Err(AppError::InvalidOption(
                "This server uses an unsupported settings discovery version".into(),
            ));
        }
        let descriptor = catalogue["groups"]
            .as_array()
            .and_then(|groups| groups.iter().find(|item| item["key"] == group))
            .ok_or_else(|| {
                AppError::InvalidOption("The server does not advertise this settings group".into())
            })?;
        if descriptor["patch_supported"] != true {
            return Err(AppError::InvalidOption(
                descriptor["read_only_reason"]
                    .as_str()
                    .unwrap_or("This group requires a dedicated update workflow")
                    .into(),
            ));
        }
        Ok(client_call("client.patch_runtime_settings", || {
            self.client().raw(Method::PATCH, &path)
                .json(&serde_json::json!({"expected_revision": expected_revision, "changes": changes}))?.send()
        })?)
    }

    pub fn release_runtime_settings(
        &self,
        group: &str,
        expected_revision: ResourceRevision,
    ) -> Result<Value, AppError> {
        let path = runtime_group_path(group)?;
        let body = client_call("client.release_runtime_settings", || {
            self.client()
                .raw(Method::DELETE, &path)
                .query_param("expected_revision", expected_revision)
                .send_text()
        })?;
        Ok(serde_json::from_str(&body)?)
    }
}

fn runtime_group_path(group: &str) -> Result<String, AppError> {
    if group.is_empty()
        || !group
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'_')
    {
        return Err(AppError::InvalidOption(
            "A settings group must be a discovered key containing letters, digits, or underscores"
                .into(),
        ));
    }
    Ok(format!("/api/v1/admin/runtime-settings/{group}"))
}
