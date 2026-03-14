use clap::{parser::ValueSource, ArgMatches};
use config::{Config, ConfigError, Environment, File};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::defaults::Defaults;
use crate::errors::AppError;
use crate::files::{get_system_config_path, get_user_config_path};
use crate::models::{OutputFormat, Protocol, TableStyle};

static CONFIG: Lazy<RwLock<AppConfig>> = Lazy::new(|| RwLock::new(AppConfig::default()));
static CONFIG_STATE: Lazy<RwLock<Option<ConfigState>>> = Lazy::new(|| RwLock::new(None));

pub fn init_config(cfg: AppConfig) -> Result<(), AppError> {
    *CONFIG
        .write()
        .map_err(|_| AppError::GeneralConfigError("Failed to update config".to_string()))? = cfg;
    Ok(())
}

pub fn get_config() -> AppConfig {
    CONFIG
        .read()
        .expect("config lock should not be poisoned")
        .clone()
}

pub fn init_config_state(state: ConfigState) -> Result<(), AppError> {
    *CONFIG_STATE
        .write()
        .map_err(|_| AppError::GeneralConfigError("Failed to update config state".to_string()))? =
        Some(state);
    Ok(())
}

pub fn get_config_state() -> ConfigState {
    CONFIG_STATE
        .read()
        .expect("config state lock should not be poisoned")
        .clone()
        .expect("Config state not initialized. Call init_config_state(...) in main after loading.")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigState {
    pub paths: ConfigPaths,
    pub entries: Vec<ConfigEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigPaths {
    pub system: PathBuf,
    pub user: PathBuf,
    pub custom: Option<PathBuf>,
    pub write_target: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub source: ConfigSource,
    pub source_detail: Option<String>,
    pub sensitive: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    Default,
    SystemFile,
    UserFile,
    CustomFile,
    Environment,
    CliOption,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub completion: CompletionConfig,
    pub background: BackgroundConfig,
    pub repl: ReplConfig,
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
pub struct BackgroundConfig {
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplConfig {
    pub enter_fetches_next_page: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub padding: i8,
    pub table_style: TableStyle,
}

#[derive(Debug, Clone, Copy)]
enum ConfigValueKind {
    String,
    Bool,
    U16,
    U64,
    I8,
    I32,
    Protocol,
    OutputFormat,
    TableStyle,
}

#[derive(Debug, Clone, Copy)]
struct ConfigKeyDescriptor {
    key: &'static str,
    cli_arg: Option<&'static str>,
    env_var: &'static str,
    value_kind: ConfigValueKind,
    sensitive: bool,
}

const CONFIG_KEYS: &[ConfigKeyDescriptor] = &[
    ConfigKeyDescriptor {
        key: "server.hostname",
        cli_arg: Some("hostname"),
        env_var: "HUBUUM_CLI__SERVER__HOSTNAME",
        value_kind: ConfigValueKind::String,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "server.port",
        cli_arg: Some("port"),
        env_var: "HUBUUM_CLI__SERVER__PORT",
        value_kind: ConfigValueKind::U16,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "server.ssl_validation",
        cli_arg: Some("ssl_validation"),
        env_var: "HUBUUM_CLI__SERVER__SSL_VALIDATION",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "server.api_version",
        cli_arg: None,
        env_var: "HUBUUM_CLI__SERVER__API_VERSION",
        value_kind: ConfigValueKind::String,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "server.username",
        cli_arg: Some("username"),
        env_var: "HUBUUM_CLI__SERVER__USERNAME",
        value_kind: ConfigValueKind::String,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "server.password",
        cli_arg: Some("password"),
        env_var: "HUBUUM_CLI__SERVER__PASSWORD",
        value_kind: ConfigValueKind::String,
        sensitive: true,
    },
    ConfigKeyDescriptor {
        key: "server.protocol",
        cli_arg: Some("protocol"),
        env_var: "HUBUUM_CLI__SERVER__PROTOCOL",
        value_kind: ConfigValueKind::Protocol,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "cache.time",
        cli_arg: Some("cache_time"),
        env_var: "HUBUUM_CLI__CACHE__TIME",
        value_kind: ConfigValueKind::U64,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "cache.size",
        cli_arg: Some("cache_size"),
        env_var: "HUBUUM_CLI__CACHE__SIZE",
        value_kind: ConfigValueKind::I32,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "cache.disable",
        cli_arg: Some("cache_disable"),
        env_var: "HUBUUM_CLI__CACHE__DISABLE",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "completion.disable_api_related",
        cli_arg: Some("completion_disable_api"),
        env_var: "HUBUUM_CLI__COMPLETION__DISABLE_API_RELATED",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "background.poll_interval_seconds",
        cli_arg: Some("background_poll_interval"),
        env_var: "HUBUUM_CLI__BACKGROUND__POLL_INTERVAL_SECONDS",
        value_kind: ConfigValueKind::U64,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "repl.enter_fetches_next_page",
        cli_arg: None,
        env_var: "HUBUUM_CLI__REPL__ENTER_FETCHES_NEXT_PAGE",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.format",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__FORMAT",
        value_kind: ConfigValueKind::OutputFormat,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.padding",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__PADDING",
        value_kind: ConfigValueKind::I8,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.table_style",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__TABLE_STYLE",
        value_kind: ConfigValueKind::TableStyle,
        sensitive: false,
    },
];

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
            background: BackgroundConfig {
                poll_interval_seconds: Defaults::BACKGROUND_POLL_INTERVAL_SECONDS,
            },
            repl: ReplConfig {
                enter_fetches_next_page: Defaults::REPL_ENTER_FETCHES_NEXT_PAGE,
            },
            output: OutputConfig {
                format: Defaults::OUTPUT_FORMAT,
                padding: Defaults::OUTPUT_PADDING,
                table_style: Defaults::OUTPUT_TABLE_STYLE,
            },
        }
    }
}

impl ConfigState {
    pub fn entry(&self, key: &str) -> Option<&ConfigEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }
}

pub fn config_key_names() -> Vec<&'static str> {
    CONFIG_KEYS
        .iter()
        .map(|descriptor| descriptor.key)
        .collect()
}

pub fn inspect_config_state(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
    matches: &ArgMatches,
) -> ConfigState {
    inspect_config_state_inner(config, cli_config_path, Some(matches))
}

fn inspect_config_state_inner(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
    matches: Option<&ArgMatches>,
) -> ConfigState {
    let system = get_system_config_path();
    let user = get_user_config_path();
    let custom = cli_config_path;
    let write_target = custom.clone().unwrap_or_else(|| user.clone());
    let system_toml = read_toml_file(&system);
    let user_toml = read_toml_file(&user);
    let custom_toml = custom.as_ref().and_then(|path| read_toml_file(path));
    let resolution_context = ConfigSourceResolutionContext {
        system_path: &system,
        system_toml: system_toml.as_ref(),
        user_path: &user,
        user_toml: user_toml.as_ref(),
        custom_path: custom.as_deref(),
        custom_toml: custom_toml.as_ref(),
        matches,
    };

    let entries = CONFIG_KEYS
        .iter()
        .map(|descriptor| {
            let (source, source_detail) = resolve_config_source(descriptor, &resolution_context);
            ConfigEntry {
                key: descriptor.key.to_string(),
                value: display_config_value(
                    config_value(config, descriptor.key),
                    descriptor.sensitive,
                ),
                source,
                source_detail,
                sensitive: descriptor.sensitive,
            }
        })
        .collect();

    ConfigState {
        paths: ConfigPaths {
            system,
            user,
            custom,
            write_target,
        },
        entries,
    }
}

pub fn set_persisted_value(key: &str, value: &str) -> Result<PathBuf, AppError> {
    let descriptor = descriptor_for_key(key)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(toml::Value::Table(toml::map::Map::new()));
    let parsed = parse_config_value(descriptor, value)?;
    set_toml_path(&mut root, descriptor.key, parsed)?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn unset_persisted_value(key: &str) -> Result<PathBuf, AppError> {
    let descriptor = descriptor_for_key(key)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(toml::Value::Table(toml::map::Map::new()));
    remove_toml_path(&mut root, descriptor.key);
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn reload_runtime_config() -> Result<(), AppError> {
    let previous_config = get_config();
    let previous_state = get_config_state();
    let custom = get_config_state().paths.custom.clone();
    let mut config = load_config(custom.clone())?;
    preserve_cli_overrides(&previous_config, &previous_state, &mut config);
    let mut state = inspect_config_state_without_cli(&config, custom);
    preserve_cli_override_sources(&previous_state, &mut state);
    init_config_state(state)?;
    init_config(config)?;
    Ok(())
}

pub fn load_config(cli_config_path: Option<PathBuf>) -> Result<AppConfig, ConfigError> {
    let system_config = get_system_config_path();
    let user_config = get_user_config_path();

    let mut builder = Config::builder()
        // Start with default values
        .set_default("output.format", Defaults::OUTPUT_FORMAT.to_string())?
        .set_default("output.padding", Defaults::OUTPUT_PADDING)?
        .set_default(
            "output.table_style",
            Defaults::OUTPUT_TABLE_STYLE.to_string(),
        )?
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
        .set_default(
            "background.poll_interval_seconds",
            Defaults::BACKGROUND_POLL_INTERVAL_SECONDS,
        )?
        .set_default(
            "repl.enter_fetches_next_page",
            Defaults::REPL_ENTER_FETCHES_NEXT_PAGE,
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

pub fn inspect_config_state_without_cli(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
) -> ConfigState {
    inspect_config_state_inner(config, cli_config_path, None)
}

fn descriptor_for_key(key: &str) -> Result<&'static ConfigKeyDescriptor, AppError> {
    CONFIG_KEYS
        .iter()
        .find(|descriptor| descriptor.key == key)
        .ok_or_else(|| {
            AppError::ParseError(format!(
                "Unknown config key: {key}. Use one of: {}",
                config_key_names().join(", ")
            ))
        })
}

fn preserve_cli_overrides(
    previous_config: &AppConfig,
    previous_state: &ConfigState,
    reloaded_config: &mut AppConfig,
) {
    for entry in previous_state
        .entries
        .iter()
        .filter(|entry| entry.source == ConfigSource::CliOption)
    {
        copy_config_value(previous_config, reloaded_config, &entry.key);
    }
}

fn preserve_cli_override_sources(previous_state: &ConfigState, reloaded_state: &mut ConfigState) {
    for previous_entry in previous_state
        .entries
        .iter()
        .filter(|entry| entry.source == ConfigSource::CliOption)
    {
        if let Some(entry) = reloaded_state
            .entries
            .iter_mut()
            .find(|entry| entry.key == previous_entry.key)
        {
            entry.source = ConfigSource::CliOption;
            entry.source_detail = previous_entry.source_detail.clone();
        }
    }
}

fn copy_config_value(source: &AppConfig, target: &mut AppConfig, key: &str) {
    match key {
        "server.hostname" => target.server.hostname = source.server.hostname.clone(),
        "server.port" => target.server.port = source.server.port,
        "server.ssl_validation" => target.server.ssl_validation = source.server.ssl_validation,
        "server.api_version" => target.server.api_version = source.server.api_version.clone(),
        "server.username" => target.server.username = source.server.username.clone(),
        "server.password" => target.server.password = source.server.password.clone(),
        "server.protocol" => target.server.protocol = source.server.protocol.clone(),
        "cache.time" => target.cache.time = source.cache.time,
        "cache.size" => target.cache.size = source.cache.size,
        "cache.disable" => target.cache.disable = source.cache.disable,
        "completion.disable_api_related" => {
            target.completion.disable_api_related = source.completion.disable_api_related
        }
        "background.poll_interval_seconds" => {
            target.background.poll_interval_seconds = source.background.poll_interval_seconds
        }
        "repl.enter_fetches_next_page" => {
            target.repl.enter_fetches_next_page = source.repl.enter_fetches_next_page
        }
        "output.format" => target.output.format = source.output.format.clone(),
        "output.padding" => target.output.padding = source.output.padding,
        "output.table_style" => target.output.table_style = source.output.table_style.clone(),
        _ => {}
    }
}

struct ConfigSourceResolutionContext<'a> {
    system_path: &'a Path,
    system_toml: Option<&'a toml::Value>,
    user_path: &'a Path,
    user_toml: Option<&'a toml::Value>,
    custom_path: Option<&'a Path>,
    custom_toml: Option<&'a toml::Value>,
    matches: Option<&'a ArgMatches>,
}

fn resolve_config_source(
    descriptor: &ConfigKeyDescriptor,
    context: &ConfigSourceResolutionContext<'_>,
) -> (ConfigSource, Option<String>) {
    let mut source = (ConfigSource::Default, None);

    if toml_has_key(context.system_toml, descriptor.key) {
        source = (
            ConfigSource::SystemFile,
            Some(context.system_path.display().to_string()),
        );
    }
    if toml_has_key(context.user_toml, descriptor.key) {
        source = (
            ConfigSource::UserFile,
            Some(context.user_path.display().to_string()),
        );
    }
    if std::env::var_os(descriptor.env_var).is_some() {
        source = (
            ConfigSource::Environment,
            Some(descriptor.env_var.to_string()),
        );
    }
    if let (Some(path), true) = (
        context.custom_path,
        toml_has_key(context.custom_toml, descriptor.key),
    ) {
        source = (ConfigSource::CustomFile, Some(path.display().to_string()));
    }
    if let Some(arg) = descriptor.cli_arg {
        if context
            .matches
            .and_then(|matches| matches.value_source(arg))
            == Some(ValueSource::CommandLine)
        {
            source = (
                ConfigSource::CliOption,
                cli_flag_name(arg).map(str::to_string),
            );
        }
    }

    source
}

fn cli_flag_name(arg: &str) -> Option<&'static str> {
    match arg {
        "hostname" => Some("--hostname"),
        "port" => Some("--port"),
        "protocol" => Some("--protocol"),
        "ssl_validation" => Some("--ssl-validation"),
        "username" => Some("--username"),
        "password" => Some("--password"),
        "cache_time" => Some("--cache-time"),
        "cache_size" => Some("--cache-size"),
        "cache_disable" => Some("--cache-disable"),
        "completion_disable_api" => Some("--completion-api-disable"),
        "background_poll_interval" => Some("--background-poll-interval"),
        _ => None,
    }
}

fn config_value<'a>(config: &'a AppConfig, key: &str) -> ConfigValueRef<'a> {
    match key {
        "server.hostname" => ConfigValueRef::String(&config.server.hostname),
        "server.port" => ConfigValueRef::U16(config.server.port),
        "server.ssl_validation" => ConfigValueRef::Bool(config.server.ssl_validation),
        "server.api_version" => ConfigValueRef::String(&config.server.api_version),
        "server.username" => ConfigValueRef::String(&config.server.username),
        "server.password" => ConfigValueRef::OptionalString(config.server.password.as_deref()),
        "server.protocol" => ConfigValueRef::Protocol(&config.server.protocol),
        "cache.time" => ConfigValueRef::U64(config.cache.time),
        "cache.size" => ConfigValueRef::I32(config.cache.size),
        "cache.disable" => ConfigValueRef::Bool(config.cache.disable),
        "completion.disable_api_related" => {
            ConfigValueRef::Bool(config.completion.disable_api_related)
        }
        "background.poll_interval_seconds" => {
            ConfigValueRef::U64(config.background.poll_interval_seconds)
        }
        "repl.enter_fetches_next_page" => ConfigValueRef::Bool(config.repl.enter_fetches_next_page),
        "output.format" => ConfigValueRef::OutputFormat(&config.output.format),
        "output.padding" => ConfigValueRef::I8(config.output.padding),
        "output.table_style" => ConfigValueRef::TableStyle(&config.output.table_style),
        _ => ConfigValueRef::String(""),
    }
}

enum ConfigValueRef<'a> {
    String(&'a str),
    OptionalString(Option<&'a str>),
    Bool(bool),
    U16(u16),
    U64(u64),
    I8(i8),
    I32(i32),
    Protocol(&'a Protocol),
    OutputFormat(&'a OutputFormat),
    TableStyle(&'a TableStyle),
}

fn display_config_value(value: ConfigValueRef<'_>, sensitive: bool) -> String {
    if sensitive {
        return match value {
            ConfigValueRef::OptionalString(Some(_)) | ConfigValueRef::String(_) => {
                "********".into()
            }
            _ => "<unset>".into(),
        };
    }

    match value {
        ConfigValueRef::String(value) => value.to_string(),
        ConfigValueRef::OptionalString(Some(value)) => value.to_string(),
        ConfigValueRef::OptionalString(None) => "<unset>".to_string(),
        ConfigValueRef::Bool(value) => value.to_string(),
        ConfigValueRef::U16(value) => value.to_string(),
        ConfigValueRef::U64(value) => value.to_string(),
        ConfigValueRef::I8(value) => value.to_string(),
        ConfigValueRef::I32(value) => value.to_string(),
        ConfigValueRef::Protocol(value) => value.to_string(),
        ConfigValueRef::OutputFormat(value) => match value {
            OutputFormat::Json => "json".to_string(),
            OutputFormat::Text => "text".to_string(),
        },
        ConfigValueRef::TableStyle(value) => value.to_string(),
    }
}

fn read_toml_file(path: &Path) -> Option<toml::Value> {
    let contents = std::fs::read_to_string(path).ok()?;
    if contents.trim().is_empty() {
        return Some(toml::Value::Table(toml::map::Map::new()));
    }
    toml::from_str(&contents).ok()
}

fn toml_has_key(root: Option<&toml::Value>, key: &str) -> bool {
    root.and_then(|value| toml_get(value, key)).is_some()
}

fn toml_get<'a>(value: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn parse_config_value(
    descriptor: &ConfigKeyDescriptor,
    value: &str,
) -> Result<toml::Value, AppError> {
    let value = match descriptor.value_kind {
        ConfigValueKind::String => toml::Value::String(value.to_string()),
        ConfigValueKind::Bool => toml::Value::Boolean(value.parse()?),
        ConfigValueKind::U16 => toml::Value::Integer(value.parse::<u16>()?.into()),
        ConfigValueKind::U64 => toml::Value::Integer(value.parse::<u64>()? as i64),
        ConfigValueKind::I8 => toml::Value::Integer(i64::from(value.parse::<i8>()?)),
        ConfigValueKind::I32 => toml::Value::Integer(i64::from(value.parse::<i32>()?)),
        ConfigValueKind::Protocol => toml::Value::String(
            value
                .parse::<Protocol>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::OutputFormat => {
            toml::Value::String(parse_output_format(value)?.to_string().to_lowercase())
        }
        ConfigValueKind::TableStyle => toml::Value::String(
            value
                .parse::<TableStyle>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
    };
    Ok(value)
}

fn parse_output_format(value: &str) -> Result<OutputFormat, AppError> {
    match value.to_lowercase().as_str() {
        "json" => Ok(OutputFormat::Json),
        "text" => Ok(OutputFormat::Text),
        _ => Err(AppError::ConfigError(format!(
            "Invalid output format: {value}. Use 'json' or 'text'."
        ))),
    }
}

fn set_toml_path(root: &mut toml::Value, key: &str, value: toml::Value) -> Result<(), AppError> {
    let mut current = root;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let Some(table) = current.as_table_mut() else {
                return Err(AppError::ConfigError(
                    "Config root is not a TOML table".to_string(),
                ));
            };
            table.insert(part.to_string(), value);
            return Ok(());
        }

        let Some(table) = current.as_table_mut() else {
            return Err(AppError::ConfigError(
                "Config root is not a TOML table".to_string(),
            ));
        };
        current = table
            .entry(part.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }
    Ok(())
}

fn remove_toml_path(root: &mut toml::Value, key: &str) {
    let parts = key.split('.').collect::<Vec<_>>();
    remove_toml_path_parts(root, &parts);
}

fn remove_toml_path_parts(current: &mut toml::Value, parts: &[&str]) -> bool {
    let Some(table) = current.as_table_mut() else {
        return false;
    };

    if parts.len() == 1 {
        table.remove(parts[0]);
        return table.is_empty();
    }

    let should_remove_child = table
        .get_mut(parts[0])
        .map(|child| remove_toml_path_parts(child, &parts[1..]))
        .unwrap_or(false);

    if should_remove_child {
        table.remove(parts[0]);
    }

    table.is_empty()
}

fn write_toml_file(path: &Path, root: &toml::Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let rendered =
        toml::to_string_pretty(root).map_err(|err| AppError::ConfigError(err.to_string()))?;
    std::fs::write(path, rendered)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{build_cli, update_config_from_cli};
    use crate::models::Protocol;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TestEnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
        _temp_home: Option<TempDir>,
    }

    impl TestEnvGuard {
        fn new() -> Self {
            Self {
                saved: Vec::new(),
                _temp_home: None,
            }
        }

        fn set(&mut self, key: &'static str, value: String) {
            self.saved.push((key, env::var(key).ok()));
            env::set_var(key, value);
        }

        fn isolate_config_home() -> Self {
            let mut guard = Self::new();
            let temp_home = TempDir::new().expect("temp home should be created");
            guard.set("HOME", temp_home.path().to_string_lossy().to_string());
            guard.set(
                "XDG_CONFIG_HOME",
                temp_home
                    .path()
                    .join("config")
                    .to_string_lossy()
                    .to_string(),
            );
            guard.set(
                "XDG_DATA_HOME",
                temp_home.path().join("data").to_string_lossy().to_string(),
            );
            guard._temp_home = Some(temp_home);
            guard
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            while let Some((key, value)) = self.saved.pop() {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }

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
            "HUBUUM_CLI__BACKGROUND__POLL_INTERVAL_SECONDS",
            "HUBUUM_CLI__REPL__ENTER_FETCHES_NEXT_PAGE",
            "HUBUUM_CLI__OUTPUT__TABLE_STYLE",
        ] {
            env::remove_var(var);
        }
    }

    #[test]
    #[serial]
    fn env_overrides_entire_config() {
        clear_env();
        let _guard = TestEnvGuard::isolate_config_home();
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
        env::set_var("HUBUUM_CLI__BACKGROUND__POLL_INTERVAL_SECONDS", "7");
        env::set_var("HUBUUM_CLI__REPL__ENTER_FETCHES_NEXT_PAGE", "true");

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
        assert_eq!(cfg.background.poll_interval_seconds, 7);
        assert!(cfg.repl.enter_fetches_next_page);
        clear_env();
    }

    #[test]
    #[serial]
    fn mixing_env_and_defaults() {
        clear_env();
        let _guard = TestEnvGuard::isolate_config_home();
        // Only override one value
        env::set_var("HUBUUM_CLI__SERVER__PORT", "5555");
        let cfg = load_config(None).unwrap();

        // port should be env value, everything else falls back to Default::default()
        assert_eq!(cfg.server.port, 5555);
        assert_eq!(cfg.server.hostname, Defaults::SERVER_HOSTNAME);
        assert_eq!(cfg.cache.disable, Defaults::CACHE_DISABLE);
        assert_eq!(
            cfg.background.poll_interval_seconds,
            Defaults::BACKGROUND_POLL_INTERVAL_SECONDS
        );
        assert_eq!(
            cfg.repl.enter_fetches_next_page,
            Defaults::REPL_ENTER_FETCHES_NEXT_PAGE
        );
        assert!(cfg.server.password.is_none());

        clear_env();
    }

    #[test]
    #[serial]
    fn source_resolution_prefers_env_over_user_file() {
        clear_env();
        env::set_var("HUBUUM_CLI__SERVER__HOSTNAME", "env.example.com");

        let descriptor = descriptor_for_key("server.hostname").expect("missing descriptor");
        let user_path = Path::new("/tmp/user.toml");
        let user_toml: toml::Value = toml::from_str(
            r#"
            [server]
            hostname = "user.example.com"
            "#,
        )
        .expect("valid user toml");

        let context = ConfigSourceResolutionContext {
            system_path: Path::new("/tmp/system.toml"),
            system_toml: None,
            user_path,
            user_toml: Some(&user_toml),
            custom_path: None,
            custom_toml: None,
            matches: None,
        };
        let (source, detail) = resolve_config_source(descriptor, &context);

        assert_eq!(source, ConfigSource::Environment);
        assert_eq!(detail.as_deref(), Some("HUBUUM_CLI__SERVER__HOSTNAME"));

        clear_env();
    }

    #[test]
    #[serial]
    fn reload_runtime_config_preserves_cli_overrides_while_applying_persisted_changes() {
        clear_env();
        let _guard = TestEnvGuard::isolate_config_home();

        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--hostname",
                "localhost",
                "--port",
                "7070",
                "--username",
                "admin",
            ])
            .expect("cli should parse");

        let mut config = load_config(None).expect("config should load");
        update_config_from_cli(&mut config, &matches);
        init_config_state(inspect_config_state(&config, None, &matches))
            .expect("config state should initialize");
        init_config(config).expect("config should initialize");

        set_persisted_value("repl.enter_fetches_next_page", "true")
            .expect("persisted value should save");
        reload_runtime_config().expect("runtime config should reload");

        let reloaded = get_config();
        assert_eq!(reloaded.server.hostname, "localhost");
        assert_eq!(reloaded.server.port, 7070);
        assert_eq!(reloaded.server.username, "admin");
        assert!(reloaded.repl.enter_fetches_next_page);

        let state = get_config_state();
        assert_eq!(
            state.entry("server.username").map(|entry| &entry.source),
            Some(&ConfigSource::CliOption)
        );
        assert_eq!(
            state
                .entry("server.port")
                .and_then(|entry| entry.source_detail.as_deref()),
            Some("--port")
        );
        assert_eq!(
            state
                .entry("repl.enter_fetches_next_page")
                .map(|entry| &entry.source),
            Some(&ConfigSource::UserFile)
        );
        clear_env();
    }
}
