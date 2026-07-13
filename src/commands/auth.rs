use std::time::Duration;

use cli_command_derive::CommandArgs;
use hubuum_client::blocking::Client as BlockingClient;
use hubuum_filter::OutputEnvelope;
use serde::Serialize;
use serde_json::json;

use super::builder::{catalog_command, CommandDocs};
use super::CliCommand;
use crate::build_info;
use crate::catalog::CommandCatalogBuilder;
use crate::config::get_config;
use crate::errors::AppError;
use crate::output::set_semantic_output;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

const PROVIDER_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &["auth"],
        catalog_command(
            "providers",
            AuthProviders::default(),
            CommandDocs {
                about: Some("List authentication providers"),
                long_about: Some(
                    "Discover the configured server's authentication providers without logging in. Use a provider name as the server.identity_scope setting or with --identity-scope.",
                ),
                examples: Some("--output json"),
            },
        ),
    );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AuthProviders {}

impl CliCommand for AuthProviders {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_auth_providers(tokens)
    }
}

pub(crate) fn render_auth_providers(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let _query = AuthProviders::parse_tokens(tokens)?;
    let config = get_config();
    let base_url = format!(
        "{}://{}:{}",
        config.server.protocol, config.server.hostname, config.server.port
    );
    let client = BlockingClient::builder_from_url(base_url)?
        .validate_certs(config.server.ssl_validation)
        .timeout(PROVIDER_DISCOVERY_TIMEOUT)
        .user_agent(format!("hubuum-cli/{}", build_info::VERSION))
        .build()?;
    let rows = client
        .auth_providers()?
        .into_providers()
        .into_iter()
        .map(|provider| json!({"provider": provider}))
        .collect();

    set_semantic_output(OutputEnvelope::rows(rows, vec!["provider".to_string()]))
}
