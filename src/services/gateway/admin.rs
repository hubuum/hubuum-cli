use hubuum_client::{ResourceRevision, RuntimeSettingsPatch};
use serde_json::{json, to_value, Value};

use crate::diagnostics::client_call;
use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub fn query_observations(&self) -> Result<Value, AppError> {
        self.require_runtime_settings()?;
        self.runtime_settings_group("query_observations")
    }

    pub fn set_query_observations(&self, enabled: bool) -> Result<Value, AppError> {
        self.require_runtime_settings()?;
        let state = client_call("client.runtime_settings_group", || {
            self.client().runtime_settings_group("query_observations")
        })?;
        let revision = state.revision.ok_or_else(|| {
            AppError::InvalidOption("This replica has not loaded a settings revision yet".into())
        })?;
        self.patch_runtime_settings("query_observations", revision, json!({"enabled": enabled}))
    }

    pub fn server_config(&self) -> Result<Value, AppError> {
        Ok(to_value(client_call("client.admin_config", || {
            self.client().admin_config()
        })?)?)
    }

    fn require_runtime_settings(&self) -> Result<(), AppError> {
        let catalogue = self.runtime_settings_schema()?;
        if catalogue["schema_version"] != 1 {
            return Err(AppError::InvalidOption(
                "This server uses an unsupported settings discovery version".into(),
            ));
        }
        Ok(())
    }

    pub fn runtime_settings_schema(&self) -> Result<Value, AppError> {
        Ok(to_value(client_call(
            "client.runtime_settings_schema",
            || self.client().runtime_settings_schema(),
        )?)?)
    }

    pub fn runtime_settings_group(&self, group: &str) -> Result<Value, AppError> {
        Ok(to_value(client_call(
            "client.runtime_settings_group",
            || self.client().runtime_settings_group(group),
        )?)?)
    }

    pub fn patch_runtime_settings(
        &self,
        group: &str,
        expected_revision: ResourceRevision,
        changes: Value,
    ) -> Result<Value, AppError> {
        let fields = changes
            .as_object()
            .ok_or_else(|| AppError::InvalidOption("changes must be a JSON object".into()))?;
        let patch = RuntimeSettingsPatch::new(expected_revision, fields.clone()).map_err(|_| {
            AppError::InvalidOption(
                "changes must contain concrete setting values, without revision metadata".into(),
            )
        })?;
        let catalogue = client_call("client.runtime_settings_schema", || {
            self.client().runtime_settings_schema()
        })?;
        if catalogue.schema_version != 1 {
            return Err(AppError::InvalidOption(
                "This server uses an unsupported settings discovery version".into(),
            ));
        }
        let descriptor = catalogue
            .groups
            .iter()
            .find(|item| item.key == group)
            .ok_or_else(|| {
                AppError::InvalidOption("The server does not advertise this settings group".into())
            })?;
        if !descriptor.patch_supported {
            return Err(AppError::InvalidOption(
                descriptor
                    .read_only_reason
                    .as_deref()
                    .unwrap_or("This group requires a dedicated update workflow")
                    .into(),
            ));
        }
        Ok(to_value(client_call(
            "client.patch_runtime_settings",
            || self.client().patch_runtime_settings(group, patch),
        )?)?)
    }

    pub fn release_runtime_settings(
        &self,
        group: &str,
        expected_revision: ResourceRevision,
    ) -> Result<Value, AppError> {
        Ok(to_value(client_call(
            "client.release_runtime_settings",
            || {
                self.client()
                    .release_runtime_settings(group, expected_revision)
            },
        )?)?)
    }
}
