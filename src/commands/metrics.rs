use std::time::Duration;

use cli_command_derive::CommandArgs;
use hubuum_client::blocking::Client as BlockingClient;
use hubuum_filter::OutputEnvelope;
use serde::Serialize;

use super::builder::{catalog_command, CommandDocs};
use super::CliCommand;
use crate::build_info;
use crate::catalog::CommandCatalogBuilder;
use crate::config::get_config;
use crate::errors::AppError;
use crate::output::set_semantic_output;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

const METRICS_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &[],
        catalog_command(
            "metrics",
            Metrics::default(),
            CommandDocs {
                about: Some("Fetch Prometheus server metrics"),
                long_about: Some(
                    "Fetch Prometheus exposition text without logging in. The default route is /metrics; use --path when the server exposes a different configured route. The server's metrics client allowlist still applies.",
                ),
                examples: Some("--path /internal/metrics\n--output json"),
            },
        ),
    );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct Metrics {
    #[option(
        long = "path",
        help = "Metrics route exposed by the server (default: /metrics)"
    )]
    pub path: Option<String>,
}

impl CliCommand for Metrics {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_metrics(tokens)
    }
}

pub(crate) fn render_metrics(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = Metrics::parse_tokens(tokens)?;
    let config = get_config();
    let base_url = format!(
        "{}://{}:{}",
        config.server.protocol, config.server.hostname, config.server.port
    );
    let client = BlockingClient::builder_from_url(base_url)?
        .validate_certs(config.server.ssl_validation)
        .timeout(METRICS_TIMEOUT)
        .user_agent(format!("hubuum-cli/{}", build_info::VERSION))
        .build()?;
    let metrics = match query.path.as_deref() {
        Some(path) => client.metrics_at(path)?,
        None => client.metrics()?,
    };

    set_semantic_output(OutputEnvelope::lines(
        metrics.lines().map(str::to_string).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use hubuum_client::DEFAULT_METRICS_PATH;

    use super::Metrics;
    use crate::commands::CommandArgs;
    use crate::tokenizer::CommandTokenizer;

    #[test]
    fn default_metrics_path_matches_client_default() {
        assert_eq!(DEFAULT_METRICS_PATH, "/metrics");
    }

    #[test]
    fn configured_metrics_path_is_parsed() {
        let tokens = CommandTokenizer::new_without_value_source_resolution(
            "metrics --path /internal/metrics",
            "metrics",
            &Metrics::options(),
        )
        .expect("metrics command should tokenize");

        let query = Metrics::parse_tokens(&tokens).expect("metrics command should parse");
        assert_eq!(query.path.as_deref(), Some("/internal/metrics"));
    }
}
