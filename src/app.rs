use std::fs::File;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use clap::ArgMatches;
use hubuum_client::{
    blocking::Client as BlockingClient, Authenticated, BaseUrl, Credentials, Token, Unauthenticated,
};
use log::debug;
use rpassword::prompt_password;
use tokio::task::spawn_blocking;
use tracing_subscriber::fmt as tracing_fmt;
use tracing_subscriber::EnvFilter;

use crate::catalog::CommandCatalog;
use crate::cli::{get_cli_config_path, update_config_from_cli};
use crate::config::{
    get_config, init_config, init_config_state, inspect_config_state, load_config, AppConfig,
};
use crate::errors::AppError;
use crate::files::{get_log_file, get_token_from_tokenfile, write_token_to_tokenfile};
use crate::models::TokenEntry;
use crate::services::AppServices;
use crate::theme::{paint, ThemeRole};

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
    let file = File::create(file)?;
    tracing_fmt()
        .with_writer(file)
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    Ok(())
}

pub fn load_app_config(matches: &ArgMatches) -> Result<Arc<AppConfig>, AppError> {
    let cli_config_path = get_cli_config_path(matches);
    let mut config = load_config(cli_config_path)?;
    update_config_from_cli(&mut config, matches);
    init_config_state(inspect_config_state(
        &config,
        get_cli_config_path(matches),
        matches,
    ))?;
    init_config(config.clone())?;
    Ok(Arc::new(config))
}

pub async fn login(config: Arc<AppConfig>) -> Result<Arc<BlockingClient<Authenticated>>, AppError> {
    spawn_blocking(move || {
        let baseurl = BaseUrl::from_str(&format!(
            "{}://{}:{}",
            config.server.protocol, config.server.hostname, config.server.port
        ))?;

        let client = BlockingClient::builder(baseurl)
            .validate_certs(config.server.ssl_validation)
            .build()?;

        authenticate(
            client,
            config.server.hostname.as_str(),
            config.server.identity_scope.as_deref(),
            config.server.username.as_str(),
            config.server.password.clone(),
        )
        .map(Arc::new)
    })
    .await
    .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
}

fn authenticate(
    client: BlockingClient<Unauthenticated>,
    hostname: &str,
    identity_scope: Option<&str>,
    username: &str,
    password: Option<String>,
) -> Result<BlockingClient<Authenticated>, AppError> {
    let token = get_token_from_tokenfile(hostname, identity_scope, username)?;
    if let Some(token) = token {
        debug!("Found existing token, testing validity...");
        if let Ok(client) = client.clone().login_with_token(Token::new(token)) {
            return Ok(client);
        }
    }

    let password = match password {
        Some(password) => password,
        None => {
            let scope = identity_scope
                .map(|scope| format!(" via {scope}"))
                .unwrap_or_default();
            prompt_password(format!("Password for {username}{scope} @ {hostname}: "))?
        }
    };

    let credentials = match identity_scope {
        Some(identity_scope) => {
            Credentials::scoped(identity_scope.to_string(), username.to_string(), password)
        }
        None => Credentials::new(username.to_string(), password),
    };
    let client = client.login(credentials)?;

    write_token_to_tokenfile(TokenEntry {
        hostname: hostname.to_string(),
        identity_scope: identity_scope.map(str::to_string),
        username: username.to_string(),
        token: client.token().to_string(),
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
        let config = get_config();
        let identity = config
            .server
            .identity_scope
            .as_deref()
            .map(|scope| format!("{}[{scope}]", config.server.username))
            .unwrap_or_else(|| config.server.username.clone());
        let base = format!(
            "{}@{}:{}",
            identity, config.server.hostname, config.server.port
        );
        let scope = session.scope();
        let pagination = session.next_page_command().map(|_| {
            if config.repl.enter_fetches_next_page {
                " [more:Enter Esc:cancel]"
            } else {
                " [more Esc:cancel]"
            }
        });
        let status = self
            .services
            .background()
            .prompt_status()
            .map(|s| format!("{s} "))
            .unwrap_or_default();
        let background = self
            .services
            .background()
            .take_prompt_badge()
            .map(|badge| format!("{badge} "))
            .unwrap_or_default();
        let pagination = pagination.unwrap_or_default();
        let base = paint(ThemeRole::Prompt, base);
        if scope.is_empty() {
            format!("{status}{background}{base}{pagination} > ")
        } else {
            format!(
                "{status}{background}{base} [{}]{pagination} > ",
                scope.join(" ")
            )
        }
    }
}
