use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::defaults::Defaults;
use crate::files::get_system_config_path;
use crate::models::Protocol;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub completion: CompletionConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub ssl_validation: bool,
    pub api_version: String,
    pub username: String,
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                hostname: Defaults::SERVER_HOSTNAME.to_string(),
                port: Defaults::SERVER_PORT,
                ssl_validation: Defaults::SERVER_SSL_VALIDATION,
                api_version: Defaults::API_VERSION.to_string(),
                username: Defaults::USER_USERNAME.to_string(),
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
        // 2. Load user-specific config
        .add_source(File::from(user_config).required(false))
        // 3. Add in settings from the environment (with a prefix of HUBUUM_CLI_)
        .add_source(Environment::with_prefix("HUBUUM_CLI").separator("__"));

    // 4. Load CLI-specified config file, if provided
    if let Some(config_path) = cli_config_path {
        builder = builder.add_source(File::from(config_path).required(true));
    }

    let config = builder.build()?;

    config.try_deserialize()
}
