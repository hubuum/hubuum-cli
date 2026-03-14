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

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

    use httpmock::prelude::*;
    use hubuum_client::{
        BaseUrl, Credentials, SyncClient, TaskKind, TaskLinks, TaskProgress, TaskResponse,
        TaskStatus, Unauthenticated,
    };
    use serial_test::serial;
    use tempfile::TempDir;
    use tokio::runtime::{Handle, Runtime};

    use super::{authenticate, AppRuntime, SharedSession};
    use crate::commands::build_command_catalog;
    use crate::config::{init_config, AppConfig};
    use crate::domain::TaskRecord;
    use crate::files;
    use crate::models::TokenEntry;
    use crate::services::AppServices;

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self { saved: Vec::new() }
        }

        fn set(&mut self, key: &'static str, value: String) {
            self.saved.push((key, std::env::var(key).ok()));
            std::env::set_var(key, value);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            while let Some((key, value)) = self.saved.pop() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn use_temp_home(home: &TempDir) -> EnvGuard {
        let mut guard = EnvGuard::new();
        guard.set("HOME", home.path().to_string_lossy().to_string());
        guard.set(
            "XDG_DATA_HOME",
            home.path().join("data").to_string_lossy().to_string(),
        );
        guard.set(
            "XDG_CONFIG_HOME",
            home.path().join("config").to_string_lossy().to_string(),
        );
        guard
    }

    fn mock_token_validation(server: &MockServer, token: &str, status: u16) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v0/auth/validate")
                .header("authorization", format!("Bearer {token}"));
            then.status(status);
        });
    }

    fn mock_login(server: &MockServer, password: &str, token: &str) {
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v0/auth/login")
                .json_body_obj(&serde_json::json!({
                    "username": "tester",
                    "password": password,
                }));
            then.status(200)
                .header("content-type", "application/json")
                .json_body_obj(&serde_json::json!({ "token": token }));
        });
    }

    fn unauthenticated_client(server: &MockServer) -> SyncClient<Unauthenticated> {
        let base_url = BaseUrl::from_str(&server.base_url()).expect("base URL should be valid");
        SyncClient::new_with_certificate_validation(base_url, true)
    }

    fn task(task_id: i32, status: TaskStatus, summary: Option<&str>) -> TaskRecord {
        TaskRecord(TaskResponse {
            id: task_id,
            kind: TaskKind::Import,
            status,
            submitted_by: Some(1),
            created_at: Default::default(),
            started_at: None,
            finished_at: None,
            progress: TaskProgress {
                total_items: 1,
                processed_items: if matches!(status, TaskStatus::Queued) {
                    0
                } else {
                    1
                },
                success_items: if matches!(status, TaskStatus::Succeeded) {
                    1
                } else {
                    0
                },
                failed_items: 0,
            },
            summary: summary.map(str::to_string),
            request_redacted_at: None,
            links: TaskLinks {
                task: "/api/v1/tasks/42".to_string(),
                events: "/api/v1/tasks/42/events".to_string(),
                import_url: None,
                import_results: None,
            },
            details: None,
        })
    }

    fn runtime_with_config(config: AppConfig, server: &MockServer, handle: Handle) -> AppRuntime {
        let base_url = BaseUrl::from_str(&server.base_url()).expect("base URL should be valid");
        let client = SyncClient::new_with_certificate_validation(base_url, true)
            .login(Credentials::new("tester".to_string(), "secret".to_string()))
            .expect("login should succeed");
        init_config(config.clone()).expect("config should initialize");

        AppRuntime::new(
            Arc::new(config),
            Arc::new(AppServices::new(
                Arc::new(client),
                handle,
                Duration::from_secs(1),
            )),
            Arc::new(build_command_catalog()),
        )
    }

    #[test]
    #[serial]
    fn authenticate_uses_cached_token_when_valid() {
        let home = TempDir::new().expect("temp home should be created");
        let _guard = use_temp_home(&home);
        let server = MockServer::start();
        mock_token_validation(&server, "cached-token", 200);
        files::write_token_to_tokenfile(TokenEntry {
            hostname: server.host(),
            username: "tester".to_string(),
            token: "cached-token".to_string(),
        })
        .expect("token file should be written");

        let client = authenticate(
            unauthenticated_client(&server),
            &server.host(),
            "tester",
            Some("secret".to_string()),
        )
        .expect("cached token should authenticate");

        assert_eq!(client.get_token(), "cached-token");
    }

    #[test]
    #[serial]
    fn authenticate_falls_back_to_password_login_and_rewrites_token_file() {
        let home = TempDir::new().expect("temp home should be created");
        let _guard = use_temp_home(&home);
        let server = MockServer::start();
        mock_token_validation(&server, "stale-token", 401);
        mock_login(&server, "secret", "fresh-token");
        files::write_token_to_tokenfile(TokenEntry {
            hostname: server.host(),
            username: "tester".to_string(),
            token: "stale-token".to_string(),
        })
        .expect("token file should be written");

        let client = authenticate(
            unauthenticated_client(&server),
            &server.host(),
            "tester",
            Some("secret".to_string()),
        )
        .expect("login should fall back to password");

        assert_eq!(client.get_token(), "fresh-token");
        assert_eq!(
            files::get_token_from_tokenfile(&server.host(), "tester")
                .expect("token lookup should succeed"),
            Some("fresh-token".to_string())
        );
    }

    #[test]
    #[serial]
    fn authenticate_writes_token_file_when_logging_in_without_cache() {
        let home = TempDir::new().expect("temp home should be created");
        let _guard = use_temp_home(&home);
        let server = MockServer::start();
        mock_login(&server, "secret", "fresh-token");

        let client = authenticate(
            unauthenticated_client(&server),
            &server.host(),
            "tester",
            Some("secret".to_string()),
        )
        .expect("fresh login should succeed");

        assert_eq!(client.get_token(), "fresh-token");
        assert_eq!(
            files::get_token_from_tokenfile(&server.host(), "tester")
                .expect("token lookup should succeed"),
            Some("fresh-token".to_string())
        );
    }

    #[test]
    #[serial]
    fn prompt_formats_scope_and_pagination_hint() {
        let runtime = Runtime::new().expect("runtime should build");
        let server = MockServer::start();
        mock_login(&server, "secret", "runtime-token");
        let mut config = AppConfig::default();
        config.server.hostname = server.host();
        config.server.port = server.port();
        config.server.username = "tester".to_string();
        config.repl.enter_fetches_next_page = false;

        let app = runtime_with_config(config, &server, runtime.handle().clone());
        let session = SharedSession::new();
        session.set_scope(vec!["object".to_string()]);
        session.set_next_page_command(Some("object list --cursor abc".to_string()));

        assert_eq!(
            app.prompt(&session),
            format!(
                "tester@{}:{} [object] [more] > ",
                server.host(),
                server.port()
            )
        );
    }

    #[test]
    #[serial]
    fn prompt_includes_background_badge_and_enter_hint() {
        let runtime = Runtime::new().expect("runtime should build");
        let server = MockServer::start();
        mock_login(&server, "secret", "runtime-token");
        let mut config = AppConfig::default();
        config.server.hostname = server.host();
        config.server.port = server.port();
        config.server.username = "tester".to_string();
        config.repl.enter_fetches_next_page = true;

        let app = runtime_with_config(config, &server, runtime.handle().clone());
        app.services.background().enable();
        app.services
            .background()
            .watch_task(task(42, TaskStatus::Running, Some("running")), "import 42");

        let session = SharedSession::new();
        session.set_next_page_command(Some("next".to_string()));

        let prompt = app.prompt(&session);
        assert!(prompt.starts_with("[bg:1] tester@"));
        assert!(prompt.contains("[more"));
    }
}
