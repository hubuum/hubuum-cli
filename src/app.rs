use std::error::Error as StdError;
use std::fs::read_to_string;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    get_config, get_config_state, init_config, init_config_state, inspect_config_state,
    load_config, update_runtime_server_port, AppConfig, ConfigSource,
};
use crate::defaults::Defaults;
use crate::errors::AppError;
use crate::files::{get_log_file, get_token_from_tokenfile, write_token_to_tokenfile};
use crate::models::TokenEntry;
use crate::services::AppServices;
use crate::theme::{paint, ThemeRole};

const SERVER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

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

pub struct LoginSession {
    config: Arc<AppConfig>,
    client: Arc<BlockingClient<Authenticated>>,
}

impl LoginSession {
    pub fn into_parts(self) -> (Arc<AppConfig>, Arc<BlockingClient<Authenticated>>) {
        (self.config, self.client)
    }
}

pub async fn login(config: Arc<AppConfig>) -> Result<LoginSession, AppError> {
    let ports = ServerPorts::from_config(&config);
    let session = login_with_cached_token(config, CachedToken::Allow, ports).await?;
    init_config(session.config.clone())?;
    update_runtime_server_port(session.config.server.port)?;
    Ok(session)
}

pub async fn reauthenticate(
    config: Arc<AppConfig>,
) -> Result<Arc<BlockingClient<Authenticated>>, AppError> {
    let port = config.server.port;
    login_with_cached_token(config, CachedToken::Skip, ServerPorts::configured(port))
        .await
        .map(|session| session.client)
}

#[derive(Debug, Clone, Copy)]
enum CachedToken {
    Allow,
    Skip,
}

async fn login_with_cached_token(
    config: Arc<AppConfig>,
    cached_token: CachedToken,
    ports: ServerPorts,
) -> Result<LoginSession, AppError> {
    spawn_blocking(move || {
        let config = reachable_server_config_on_ports(config, &ports)?;
        let client = build_client(&config)?;

        let client = authenticate(
            client,
            config.server.hostname.as_str(),
            config.server.identity_scope.as_deref(),
            config.server.username.as_str(),
            config.server.password.clone(),
            config.server.token_file.as_deref(),
            cached_token,
        )?;

        Ok(LoginSession {
            config,
            client: Arc::new(client),
        })
    })
    .await
    .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
}

pub(crate) fn reachable_server_config(config: Arc<AppConfig>) -> Result<Arc<AppConfig>, AppError> {
    let ports = ServerPorts::from_config(&config);
    reachable_server_config_on_ports(config, &ports)
}

fn reachable_server_config_on_ports(
    config: Arc<AppConfig>,
    ports: &ServerPorts,
) -> Result<Arc<AppConfig>, AppError> {
    let port = select_reachable_server_port(&config.server.hostname, ports, |port| {
        probe_server(&config, port)
    })?;

    if port == config.server.port {
        return Ok(config);
    }

    let mut selected = (*config).clone();
    selected.server.port = port;
    Ok(Arc::new(selected))
}

fn server_base_url(config: &AppConfig, port: u16) -> Result<BaseUrl, AppError> {
    let baseurl = BaseUrl::from_str(&format!(
        "{}://{}:{port}",
        config.server.protocol, config.server.hostname
    ))?;
    Ok(baseurl)
}

fn build_client(config: &AppConfig) -> Result<BlockingClient<Unauthenticated>, AppError> {
    let baseurl = server_base_url(config, config.server.port)?;
    BlockingClient::builder(baseurl)
        .validate_certs(config.server.ssl_validation)
        .user_agent(format!("hubuum-cli/{}", crate::build_info::VERSION))
        .build()
        .map_err(AppError::from)
}

fn probe_server(config: &AppConfig, port: u16) -> Result<(), String> {
    let baseurl = server_base_url(config, port).map_err(|error| error.to_string())?;
    let url = format!("{}healthz", baseurl.as_str());
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(!config.server.ssl_validation)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(SERVER_PROBE_TIMEOUT)
        .user_agent(format!("hubuum-cli/{}", crate::build_info::VERSION))
        .build()
        .map_err(|error| error_chain(&error))?;
    client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map(|_| ())
        .map_err(|error| error_chain(&error))
}

fn error_chain(error: &dyn StdError) -> String {
    let mut messages = vec![error.to_string()];
    let mut source = error.source();
    while let Some(error) = source {
        let message = error.to_string();
        if messages.last() != Some(&message) {
            messages.push(message);
        }
        source = error.source();
    }
    messages.join(": ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServerPorts(Vec<u16>);

impl ServerPorts {
    fn from_config(config: &AppConfig) -> Self {
        let uses_default = config.server.port == Defaults::SERVER_PORT
            && get_config_state()
                .entry("server.port")
                .is_some_and(|entry| entry.source == ConfigSource::Default);
        if uses_default {
            Self::defaults()
        } else {
            Self::configured(config.server.port)
        }
    }

    fn defaults() -> Self {
        Self(Defaults::SERVER_PORTS.to_vec())
    }

    fn configured(port: u16) -> Self {
        Self(vec![port])
    }

    fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().copied()
    }

    fn description(&self) -> String {
        let ports = self
            .iter()
            .map(|port| port.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if self.0.len() == 1 {
            format!("port {ports}")
        } else {
            format!("ports {ports}")
        }
    }
}

fn select_reachable_server_port(
    hostname: &str,
    ports: &ServerPorts,
    mut probe: impl FnMut(u16) -> Result<(), String>,
) -> Result<u16, AppError> {
    let mut failures = Vec::new();
    for port in ports.iter() {
        match probe(port) {
            Ok(()) => return Ok(port),
            Err(error) => failures.push(format!("port {port}: {error}")),
        }
    }

    Err(AppError::ServerUnreachable {
        hostname: hostname.to_string(),
        ports: ports.description(),
        failures: failures.join("; "),
    })
}

fn authenticate(
    client: BlockingClient<Unauthenticated>,
    hostname: &str,
    identity_scope: Option<&str>,
    username: &str,
    password: Option<String>,
    token_file: Option<&str>,
    cached_token: CachedToken,
) -> Result<BlockingClient<Authenticated>, AppError> {
    if let Some(token_file) = token_file {
        let token = BearerTokenFile::new(token_file)?.read()?;
        return client.login_with_token(token).map_err(AppError::from);
    }

    if matches!(cached_token, CachedToken::Allow) {
        let token = get_token_from_tokenfile(hostname, identity_scope, username)?;
        if let Some(token) = token {
            debug!("Found existing token, testing validity...");
            if let Ok(client) = client.clone().login_with_token(Token::new(token)) {
                return Ok(client);
            }
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

#[derive(Debug, Clone)]
struct BearerTokenFile(PathBuf);

impl BearerTokenFile {
    fn new(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(AppError::GeneralConfigError(
                "Bearer token file path cannot be empty".to_string(),
            ));
        }
        Ok(Self(path.to_path_buf()))
    }

    fn read(&self) -> Result<Token, AppError> {
        let contents = read_to_string(&self.0)?;
        let token = contents.trim();
        if token.is_empty() {
            return Err(AppError::GeneralConfigError(format!(
                "Bearer token file '{}' is empty",
                self.0.display()
            )));
        }
        Ok(Token::new(token))
    }
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

#[cfg(test)]
mod tests {
    use std::fs::write;

    use tempfile::tempdir;

    use super::{select_reachable_server_port, BearerTokenFile, ServerPorts};
    use crate::defaults::Defaults;

    #[test]
    fn default_server_ports_prefer_https_then_legacy_port() {
        assert_eq!(ServerPorts::defaults().0, vec![443, 8080]);
        assert_eq!(Defaults::SERVER_PORT, 443);
    }

    #[test]
    fn server_port_selection_tries_candidates_in_order() {
        let ports = ServerPorts::defaults();
        let mut attempts = Vec::new();

        let selected = select_reachable_server_port("hubuum.example.com", &ports, |port| {
            attempts.push(port);
            if port == 8080 {
                Ok(())
            } else {
                Err("connection refused".to_string())
            }
        })
        .expect("the fallback port should be selected");

        assert_eq!(attempts, vec![443, 8080]);
        assert_eq!(selected, 8080);
    }

    #[test]
    fn configured_server_port_is_the_only_candidate() {
        let ports = ServerPorts::configured(8443);
        let mut attempts = Vec::new();

        let error = select_reachable_server_port("hubuum.example.com", &ports, |port| {
            attempts.push(port);
            Err("connection refused".to_string())
        })
        .expect_err("an unreachable configured port should fail");

        assert_eq!(attempts, vec![8443]);
        assert!(error.to_string().contains("port 8443"));
    }

    #[test]
    fn bearer_token_file_trims_surrounding_whitespace() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("token");
        write(&path, "  service-account-token\n").expect("token file should be written");

        let token = BearerTokenFile::new(&path)
            .expect("path should be accepted")
            .read()
            .expect("token should be read");

        assert_eq!(token.as_str(), "service-account-token");
    }

    #[test]
    fn bearer_token_file_rejects_empty_tokens() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("token");
        write(&path, " \n").expect("token file should be written");

        let error = BearerTokenFile::new(&path)
            .expect("path should be accepted")
            .read()
            .expect_err("empty token should be rejected");

        assert!(error.to_string().contains("is empty"));
    }
}
