use super::OfflineExecution;
use std::time::Duration;

use cli_command_derive::CommandArgs;
use hubuum_client::blocking::Client as BlockingClient;
use hubuum_client::{Authenticated, SessionInfo};
use hubuum_filter::OutputEnvelope;
use serde::Serialize;
use serde_json::json;

use super::builder::{catalog_command, CommandDocs};
use super::{required_option_or_pos, CliCommand};
use crate::app::{reachable_server_config, resume_saved_session};
use crate::build_info;
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::config::get_config;
use crate::diagnostics::client_call;
use crate::errors::{AppError, ReauthenticationRetry};
use crate::output::set_semantic_output;
use crate::services::{logout_session_client, AppServices};
use crate::tokenizer::CommandTokenizer;

const PROVIDER_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(&["auth"], catalog_command("status", AuthStatus::default(), CommandDocs {
        about: Some("Inspect the current or saved rotating session without logging in"),
        long_about: Some("Shows non-secret session deadlines and storage mode. A saved session is validated and may refresh. Missing sessions never trigger password login; token-file configuration is reported without reading or validating its credential."),
        ..Default::default()
    })).add_command(&["auth"], catalog_command("logout", AuthLogout::default(), CommandDocs {
        about: Some("Revoke the current rotating session and clear its matching saved credentials"),
        long_about: Some("Resumes an existing saved session when invoked outside the REPL; never creates a new login. Clears only the matching session family. PATs and service tokens are managed with token revoke commands."),
        ..Default::default()
    }));
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
    builder.add_command(
        &["auth", "approval"],
        catalog_command(
            "show",
            ApprovalShow::default(),
            CommandDocs {
                about: Some("Inspect retained credential approval evidence by ID"),
                ..CommandDocs::default()
            },
        ),
    );
}

#[derive(Clone, Default, CommandArgs)]
pub(crate) struct AuthStatus {}

impl CliCommand for AuthStatus {
    const OFFLINE: OfflineExecution =
        OfflineExecution::Always(|tokens| render_saved_session(tokens, false));

    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        Self::parse_tokens(tokens)?;
        render_session_status(services.gateway().session_info(), true)
    }
}

#[derive(Clone, Default, CommandArgs)]
pub(crate) struct AuthLogout {}

impl CliCommand for AuthLogout {
    const OFFLINE: OfflineExecution =
        OfflineExecution::Always(|tokens| render_saved_session(tokens, true));

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        Self::parse_tokens(tokens)?;
        services.gateway().logout_session()?;
        render_logout_result(true)
    }
}

pub(crate) fn render_saved_session(
    tokens: &CommandTokenizer,
    logout: bool,
) -> Result<(), AppError> {
    if logout {
        AuthLogout::parse_tokens(tokens)?;
    } else {
        AuthStatus::parse_tokens(tokens)?;
    }
    let client = resume_saved_session(get_config())?;
    if logout {
        logout_session(client.as_ref())
    } else {
        render_session_status(
            client.as_ref().and_then(|client| client.session_info()),
            client.is_some(),
        )
    }
}

fn render_session_status(info: Option<SessionInfo>, current_client: bool) -> Result<(), AppError> {
    let config = get_config();
    let status = if info.is_some() {
        "active"
    } else if config.server.token_file.is_some() {
        "token_file_configured"
    } else if current_client {
        "no_rotating_session"
    } else {
        "no_saved_session"
    };
    set_semantic_output(OutputEnvelope::detail(
        json!({
            "status": status,
            "storage": config.server.session_storage,
            "session_id": info.as_ref().map(|info| info.id()),
            "access_expires_at": info.as_ref().map(|info| info.access_expires_at()),
            "refresh_expires_at": info.as_ref().map(|info| info.refresh_expires_at()),
            "absolute_expires_at": info.as_ref().map(|info| info.absolute_expires_at()),
        }),
        Vec::new(),
    ))
}

fn logout_session(client: Option<&BlockingClient<Authenticated>>) -> Result<(), AppError> {
    let config = get_config();
    if config.server.token_file.is_some() {
        return Err(AppError::InvalidOption("auth logout manages rotating sessions; use user token revoke or service-account token revoke for bearer tokens".into()));
    }
    if let Some(client) = client {
        logout_session_client(client)?;
    }
    render_logout_result(client.is_some())
}

fn render_logout_result(logged_out: bool) -> Result<(), AppError> {
    set_semantic_output(OutputEnvelope::detail(
        json!({"status": if logged_out { "logged_out" } else { "no_saved_session" }}),
        Vec::new(),
    ))
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ApprovalShow {
    #[option(long = "id", help = "Credential approval ID")]
    id: Option<i32>,
}

impl CliCommand for ApprovalShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let id = required_option_or_pos(query.id, tokens, 0, "id")?;
        let value = services.gateway().credential_approval(id)?;
        set_semantic_output(OutputEnvelope::detail(
            value,
            vec![
                "id",
                "actor_id",
                "operation",
                "target_id",
                "restore_job_id",
                "authenticated_at",
                "expires_at",
                "consumed_at",
                "invalidated_at",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ))
    }
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct AuthProviders {}

impl CliCommand for AuthProviders {
    const OFFLINE: OfflineExecution = OfflineExecution::Always(render_auth_providers);

    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_auth_providers(tokens)
    }
}

pub(crate) fn render_auth_providers(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let _query = AuthProviders::parse_tokens(tokens)?;
    let config = reachable_server_config(get_config())?;
    let base_url = format!(
        "{}://{}:{}",
        config.server.protocol, config.server.hostname, config.server.port
    );
    let client = BlockingClient::builder_from_url(base_url)?
        .validate_certs(config.server.ssl_validation)
        .timeout(PROVIDER_DISCOVERY_TIMEOUT)
        .user_agent(format!("hubuum-cli/{}", build_info::VERSION))
        .build()?;
    let rows = client_call("client.session_providers", || client.session_providers())?
        .into_providers()
        .into_iter()
        .map(|provider| json!({"provider": provider}))
        .collect();

    set_semantic_output(OutputEnvelope::rows(rows, vec!["provider".to_string()]))
}
