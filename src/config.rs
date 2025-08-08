use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::defaults::Defaults;
use crate::errors::AppError;
use crate::files::get_system_config_path;
use crate::models::{OutputFormat, Protocol};

use std::sync::OnceLock;

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

pub fn init_config(cfg: AppConfig) -> Result<(), AppError> {
    CONFIG
        .set(cfg)
        .map_err(|_| AppError::GeneralConfigError("Failed to initialize config".to_string()))
}

pub fn get_config() -> &'static AppConfig {
    CONFIG
        .get()
        .expect("App config not initialized. Call init_config(...) in main after loading.")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub completion: CompletionConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub ssl_validation: bool,
    pub api_version: String,
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    pub protocol: Protocol,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheConfig {
    pub time: u64,
    pub size: i32,
    pub disable: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CompletionConfig {
    pub disable_api_related: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub padding: i8,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                hostname: Defaults::SERVER_HOSTNAME.to_string(),
                port: Defaults::SERVER_PORT,
                ssl_validation: Defaults::SERVER_SSL_VALIDATION,
                api_version: Defaults::API_VERSION.to_string(),
                username: Defaults::USER_USERNAME.to_string(),
                password: None,
                protocol: Defaults::PROTOCOL,
            },
            cache: CacheConfig {
                time: Defaults::CACHE_TIME,
                size: Defaults::CACHE_SIZE,
                disable: Defaults::CACHE_DISABLE,
            },
            completion: CompletionConfig {
                disable_api_related: Defaults::COMPLETION_DISABLE_API_RELATED,
            },
            output: OutputConfig {
                format: Defaults::OUTPUT_FORMAT,
                padding: Defaults::OUTPUT_PADDING,
            },
        }
    }
}

pub fn load_config(cli_config_path: Option<PathBuf>) -> Result<AppConfig, ConfigError> {
    let system_config = get_system_config_path();
    let user_config = dirs::config_dir()
        .map(|mut path| {
            path.push(".hubuum_cli/config.toml");
            path
        })
        .unwrap_or_else(|| PathBuf::from("config.toml"));

    let mut builder = Config::builder()
        // Start with default values
        .set_default("output.format", Defaults::OUTPUT_FORMAT.to_string())?
        .set_default("output.padding", Defaults::OUTPUT_PADDING)?
        .set_default("server.hostname", Defaults::SERVER_HOSTNAME)?
        .set_default("server.port", Defaults::SERVER_PORT)?
        .set_default("server.ssl_validation", Defaults::SERVER_SSL_VALIDATION)?
        .set_default("server.api_version", Defaults::API_VERSION)?
        .set_default("server.username", Defaults::USER_USERNAME)?
        .set_default("server.protocol", Defaults::PROTOCOL)?
        .set_default("cache.time", Defaults::CACHE_TIME)?
        .set_default("cache.size", Defaults::CACHE_SIZE)?
        .set_default("cache.disable", Defaults::CACHE_DISABLE)?
        .set_default(
            "completion.disable_api_related",
            Defaults::COMPLETION_DISABLE_API_RELATED,
        )?
        // 1. Load system-wide config
        .add_source(File::from(system_config).required(false))
        // 2. Add in settings from the environment (with a prefix of HUBUUM_CLI_)
        .add_source(Environment::with_prefix("HUBUUM_CLI").separator("__"))
        // 3. Load user-specific config
        .add_source(File::from(user_config).required(false));

    // 4. Load CLI-specified config file, if provided
    if let Some(config_path) = cli_config_path {
        builder = builder.add_source(File::from(config_path).required(true));
    }

    let config = builder.build()?;

    config.try_deserialize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Protocol;
    use serial_test::serial;
    use std::env;

    /// Helper to clear all HUBUUM_CLI_... vars we use in this test.
    fn clear_env() {
        for &var in &[
            "HUBUUM_CLI__SERVER__HOSTNAME",
            "HUBUUM_CLI__SERVER__PORT",
            "HUBUUM_CLI__SERVER__SSL_VALIDATION",
            "HUBUUM_CLI__SERVER__API_VERSION",
            "HUBUUM_CLI__SERVER__USERNAME",
            "HUBUUM_CLI__SERVER__PASSWORD",
            "HUBUUM_CLI__SERVER__PROTOCOL",
            "HUBUUM_CLI__CACHE__TIME",
            "HUBUUM_CLI__CACHE__SIZE",
            "HUBUUM_CLI__CACHE__DISABLE",
            "HUBUUM_CLI__COMPLETION__DISABLE_API_RELATED",
        ] {
            env::remove_var(var);
        }
    }

    #[test]
    #[serial]
    fn env_overrides_entire_config() {
        clear_env();
        env::set_var("HUBUUM_CLI__SERVER__HOSTNAME", "env.example.com");
        env::set_var("HUBUUM_CLI__SERVER__PORT", "4321");
        env::set_var("HUBUUM_CLI__SERVER__SSL_VALIDATION", "false");
        env::set_var("HUBUUM_CLI__SERVER__API_VERSION", "v9");
        env::set_var("HUBUUM_CLI__SERVER__USERNAME", "env_user");
        env::set_var("HUBUUM_CLI__SERVER__PASSWORD", "hunter2");
        env::set_var("HUBUUM_CLI__SERVER__PROTOCOL", "http");

        env::set_var("HUBUUM_CLI__CACHE__TIME", "99");
        env::set_var("HUBUUM_CLI__CACHE__SIZE", "42");
        env::set_var("HUBUUM_CLI__CACHE__DISABLE", "true");

        env::set_var("HUBUUM_CLI__COMPLETION__DISABLE_API_RELATED", "true");

        // 2. load and assert
        let cfg = load_config(None).expect("failed to load config from env");

        assert_eq!(cfg.server.hostname, "env.example.com");
        assert_eq!(cfg.server.port, 4321);
        assert!(!cfg.server.ssl_validation);
        assert_eq!(cfg.server.api_version, "v9");
        assert_eq!(cfg.server.username, "env_user");
        assert_eq!(cfg.server.password, Some("hunter2".into()));
        assert_eq!(cfg.server.protocol, Protocol::Http);

        assert_eq!(cfg.cache.time, 99);
        assert_eq!(cfg.cache.size, 42);
        assert!(cfg.cache.disable);

        assert!(cfg.completion.disable_api_related);
        clear_env();
    }

    #[test]
    #[serial]
    fn mixing_env_and_defaults() {
        clear_env();
        // Only override one value
        env::set_var("HUBUUM_CLI__SERVER__PORT", "5555");
        let cfg = load_config(None).unwrap();

        // port should be env value, everything else falls back to Default::default()
        assert_eq!(cfg.server.port, 5555);
        assert_eq!(cfg.server.hostname, Defaults::SERVER_HOSTNAME);
        assert_eq!(cfg.cache.disable, Defaults::CACHE_DISABLE);
        assert!(cfg.server.password.is_none());

        clear_env();
    }
}
