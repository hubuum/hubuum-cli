use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use serde::Serialize;
use serde_json::{json, Value};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::set_semantic_output;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &["admin"],
        catalog_command(
            "config",
            AdminConfig::default(),
            CommandDocs {
                about: Some("Show the server's effective configuration"),
                long_about: Some(
                    "Show the authenticated server's effective process configuration. Secrets are redacted by the server. Administrator access is required.",
                ),
                examples: Some("--output json"),
            },
        ),
    );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AdminConfig {}

impl CliCommand for AdminConfig {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let _query = Self::parse_tokens(tokens)?;
        let config = services.gateway().server_config()?;
        render_server_config(config, desired_format(tokens))
    }
}

fn render_server_config(config: Value, format: OutputFormat) -> Result<(), AppError> {
    render_structured_value(config, format)
}

pub(super) fn render_structured_value(config: Value, format: OutputFormat) -> Result<(), AppError> {
    match format {
        OutputFormat::Json => set_semantic_output(OutputEnvelope::detail(config, Vec::new())),
        OutputFormat::Text => {
            let mut rows = Vec::new();
            flatten_config(None, &config, &mut rows);
            set_semantic_output(OutputEnvelope::rows(
                rows,
                vec!["key".to_string(), "value".to_string()],
            ))
        }
    }
}

fn flatten_config(prefix: Option<&str>, value: &Value, rows: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let path = match prefix {
                    Some(prefix) => format!("{prefix}.{key}"),
                    None => key.clone(),
                };
                flatten_config(Some(&path), value, rows);
            }
        }
        Value::Array(values) => {
            rows.push(json!({
                "key": prefix.unwrap_or_default(),
                "value": Value::Array(values.clone()).to_string(),
            }));
        }
        Value::String(value) => rows.push(json!({
            "key": prefix.unwrap_or_default(),
            "value": value,
        })),
        Value::Null => rows.push(json!({
            "key": prefix.unwrap_or_default(),
            "value": "",
        })),
        value => rows.push(json!({
            "key": prefix.unwrap_or_default(),
            "value": value.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::flatten_config;

    #[test]
    fn server_config_is_flattened_to_stable_dotted_keys() {
        let mut rows = Vec::new();
        flatten_config(
            None,
            &json!({
                "server": {"bind_port": 8080, "metrics_enabled": true},
                "authentication": {"admin_identity_scope": null}
            }),
            &mut rows,
        );

        assert_eq!(
            rows,
            vec![
                json!({"key": "authentication.admin_identity_scope", "value": ""}),
                json!({"key": "server.bind_port", "value": "8080"}),
                json!({"key": "server.metrics_enabled", "value": "true"}),
            ]
        );
    }
}
