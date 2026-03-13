use std::str::FromStr;
use std::sync::{Arc, Mutex};

use hubuum_client::{Authenticated, Credentials, SyncClient, Token, Unauthenticated};
use log::debug;
use tracing_subscriber::EnvFilter;

use crate::catalog::CommandCatalog;
use crate::config::{self, AppConfig};
use crate::errors::AppError;
use crate::files::{self, get_log_file};
use crate::models::TokenEntry;
use crate::services::AppServices;

#[derive(Clone)]
pub struct AppRuntime {
    pub config: Arc<AppConfig>,
    pub services: Arc<AppServices>,
    pub catalog: Arc<CommandCatalog>,
}

#[derive(Debug, Default)]
pub struct AppSession {
    scope: Vec<String>,
    next_page_command: Option<String>,
}

#[derive(Clone)]
pub struct SharedSession {
    inner: Arc<Mutex<AppSession>>,
}

impl SharedSession {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AppSession::default())),
        }
    }

    pub fn scope(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .scope
            .clone()
    }

    pub fn set_scope(&self, scope: Vec<String>) {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .scope = scope;
    }

    pub fn exit_scope(&self) -> bool {
        let mut guard = self
            .inner
            .lock()
            .expect("session scope lock should not be poisoned");
        guard.scope.pop().is_some()
    }

    pub fn next_page_command(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .next_page_command
            .clone()
    }

    pub fn set_next_page_command(&self, command: Option<String>) {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .next_page_command = command;
    }
}

pub fn init_logging() -> Result<(), AppError> {
    let file = get_log_file()?;
    let file = std::fs::File::create(file)?;
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    Ok(())
}

pub fn load_app_config(matches: &clap::ArgMatches) -> Result<Arc<AppConfig>, AppError> {
    let cli_config_path = crate::cli::get_cli_config_path(matches);
    let mut config = config::load_config(cli_config_path)?;
    crate::cli::update_config_from_cli(&mut config, matches);
    config::init_config_state(config::inspect_config_state(
        &config,
        crate::cli::get_cli_config_path(matches),
        matches,
    ))?;
    config::init_config(config.clone())?;
    Ok(Arc::new(config))
}

pub async fn login(config: Arc<AppConfig>) -> Result<Arc<SyncClient<Authenticated>>, AppError> {
    tokio::task::spawn_blocking(move || {
        let baseurl = hubuum_client::BaseUrl::from_str(&format!(
            "{}://{}:{}",
            config.server.protocol, config.server.hostname, config.server.port
        ))?;

        let client = hubuum_client::SyncClient::new_with_certificate_validation(
            baseurl,
            config.server.ssl_validation,
        );

        authenticate(
            client,
            config.server.hostname.as_str(),
            config.server.username.as_str(),
            config.server.password.clone(),
        )
        .map(Arc::new)
    })
    .await
    .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
}

fn authenticate(
    client: hubuum_client::SyncClient<Unauthenticated>,
    hostname: &str,
    username: &str,
    password: Option<String>,
) -> Result<SyncClient<Authenticated>, AppError> {
    let token = files::get_token_from_tokenfile(hostname, username)?;
    if let Some(token) = token {
        debug!("Found existing token, testing validity...");
        if let Ok(client) = client.clone().login_with_token(Token { token }) {
            return Ok(client);
        }
    }

    let password = match password {
        Some(password) => password,
        None => rpassword::prompt_password(format!("Password for {username} @ {hostname}: "))?,
    };

    let client = client.login(Credentials::new(username.to_string(), password))?;

    files::write_token_to_tokenfile(TokenEntry {
        hostname: hostname.to_string(),
        username: username.to_string(),
        token: client.get_token().to_string(),
    })?;

    Ok(client)
}

impl AppRuntime {
    pub fn new(
        config: Arc<AppConfig>,
        services: Arc<AppServices>,
        catalog: Arc<CommandCatalog>,
    ) -> Self {
        Self {
            config,
            services,
            catalog,
        }
    }

    pub fn prompt(&self, session: &SharedSession) -> String {
        let config = crate::config::get_config();
        let base = format!(
            "{}@{}:{}",
            config.server.username, config.server.hostname, config.server.port
        );
        let scope = session.scope();
        let pagination = session.next_page_command().map(|_| {
            if config.repl.enter_fetches_next_page {
                " [more:Enter]"
            } else {
                " [more]"
            }
        });
        let background = self
            .services
            .background()
            .take_prompt_badge()
            .map(|badge| format!("{badge} "))
            .unwrap_or_default();
        let pagination = pagination.unwrap_or_default();
        if scope.is_empty() {
            format!("{background}{base}{pagination} > ")
        } else {
            format!("{background}{base} [{}]{pagination} > ", scope.join(" "))
        }
    }
}
