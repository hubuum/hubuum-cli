use clap::{parser::ValueSource, ArgMatches};
use config::{Config, ConfigError, Environment, File};
use hubuum_theme::{catalog as theme_catalog, theme_names};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::to_string as to_json_string;
use std::collections::{HashMap, HashSet};
use std::env::var_os;
use std::fs::{create_dir_all, read_to_string, write};
use std::mem::take;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use toml::map::Map as TomlMap;
use toml::{from_str as parse_toml, to_string_pretty as format_toml, Value as TomlValue};

use crate::defaults::Defaults;
use crate::domain::ComputedFieldSet;
use crate::errors::AppError;
use crate::files::{get_system_config_path, get_user_config_path};
use crate::models::{
    EmptyResult, ObjectListDataColumns, OutputColor, OutputFormat, Protocol, TableBands,
    TableStyle, TableWidth, TableWrap,
};

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
    #[serde(default)]
    pub settings: SettingsConfig,
    pub completion: CompletionConfig,
    pub background: BackgroundConfig,
    pub repl: ReplConfig,
    pub relations: RelationsConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SettingsConfig {
    pub store_on_server: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserPreferences {
    pub completion: CompletionConfig,
    pub background: BackgroundConfig,
    pub repl: ReplConfig,
    pub relations: RelationsConfig,
    pub output: UserOutputPreferences,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserOutputPreferences {
    pub format: OutputFormat,
    pub color: OutputColor,
    pub theme: String,
    pub padding: i8,
    pub table_style: TableStyle,
    pub table_width: TableWidth,
    pub table_wrap: TableWrap,
    pub table_bands: TableBands,
    pub empty_result: EmptyResult,
    pub object_show_data: bool,
    pub object_list_data_columns: ObjectListDataColumns,
    pub object_list_class_columns: HashMap<String, Vec<String>>,
    #[serde(default, alias = "object_list_class_meta")]
    pub object_list_class_aliases: HashMap<String, HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub object_class_computed_fields: HashMap<String, ComputedFieldSet>,
}

impl UserPreferences {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            completion: config.completion.clone(),
            background: config.background.clone(),
            repl: config.repl.clone(),
            relations: config.relations.clone(),
            output: UserOutputPreferences {
                format: config.output.format.clone(),
                color: config.output.color,
                theme: config.output.theme.clone(),
                padding: config.output.padding,
                table_style: config.output.table_style.clone(),
                table_width: config.output.table_width.clone(),
                table_wrap: config.output.table_wrap.clone(),
                table_bands: config.output.table_bands,
                empty_result: config.output.empty_result,
                object_show_data: config.output.object_show_data,
                object_list_data_columns: config.output.object_list_data_columns,
                object_list_class_columns: config.output.object_list_class_columns.clone(),
                object_list_class_aliases: config.output.object_list_class_aliases.clone(),
                object_class_computed_fields: config.output.object_class_computed_fields.clone(),
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub ssl_validation: bool,
    pub api_version: String,
    #[serde(default)]
    pub identity_scope: Option<String>,
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
pub struct RelationsConfig {
    pub ignore_same_class: bool,
    pub max_depth: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub color: OutputColor,
    pub theme: String,
    pub theme_file: String,
    pub padding: i8,
    pub table_style: TableStyle,
    pub table_width: TableWidth,
    pub table_wrap: TableWrap,
    pub table_bands: TableBands,
    pub empty_result: EmptyResult,
    pub object_show_data: bool,
    pub object_list_data_columns: ObjectListDataColumns,
    #[serde(default)]
    pub object_list_class_columns: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub object_list_class_aliases: HashMap<String, HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub object_class_computed_fields: HashMap<String, ComputedFieldSet>,
    #[serde(default, rename = "object_list_class_meta", skip_serializing)]
    legacy_object_list_class_meta: HashMap<String, HashMap<String, Vec<String>>>,
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
    OutputColor,
    ThemeName,
    TableStyle,
    TableWidth,
    TableWrap,
    TableBands,
    EmptyResult,
    ObjectListDataColumns,
    StringListMap,
    StringNestedListMap,
    ComputedFieldSetMap,
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
        key: "settings.store_on_server",
        cli_arg: None,
        env_var: "HUBUUM_CLI__SETTINGS__STORE_ON_SERVER",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
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
        key: "server.identity_scope",
        cli_arg: Some("identity_scope"),
        env_var: "HUBUUM_CLI__SERVER__IDENTITY_SCOPE",
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
        key: "relations.ignore_same_class",
        cli_arg: Some("relations_ignore_same_class"),
        env_var: "HUBUUM_CLI__RELATIONS__IGNORE_SAME_CLASS",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "relations.max_depth",
        cli_arg: Some("relations_max_depth"),
        env_var: "HUBUUM_CLI__RELATIONS__MAX_DEPTH",
        value_kind: ConfigValueKind::I32,
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
        key: "output.color",
        cli_arg: Some("color"),
        env_var: "HUBUUM_CLI__OUTPUT__COLOR",
        value_kind: ConfigValueKind::OutputColor,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.theme",
        cli_arg: Some("theme"),
        env_var: "HUBUUM_CLI__OUTPUT__THEME",
        value_kind: ConfigValueKind::ThemeName,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.theme_file",
        cli_arg: Some("theme_file"),
        env_var: "HUBUUM_CLI__OUTPUT__THEME_FILE",
        value_kind: ConfigValueKind::String,
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
        cli_arg: Some("table_style"),
        env_var: "HUBUUM_CLI__OUTPUT__TABLE_STYLE",
        value_kind: ConfigValueKind::TableStyle,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.table_width",
        cli_arg: Some("table_width"),
        env_var: "HUBUUM_CLI__OUTPUT__TABLE_WIDTH",
        value_kind: ConfigValueKind::TableWidth,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.table_wrap",
        cli_arg: Some("table_wrap"),
        env_var: "HUBUUM_CLI__OUTPUT__TABLE_WRAP",
        value_kind: ConfigValueKind::TableWrap,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.table_bands",
        cli_arg: Some("table_bands"),
        env_var: "HUBUUM_CLI__OUTPUT__TABLE_BANDS",
        value_kind: ConfigValueKind::TableBands,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.empty_result",
        cli_arg: Some("empty_result"),
        env_var: "HUBUUM_CLI__OUTPUT__EMPTY_RESULT",
        value_kind: ConfigValueKind::EmptyResult,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.object_show_data",
        cli_arg: Some("output_object_show_data"),
        env_var: "HUBUUM_CLI__OUTPUT__OBJECT_SHOW_DATA",
        value_kind: ConfigValueKind::Bool,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.object_list_data_columns",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__OBJECT_LIST_DATA_COLUMNS",
        value_kind: ConfigValueKind::ObjectListDataColumns,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.object_list_class_columns",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_COLUMNS",
        value_kind: ConfigValueKind::StringListMap,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.object_list_class_aliases",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_ALIASES",
        value_kind: ConfigValueKind::StringNestedListMap,
        sensitive: false,
    },
    ConfigKeyDescriptor {
        key: "output.object_class_computed_fields",
        cli_arg: None,
        env_var: "HUBUUM_CLI__OUTPUT__OBJECT_CLASS_COMPUTED_FIELDS",
        value_kind: ConfigValueKind::ComputedFieldSetMap,
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
                identity_scope: None,
                username: Defaults::USER_USERNAME.to_string(),
                password: None,
                protocol: Defaults::PROTOCOL,
            },
            cache: CacheConfig {
                time: Defaults::CACHE_TIME,
                size: Defaults::CACHE_SIZE,
                disable: Defaults::CACHE_DISABLE,
            },
            settings: SettingsConfig::default(),
            completion: CompletionConfig {
                disable_api_related: Defaults::COMPLETION_DISABLE_API_RELATED,
            },
            background: BackgroundConfig {
                poll_interval_seconds: Defaults::BACKGROUND_POLL_INTERVAL_SECONDS,
            },
            repl: ReplConfig {
                enter_fetches_next_page: Defaults::REPL_ENTER_FETCHES_NEXT_PAGE,
            },
            relations: RelationsConfig {
                ignore_same_class: Defaults::RELATIONS_IGNORE_SAME_CLASS,
                max_depth: Defaults::RELATIONS_MAX_DEPTH,
            },
            output: OutputConfig {
                format: Defaults::OUTPUT_FORMAT,
                color: Defaults::OUTPUT_COLOR,
                theme: Defaults::OUTPUT_THEME.to_string(),
                theme_file: Defaults::OUTPUT_THEME_FILE.to_string(),
                padding: Defaults::OUTPUT_PADDING,
                table_style: Defaults::OUTPUT_TABLE_STYLE,
                table_width: Defaults::OUTPUT_TABLE_WIDTH,
                table_wrap: Defaults::OUTPUT_TABLE_WRAP,
                table_bands: Defaults::OUTPUT_TABLE_BANDS,
                empty_result: Defaults::OUTPUT_EMPTY_RESULT,
                object_show_data: Defaults::OUTPUT_OBJECT_SHOW_DATA,
                object_list_data_columns: Defaults::OUTPUT_OBJECT_LIST_DATA_COLUMNS,
                object_list_class_columns: HashMap::new(),
                object_list_class_aliases: HashMap::new(),
                object_class_computed_fields: HashMap::new(),
                legacy_object_list_class_meta: HashMap::new(),
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

pub fn is_user_preference_key(key: &str) -> bool {
    (key.starts_with("completion.")
        || key.starts_with("background.")
        || key.starts_with("repl.")
        || key.starts_with("relations.")
        || key.starts_with("output."))
        && key != "output.theme_file"
}

pub fn config_value_candidates(key: &str) -> Vec<String> {
    let Ok(descriptor) = descriptor_for_key(key) else {
        return Vec::new();
    };

    match descriptor.value_kind {
        ConfigValueKind::Bool => strings(&["true", "false"]),
        ConfigValueKind::Protocol => strings(&["http", "https"]),
        ConfigValueKind::OutputFormat => strings(&["text", "json"]),
        ConfigValueKind::OutputColor => strings(&["auto", "always", "never"]),
        ConfigValueKind::ThemeName => theme_value_candidates(),
        ConfigValueKind::TableStyle => {
            strings(&["ascii", "compact", "dense", "markdown", "plain", "rounded"])
        }
        ConfigValueKind::TableWidth => strings(&["auto", "full"]),
        ConfigValueKind::TableWrap => strings(&["auto", "never"]),
        ConfigValueKind::TableBands => strings(&["auto", "always", "never"]),
        ConfigValueKind::EmptyResult => strings(&["message", "silent"]),
        ConfigValueKind::ObjectListDataColumns => strings(&["auto", "preview", "all"]),
        ConfigValueKind::StringListMap
        | ConfigValueKind::StringNestedListMap
        | ConfigValueKind::ComputedFieldSetMap => Vec::new(),
        ConfigValueKind::String
        | ConfigValueKind::U16
        | ConfigValueKind::U64
        | ConfigValueKind::I8
        | ConfigValueKind::I32 => Vec::new(),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

pub fn theme_value_candidates() -> Vec<String> {
    let cfg = get_config();
    let theme_file =
        (!cfg.output.theme_file.is_empty()).then_some(Path::new(&cfg.output.theme_file));
    match theme_catalog(theme_file) {
        Ok(catalog) => catalog.names().into_iter().map(str::to_string).collect(),
        Err(_) => theme_names().into_iter().collect(),
    }
}

pub fn inspect_config_state(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
    matches: &ArgMatches,
) -> ConfigState {
    inspect_config_state_inner(config, cli_config_path, None, Some(matches))
}

fn inspect_config_state_inner(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
    runtime_cli_args: Option<&HashSet<String>>,
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
        runtime_cli_args,
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
    if let Some(class_name) = object_list_class_columns_key(key) {
        return set_persisted_object_list_class_columns(class_name, value);
    }
    if let Some(class_name) = object_class_computed_fields_key(key) {
        return set_persisted_object_class_computed_fields(class_name, value);
    }
    if let Some((class_name, alias)) = object_list_class_alias_key(key) {
        return set_persisted_object_list_class_alias(class_name, alias, value);
    }
    let descriptor = descriptor_for_key(key)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
    let parsed = parse_config_value(descriptor, value)?;
    set_toml_path(&mut root, descriptor.key, parsed)?;
    if descriptor.key == "output.object_list_class_aliases" {
        remove_toml_path(&mut root, "output.object_list_class_meta");
    }
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn unset_persisted_value(key: &str) -> Result<PathBuf, AppError> {
    if object_list_class_columns_key(key).is_some()
        || object_class_computed_fields_key(key).is_some()
        || object_list_class_alias_key(key).is_some()
    {
        let path = get_config_state().paths.write_target.clone();
        let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
        if let Some((class_name, alias)) = object_list_class_alias_key(key) {
            remove_toml_path(
                &mut root,
                &format!("output.object_list_class_aliases.{class_name}.{alias}"),
            );
            remove_toml_path(
                &mut root,
                &format!("output.object_list_class_meta.{class_name}.{alias}"),
            );
        } else {
            remove_toml_path(&mut root, key);
        }
        write_toml_file(&path, &root)?;
        return Ok(path);
    }
    let descriptor = descriptor_for_key(key)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
    remove_toml_path(&mut root, descriptor.key);
    if descriptor.key == "output.object_list_class_aliases" {
        remove_toml_path(&mut root, "output.object_list_class_meta");
    }
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn persist_user_preferences(preferences: &UserPreferences) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
    merge_user_preferences(&mut root, preferences)?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

fn merge_user_preferences(
    root: &mut TomlValue,
    preferences: &UserPreferences,
) -> Result<(), AppError> {
    let serialized = TomlValue::try_from(preferences)
        .map_err(|error| AppError::ConfigError(error.to_string()))?;
    let preference_sections = serialized.as_table().ok_or_else(|| {
        AppError::ConfigError("Serialized user preferences must be a TOML table".to_string())
    })?;
    let target = root
        .as_table_mut()
        .ok_or_else(|| AppError::ConfigError("Config root is not a TOML table".to_string()))?;

    for section in ["completion", "background", "repl", "relations"] {
        if let Some(value) = preference_sections.get(section) {
            target.insert(section.to_string(), value.clone());
        }
    }
    let output = preference_sections
        .get("output")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| {
            AppError::ConfigError("Serialized output preferences must be a TOML table".to_string())
        })?;
    let target_output = target
        .entry("output".to_string())
        .or_insert_with(|| TomlValue::Table(TomlMap::new()))
        .as_table_mut()
        .ok_or_else(|| AppError::ConfigError("Config output must be a TOML table".to_string()))?;
    for (key, value) in output {
        target_output.insert(key.clone(), value.clone());
    }
    target_output.remove("object_list_class_meta");
    Ok(())
}

pub fn reload_runtime_config() -> Result<(), AppError> {
    let previous_state = get_config_state();
    let custom = previous_state.paths.custom.clone();
    let runtime_cli_keys: Vec<String> = previous_state
        .entries
        .iter()
        .filter(|entry| entry.source == ConfigSource::CliOption)
        .map(|entry| entry.key.clone())
        .collect();

    let mut config = load_config(custom.clone())?;
    if !runtime_cli_keys.is_empty() {
        let previous_config = get_config();
        apply_runtime_overrides(&mut config, &previous_config, &runtime_cli_keys);
    }

    let runtime_cli_args: HashSet<String> = runtime_cli_keys
        .iter()
        .filter_map(|key| {
            descriptor_for_key(key)
                .ok()
                .and_then(|descriptor| descriptor.cli_arg)
        })
        .map(str::to_string)
        .collect();

    let state = if runtime_cli_args.is_empty() {
        inspect_config_state_without_cli(&config, custom)
    } else {
        inspect_config_state_with_runtime_cli(&config, custom, &runtime_cli_args)
    };
    init_config_state(state)?;
    init_config(config)?;
    Ok(())
}

fn apply_runtime_overrides(target: &mut AppConfig, source: &AppConfig, keys: &[String]) {
    for key in keys {
        match key.as_str() {
            "server.hostname" => target.server.hostname = source.server.hostname.clone(),
            "server.port" => target.server.port = source.server.port,
            "server.ssl_validation" => target.server.ssl_validation = source.server.ssl_validation,
            "server.identity_scope" => {
                target.server.identity_scope = source.server.identity_scope.clone();
            }
            "server.username" => target.server.username = source.server.username.clone(),
            "server.password" => target.server.password = source.server.password.clone(),
            "server.protocol" => target.server.protocol = source.server.protocol.clone(),
            "cache.time" => target.cache.time = source.cache.time,
            "cache.size" => target.cache.size = source.cache.size,
            "cache.disable" => target.cache.disable = source.cache.disable,
            "completion.disable_api_related" => {
                target.completion.disable_api_related = source.completion.disable_api_related;
            }
            "background.poll_interval_seconds" => {
                target.background.poll_interval_seconds = source.background.poll_interval_seconds;
            }
            "relations.ignore_same_class" => {
                target.relations.ignore_same_class = source.relations.ignore_same_class;
            }
            "relations.max_depth" => target.relations.max_depth = source.relations.max_depth,
            "output.object_show_data" => {
                target.output.object_show_data = source.output.object_show_data;
            }
            "output.object_list_data_columns" => {
                target.output.object_list_data_columns = source.output.object_list_data_columns;
            }
            "output.object_list_class_columns" => {
                target.output.object_list_class_columns =
                    source.output.object_list_class_columns.clone();
            }
            "output.object_list_class_aliases" => {
                target.output.object_list_class_aliases =
                    source.output.object_list_class_aliases.clone();
            }
            "output.object_class_computed_fields" => {
                target.output.object_class_computed_fields =
                    source.output.object_class_computed_fields.clone();
            }
            "output.color" => target.output.color = source.output.color,
            "output.theme" => target.output.theme = source.output.theme.clone(),
            "output.theme_file" => target.output.theme_file = source.output.theme_file.clone(),
            "output.table_style" => target.output.table_style = source.output.table_style.clone(),
            "output.table_width" => target.output.table_width = source.output.table_width.clone(),
            "output.table_wrap" => target.output.table_wrap = source.output.table_wrap.clone(),
            "output.table_bands" => target.output.table_bands = source.output.table_bands,
            "output.empty_result" => target.output.empty_result = source.output.empty_result,
            _ => {}
        }
    }
}

pub fn load_config(cli_config_path: Option<PathBuf>) -> Result<AppConfig, ConfigError> {
    let system_config = get_system_config_path();
    let user_config = get_user_config_path();

    let mut builder = Config::builder()
        // Start with default values
        .set_default("output.format", Defaults::OUTPUT_FORMAT.to_string())?
        .set_default("output.color", Defaults::OUTPUT_COLOR.to_string())?
        .set_default("output.theme", Defaults::OUTPUT_THEME)?
        .set_default("output.theme_file", Defaults::OUTPUT_THEME_FILE)?
        .set_default("output.padding", Defaults::OUTPUT_PADDING)?
        .set_default(
            "output.table_style",
            Defaults::OUTPUT_TABLE_STYLE.to_string(),
        )?
        .set_default(
            "output.table_width",
            Defaults::OUTPUT_TABLE_WIDTH.to_string(),
        )?
        .set_default("output.table_wrap", Defaults::OUTPUT_TABLE_WRAP.to_string())?
        .set_default(
            "output.table_bands",
            Defaults::OUTPUT_TABLE_BANDS.to_string(),
        )?
        .set_default(
            "output.empty_result",
            Defaults::OUTPUT_EMPTY_RESULT.to_string(),
        )?
        .set_default(
            "output.object_list_data_columns",
            Defaults::OUTPUT_OBJECT_LIST_DATA_COLUMNS.to_string(),
        )?
        .set_default(
            "output.object_list_class_columns",
            HashMap::<String, Vec<String>>::new(),
        )?
        .set_default(
            "output.object_list_class_aliases",
            HashMap::<String, HashMap<String, Vec<String>>>::new(),
        )?
        .set_default(
            "output.object_class_computed_fields",
            HashMap::<String, Vec<String>>::new(),
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
        .set_default("settings.store_on_server", false)?
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
        .set_default(
            "relations.ignore_same_class",
            Defaults::RELATIONS_IGNORE_SAME_CLASS,
        )?
        .set_default("relations.max_depth", Defaults::RELATIONS_MAX_DEPTH)?
        // 1. Load system-wide config
        .set_default("output.object_show_data", Defaults::OUTPUT_OBJECT_SHOW_DATA)?
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
    let mut config: AppConfig = config.try_deserialize()?;
    merge_legacy_object_list_class_aliases(&mut config.output);
    Ok(config)
}

fn merge_legacy_object_list_class_aliases(output: &mut OutputConfig) {
    for (class_name, aliases) in take(&mut output.legacy_object_list_class_meta) {
        let target = output
            .object_list_class_aliases
            .entry(class_name)
            .or_default();
        for (alias, selectors) in aliases {
            target.entry(alias).or_insert(selectors);
        }
    }
}

pub fn inspect_config_state_without_cli(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
) -> ConfigState {
    inspect_config_state_inner(config, cli_config_path, None, None)
}

fn inspect_config_state_with_runtime_cli(
    config: &AppConfig,
    cli_config_path: Option<PathBuf>,
    runtime_cli_args: &HashSet<String>,
) -> ConfigState {
    inspect_config_state_inner(config, cli_config_path, Some(runtime_cli_args), None)
}

fn descriptor_for_key(key: &str) -> Result<&'static ConfigKeyDescriptor, AppError> {
    let key = match key {
        "output.object_list_class_meta" => "output.object_list_class_aliases",
        _ => key,
    };
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

struct ConfigSourceResolutionContext<'a> {
    system_path: &'a Path,
    system_toml: Option<&'a TomlValue>,
    user_path: &'a Path,
    user_toml: Option<&'a TomlValue>,
    custom_path: Option<&'a Path>,
    custom_toml: Option<&'a TomlValue>,
    runtime_cli_args: Option<&'a HashSet<String>>,
    matches: Option<&'a ArgMatches>,
}

fn resolve_config_source(
    descriptor: &ConfigKeyDescriptor,
    context: &ConfigSourceResolutionContext<'_>,
) -> (ConfigSource, Option<String>) {
    let mut source = (ConfigSource::Default, None);

    if toml_has_descriptor_key(context.system_toml, descriptor.key) {
        source = (
            ConfigSource::SystemFile,
            Some(context.system_path.display().to_string()),
        );
    }
    if toml_has_descriptor_key(context.user_toml, descriptor.key) {
        source = (
            ConfigSource::UserFile,
            Some(context.user_path.display().to_string()),
        );
    }
    if let Some(env_var) = configured_descriptor_env_var(descriptor) {
        source = (ConfigSource::Environment, Some(env_var.to_string()));
    }
    if let (Some(path), true) = (
        context.custom_path,
        toml_has_descriptor_key(context.custom_toml, descriptor.key),
    ) {
        source = (ConfigSource::CustomFile, Some(path.display().to_string()));
    }
    if let Some(arg) = descriptor.cli_arg {
        if context
            .runtime_cli_args
            .is_some_and(|runtime_cli_args| runtime_cli_args.contains(arg))
        {
            source = (
                ConfigSource::CliOption,
                cli_flag_name(arg).map(str::to_string),
            );
        }
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

fn toml_has_descriptor_key(root: Option<&TomlValue>, key: &str) -> bool {
    toml_has_key(root, key)
        || (key == "output.object_list_class_aliases"
            && toml_has_key(root, "output.object_list_class_meta"))
}

fn configured_descriptor_env_var(descriptor: &ConfigKeyDescriptor) -> Option<&'static str> {
    if var_os(descriptor.env_var).is_some() {
        return Some(descriptor.env_var);
    }
    (descriptor.key == "output.object_list_class_aliases"
        && var_os("HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_META").is_some())
    .then_some("HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_META")
}

fn cli_flag_name(arg: &str) -> Option<&'static str> {
    match arg {
        "hostname" => Some("--hostname"),
        "port" => Some("--port"),
        "protocol" => Some("--protocol"),
        "ssl_validation" => Some("--ssl-validation"),
        "identity_scope" => Some("--identity-scope"),
        "username" => Some("--username"),
        "password" => Some("--password"),
        "cache_time" => Some("--cache-time"),
        "cache_size" => Some("--cache-size"),
        "cache_disable" => Some("--cache-disable"),
        "completion_disable_api" => Some("--completion-api-disable"),
        "background_poll_interval" => Some("--background-poll-interval"),
        "relations_ignore_same_class" => Some("--relations-ignore-same-class"),
        "relations_max_depth" => Some("--relations-max-depth"),
        "color" => Some("--color"),
        "theme" => Some("--theme"),
        "theme_file" => Some("--theme-file"),
        "table_style" => Some("--table-style"),
        "table_width" => Some("--table-width"),
        "table_wrap" => Some("--table-wrap"),
        "table_bands" => Some("--table-bands"),
        "empty_result" => Some("--empty-result"),
        "output_object_show_data" => Some("--output-object-show-data"),
        _ => None,
    }
}

fn config_value<'a>(config: &'a AppConfig, key: &str) -> ConfigValueRef<'a> {
    match key {
        "server.hostname" => ConfigValueRef::String(&config.server.hostname),
        "server.port" => ConfigValueRef::U16(config.server.port),
        "server.ssl_validation" => ConfigValueRef::Bool(config.server.ssl_validation),
        "server.api_version" => ConfigValueRef::String(&config.server.api_version),
        "server.identity_scope" => {
            ConfigValueRef::OptionalString(config.server.identity_scope.as_deref())
        }
        "server.username" => ConfigValueRef::String(&config.server.username),
        "server.password" => ConfigValueRef::OptionalString(config.server.password.as_deref()),
        "server.protocol" => ConfigValueRef::Protocol(&config.server.protocol),
        "cache.time" => ConfigValueRef::U64(config.cache.time),
        "cache.size" => ConfigValueRef::I32(config.cache.size),
        "cache.disable" => ConfigValueRef::Bool(config.cache.disable),
        "settings.store_on_server" => ConfigValueRef::Bool(config.settings.store_on_server),
        "completion.disable_api_related" => {
            ConfigValueRef::Bool(config.completion.disable_api_related)
        }
        "background.poll_interval_seconds" => {
            ConfigValueRef::U64(config.background.poll_interval_seconds)
        }
        "repl.enter_fetches_next_page" => ConfigValueRef::Bool(config.repl.enter_fetches_next_page),
        "relations.ignore_same_class" => ConfigValueRef::Bool(config.relations.ignore_same_class),
        "relations.max_depth" => ConfigValueRef::I32(config.relations.max_depth),
        "output.format" => ConfigValueRef::OutputFormat(&config.output.format),
        "output.color" => ConfigValueRef::OutputColor(&config.output.color),
        "output.theme" => ConfigValueRef::String(&config.output.theme),
        "output.theme_file" => ConfigValueRef::String(&config.output.theme_file),
        "output.padding" => ConfigValueRef::I8(config.output.padding),
        "output.table_style" => ConfigValueRef::TableStyle(&config.output.table_style),
        "output.table_width" => ConfigValueRef::TableWidth(&config.output.table_width),
        "output.table_wrap" => ConfigValueRef::TableWrap(&config.output.table_wrap),
        "output.table_bands" => ConfigValueRef::TableBands(&config.output.table_bands),
        "output.empty_result" => ConfigValueRef::EmptyResult(&config.output.empty_result),
        "output.object_show_data" => ConfigValueRef::Bool(config.output.object_show_data),
        "output.object_list_data_columns" => {
            ConfigValueRef::ObjectListDataColumns(&config.output.object_list_data_columns)
        }
        "output.object_list_class_columns" => {
            ConfigValueRef::StringListMap(&config.output.object_list_class_columns)
        }
        "output.object_list_class_aliases" => {
            ConfigValueRef::StringNestedListMap(&config.output.object_list_class_aliases)
        }
        "output.object_class_computed_fields" => {
            ConfigValueRef::ComputedFieldSetMap(&config.output.object_class_computed_fields)
        }
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
    OutputColor(&'a OutputColor),
    TableStyle(&'a TableStyle),
    TableWidth(&'a TableWidth),
    TableWrap(&'a TableWrap),
    TableBands(&'a TableBands),
    EmptyResult(&'a EmptyResult),
    ObjectListDataColumns(&'a ObjectListDataColumns),
    StringListMap(&'a HashMap<String, Vec<String>>),
    StringNestedListMap(&'a HashMap<String, HashMap<String, Vec<String>>>),
    ComputedFieldSetMap(&'a HashMap<String, ComputedFieldSet>),
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
        ConfigValueRef::OutputColor(value) => value.to_string(),
        ConfigValueRef::TableStyle(value) => value.to_string(),
        ConfigValueRef::TableWidth(value) => value.to_string(),
        ConfigValueRef::TableWrap(value) => value.to_string(),
        ConfigValueRef::TableBands(value) => value.to_string(),
        ConfigValueRef::EmptyResult(value) => value.to_string(),
        ConfigValueRef::ObjectListDataColumns(value) => value.to_string(),
        ConfigValueRef::StringListMap(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::StringNestedListMap(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::ComputedFieldSetMap(value) => to_json_string(value).unwrap_or_default(),
    }
}

fn read_toml_file(path: &Path) -> Option<TomlValue> {
    let contents = read_to_string(path).ok()?;
    if contents.trim().is_empty() {
        return Some(TomlValue::Table(TomlMap::new()));
    }
    parse_toml(&contents).ok()
}

fn toml_has_key(root: Option<&TomlValue>, key: &str) -> bool {
    root.and_then(|value| toml_get(value, key)).is_some()
}

fn toml_get<'a>(value: &'a TomlValue, key: &str) -> Option<&'a TomlValue> {
    let mut current = value;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn parse_config_value(
    descriptor: &ConfigKeyDescriptor,
    value: &str,
) -> Result<TomlValue, AppError> {
    let value = match descriptor.value_kind {
        ConfigValueKind::String => TomlValue::String(value.to_string()),
        ConfigValueKind::Bool => TomlValue::Boolean(value.parse()?),
        ConfigValueKind::U16 => TomlValue::Integer(value.parse::<u16>()?.into()),
        ConfigValueKind::U64 => TomlValue::Integer(value.parse::<u64>()? as i64),
        ConfigValueKind::I8 => TomlValue::Integer(i64::from(value.parse::<i8>()?)),
        ConfigValueKind::I32 => TomlValue::Integer(i64::from(value.parse::<i32>()?)),
        ConfigValueKind::Protocol => TomlValue::String(
            value
                .parse::<Protocol>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::OutputFormat => {
            TomlValue::String(parse_output_format(value)?.to_string().to_lowercase())
        }
        ConfigValueKind::OutputColor => TomlValue::String(
            value
                .parse::<OutputColor>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::ThemeName => {
            validate_theme_name_config_value(value)?;
            TomlValue::String(value.to_string())
        }
        ConfigValueKind::TableStyle => TomlValue::String(
            value
                .parse::<TableStyle>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableWidth => TomlValue::String(
            value
                .parse::<TableWidth>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableWrap => TomlValue::String(
            value
                .parse::<TableWrap>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableBands => TomlValue::String(
            value
                .parse::<TableBands>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::EmptyResult => TomlValue::String(
            value
                .parse::<EmptyResult>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::ObjectListDataColumns => TomlValue::String(
            value
                .parse::<ObjectListDataColumns>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::StringListMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
        ConfigValueKind::StringNestedListMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
        ConfigValueKind::ComputedFieldSetMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
    };
    Ok(value)
}

fn validate_theme_name_config_value(value: &str) -> Result<(), AppError> {
    let cfg = get_config();
    let theme_file =
        (!cfg.output.theme_file.is_empty()).then_some(Path::new(&cfg.output.theme_file));
    let catalog = theme_catalog(theme_file).map_err(|err| {
        AppError::ConfigError(format!("Could not load configured theme file: {err}"))
    })?;
    if catalog.get(value).is_some() {
        return Ok(());
    }
    Err(AppError::ConfigError(format!(
        "Unknown theme: {value}. Use one of: {}",
        catalog.names().join(", ")
    )))
}

fn object_list_class_columns_key(key: &str) -> Option<&str> {
    key.strip_prefix("output.object_list_class_columns.")
        .filter(|class_name| !class_name.is_empty())
}

fn object_class_computed_fields_key(key: &str) -> Option<&str> {
    key.strip_prefix("output.object_class_computed_fields.")
        .filter(|class_name| !class_name.is_empty())
}

fn object_list_class_alias_key(key: &str) -> Option<(&str, &str)> {
    let rest = key
        .strip_prefix("output.object_list_class_aliases.")
        .or_else(|| key.strip_prefix("output.object_list_class_meta."))?;
    let (class_name, alias) = rest.split_once('.')?;
    (!class_name.is_empty() && !alias.is_empty()).then_some((class_name, alias))
}

fn set_persisted_object_list_class_columns(
    class_name: &str,
    value: &str,
) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
    let columns = value
        .split(',')
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .map(|column| TomlValue::String(column.to_string()))
        .collect::<Vec<_>>();
    set_toml_path(
        &mut root,
        &format!("output.object_list_class_columns.{class_name}"),
        TomlValue::Array(columns),
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

fn set_persisted_object_class_computed_fields(
    class_name: &str,
    value: &str,
) -> Result<PathBuf, AppError> {
    let fields =
        ComputedFieldSet::from_values(&[value.to_string()]).map_err(AppError::ConfigError)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
    let fields = fields
        .selectors()
        .iter()
        .map(|field| TomlValue::String(field.to_string()))
        .collect();
    set_toml_path(
        &mut root,
        &format!("output.object_class_computed_fields.{class_name}"),
        TomlValue::Array(fields),
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

fn set_persisted_object_list_class_alias(
    class_name: &str,
    alias: &str,
    value: &str,
) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file(&path).unwrap_or(TomlValue::Table(TomlMap::new()));
    remove_toml_path(
        &mut root,
        &format!("output.object_list_class_meta.{class_name}.{alias}"),
    );
    let selectors = value
        .split(',')
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
        .map(|selector| TomlValue::String(selector.to_string()))
        .collect::<Vec<_>>();
    set_toml_path(
        &mut root,
        &format!("output.object_list_class_aliases.{class_name}.{alias}"),
        TomlValue::Array(selectors),
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
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

fn set_toml_path(root: &mut TomlValue, key: &str, value: TomlValue) -> Result<(), AppError> {
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
            .or_insert_with(|| TomlValue::Table(TomlMap::new()));
    }
    Ok(())
}

fn remove_toml_path(root: &mut TomlValue, key: &str) {
    let parts = key.split('.').collect::<Vec<_>>();
    remove_toml_path_parts(root, &parts);
}

fn remove_toml_path_parts(current: &mut TomlValue, parts: &[&str]) -> bool {
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

fn write_toml_file(path: &Path, root: &TomlValue) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let rendered = format_toml(root).map_err(|err| AppError::ConfigError(err.to_string()))?;
    write(path, rendered)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{build_cli, update_config_from_cli};
    use crate::models::{
        EmptyResult, ObjectListDataColumns, OutputColor, Protocol, TableBands, TableStyle,
        TableWidth, TableWrap,
    };
    use serial_test::serial;
    use std::env::{remove_var, set_var, temp_dir};
    use std::fs::{remove_file, write};
    use std::process::id;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;

    /// Helper to clear all HUBUUM_CLI_... vars we use in this test.
    fn clear_env() {
        for &var in &[
            "HUBUUM_CLI__SETTINGS__STORE_ON_SERVER",
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
            "HUBUUM_CLI__RELATIONS__IGNORE_SAME_CLASS",
            "HUBUUM_CLI__RELATIONS__MAX_DEPTH",
            "HUBUUM_CLI__OUTPUT__COLOR",
            "HUBUUM_CLI__OUTPUT__THEME",
            "HUBUUM_CLI__OUTPUT__THEME_FILE",
            "HUBUUM_CLI__OUTPUT__TABLE_STYLE",
            "HUBUUM_CLI__OUTPUT__TABLE_WIDTH",
            "HUBUUM_CLI__OUTPUT__TABLE_WRAP",
            "HUBUUM_CLI__OUTPUT__TABLE_BANDS",
            "HUBUUM_CLI__OUTPUT__EMPTY_RESULT",
            "HUBUUM_CLI__OUTPUT__OBJECT_SHOW_DATA",
            "HUBUUM_CLI__OUTPUT__OBJECT_LIST_DATA_COLUMNS",
            "HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_COLUMNS",
            "HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_ALIASES",
            "HUBUUM_CLI__OUTPUT__OBJECT_LIST_CLASS_META",
            "HUBUUM_CLI__OUTPUT__OBJECT_CLASS_COMPUTED_FIELDS",
        ] {
            remove_var(var);
        }
    }

    #[test]
    #[serial]
    fn env_overrides_entire_config() {
        clear_env();
        set_var("HUBUUM_CLI__SERVER__HOSTNAME", "env.example.com");
        set_var("HUBUUM_CLI__SERVER__PORT", "4321");
        set_var("HUBUUM_CLI__SERVER__SSL_VALIDATION", "false");
        set_var("HUBUUM_CLI__SERVER__API_VERSION", "v9");
        set_var("HUBUUM_CLI__SERVER__USERNAME", "env_user");
        set_var("HUBUUM_CLI__SERVER__PASSWORD", "hunter2");
        set_var("HUBUUM_CLI__SERVER__PROTOCOL", "http");

        set_var("HUBUUM_CLI__CACHE__TIME", "99");
        set_var("HUBUUM_CLI__CACHE__SIZE", "42");
        set_var("HUBUUM_CLI__CACHE__DISABLE", "true");

        set_var("HUBUUM_CLI__COMPLETION__DISABLE_API_RELATED", "true");
        set_var("HUBUUM_CLI__BACKGROUND__POLL_INTERVAL_SECONDS", "7");
        set_var("HUBUUM_CLI__REPL__ENTER_FETCHES_NEXT_PAGE", "true");
        set_var("HUBUUM_CLI__RELATIONS__IGNORE_SAME_CLASS", "false");
        set_var("HUBUUM_CLI__RELATIONS__MAX_DEPTH", "4");
        set_var("HUBUUM_CLI__OUTPUT__COLOR", "never");
        set_var("HUBUUM_CLI__OUTPUT__THEME", "solarized-dark");
        set_var("HUBUUM_CLI__OUTPUT__THEME_FILE", "/tmp/hubuum-themes.toml");
        set_var("HUBUUM_CLI__OUTPUT__TABLE_STYLE", "plain");
        set_var("HUBUUM_CLI__OUTPUT__TABLE_WIDTH", "100");
        set_var("HUBUUM_CLI__OUTPUT__TABLE_WRAP", "never");
        set_var("HUBUUM_CLI__OUTPUT__TABLE_BANDS", "always");
        set_var("HUBUUM_CLI__OUTPUT__EMPTY_RESULT", "silent");
        set_var("HUBUUM_CLI__OUTPUT__OBJECT_SHOW_DATA", "true");
        set_var("HUBUUM_CLI__OUTPUT__OBJECT_LIST_DATA_COLUMNS", "all");

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
        assert!(!cfg.relations.ignore_same_class);
        assert_eq!(cfg.relations.max_depth, 4);
        assert_eq!(cfg.output.color, OutputColor::Never);
        assert_eq!(cfg.output.theme, "solarized-dark");
        assert_eq!(cfg.output.theme_file, "/tmp/hubuum-themes.toml");
        assert_eq!(cfg.output.table_style, TableStyle::Plain);
        assert_eq!(cfg.output.table_width, TableWidth::Fixed(100));
        assert_eq!(cfg.output.table_wrap, TableWrap::Never);
        assert_eq!(cfg.output.table_bands, TableBands::Always);
        assert_eq!(cfg.output.empty_result, EmptyResult::Silent);
        assert!(cfg.output.object_show_data);
        assert_eq!(
            cfg.output.object_list_data_columns,
            ObjectListDataColumns::All
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn object_list_class_columns_load_from_toml() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"
[output.object_list_class_columns]
Hosts = ["contact", "jack", "data.name"]
"#,
        )
        .expect("write config");

        let cfg = load_config(Some(path)).expect("load config");

        assert_eq!(
            cfg.output.object_list_class_columns.get("Hosts"),
            Some(&vec![
                "contact".to_string(),
                "jack".to_string(),
                "data.name".to_string()
            ])
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn object_list_class_aliases_load_from_toml() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"
[output.object_list_class_aliases.Hosts]
os_version = ["data.os.macos.version", "data.os.redhat.version"]
"#,
        )
        .expect("write config");

        let cfg = load_config(Some(path)).expect("load config");

        assert_eq!(
            cfg.output
                .object_list_class_aliases
                .get("Hosts")
                .and_then(|aliases| aliases.get("os_version")),
            Some(&vec![
                "data.os.macos.version".to_string(),
                "data.os.redhat.version".to_string()
            ])
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn object_class_computed_fields_load_from_toml() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"
[output.object_class_computed_fields]
Hosts = ["S:os_version", "P:note"]
Everything = ["all"]
"#,
        )
        .expect("write config");

        let cfg = load_config(Some(path)).expect("load config");

        assert_eq!(
            cfg.output.object_class_computed_fields["Hosts"]
                .selectors()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["S:os_version", "P:note"]
        );
        assert!(cfg.output.object_class_computed_fields["Everything"].is_all());
        clear_env();
    }

    #[test]
    #[serial]
    fn object_class_computed_fields_reject_invalid_combinations() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"
[output.object_class_computed_fields]
Hosts = ["all", "S:os_version"]
"#,
        )
        .expect("write config");

        let error = load_config(Some(path)).expect_err("invalid defaults should fail");

        assert!(error.to_string().contains("cannot be combined"));
        clear_env();
    }

    #[test]
    #[serial]
    fn config_set_and_unset_persist_class_computed_fields() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        init_config_state(ConfigState {
            paths: ConfigPaths {
                system: dir.path().join("system.toml"),
                user: path.clone(),
                custom: Some(path.clone()),
                write_target: path.clone(),
            },
            entries: Vec::new(),
        })
        .expect("config state should initialize");

        set_persisted_value("output.object_class_computed_fields.Hosts", "S:load,P:note")
            .expect("computed defaults should persist");
        let configured = load_config(Some(path.clone())).expect("persisted config should load");
        assert_eq!(
            configured.output.object_class_computed_fields["Hosts"]
                .selectors()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["S:load", "P:note"]
        );

        unset_persisted_value("output.object_class_computed_fields.Hosts")
            .expect("computed defaults should be removed");
        let configured = load_config(Some(path)).expect("updated config should load");
        assert!(configured.output.object_class_computed_fields.is_empty());
        clear_env();
    }

    #[test]
    #[serial]
    fn legacy_object_list_class_meta_loads_as_display_aliases() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"
[output.object_list_class_meta.Hosts]
os_version = ["data.os.macos.version", "data.os.redhat.version"]
"#,
        )
        .expect("write config");

        let cfg = load_config(Some(path)).expect("load legacy config");

        assert_eq!(
            cfg.output
                .object_list_class_aliases
                .get("Hosts")
                .and_then(|aliases| aliases.get("os_version")),
            Some(&vec![
                "data.os.macos.version".to_string(),
                "data.os.redhat.version".to_string()
            ])
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn mixing_env_and_defaults() {
        clear_env();
        let baseline = load_config(None).unwrap();
        // Only override one value
        set_var("HUBUUM_CLI__SERVER__PORT", "5555");
        let cfg = load_config(None).unwrap();

        // port should be env value, everything else should match the non-env baseline
        assert_eq!(cfg.server.port, 5555);
        assert_eq!(cfg.server.hostname, baseline.server.hostname);
        assert_eq!(cfg.cache.disable, baseline.cache.disable);
        assert_eq!(
            cfg.background.poll_interval_seconds,
            baseline.background.poll_interval_seconds
        );
        assert_eq!(
            cfg.repl.enter_fetches_next_page,
            baseline.repl.enter_fetches_next_page
        );
        assert_eq!(
            cfg.relations.ignore_same_class,
            baseline.relations.ignore_same_class
        );
        assert_eq!(cfg.relations.max_depth, baseline.relations.max_depth);
        assert_eq!(
            cfg.output.object_show_data,
            baseline.output.object_show_data
        );
        assert_eq!(cfg.output.color, baseline.output.color);
        assert_eq!(cfg.output.theme, baseline.output.theme);
        assert_eq!(cfg.output.theme_file, baseline.output.theme_file);
        assert_eq!(cfg.output.table_style, baseline.output.table_style);
        assert_eq!(cfg.output.table_width, baseline.output.table_width);
        assert_eq!(cfg.output.table_wrap, baseline.output.table_wrap);
        assert_eq!(cfg.output.table_bands, baseline.output.table_bands);
        assert_eq!(cfg.output.empty_result, baseline.output.empty_result);
        assert_eq!(
            cfg.output.object_list_data_columns,
            baseline.output.object_list_data_columns
        );
        assert_eq!(cfg.server.password, baseline.server.password);

        clear_env();
    }

    #[test]
    fn config_value_candidates_expose_enum_values() {
        assert_eq!(
            config_value_candidates("output.table_style"),
            strings(&["ascii", "compact", "dense", "markdown", "plain", "rounded"])
        );
        assert_eq!(
            config_value_candidates("output.table_bands"),
            strings(&["auto", "always", "never"])
        );
        assert_eq!(
            config_value_candidates("output.object_list_data_columns"),
            strings(&["auto", "preview", "all"])
        );
        assert!(config_value_candidates("output.theme").contains(&"hubuum-dark".to_string()));
        assert_eq!(
            config_value_candidates("server.hostname"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn server_sync_only_includes_portable_preference_keys() {
        assert!(is_user_preference_key("output.theme"));
        assert!(is_user_preference_key(
            "output.object_list_class_columns.Hosts"
        ));
        assert!(is_user_preference_key(
            "output.object_list_class_aliases.Hosts.os_version"
        ));
        assert!(is_user_preference_key(
            "output.object_class_computed_fields.Hosts"
        ));
        assert!(is_user_preference_key("repl.enter_fetches_next_page"));
        assert!(!is_user_preference_key("output.theme_file"));
        assert!(!is_user_preference_key("server.hostname"));
        assert!(!is_user_preference_key("settings.store_on_server"));
    }

    #[test]
    fn display_alias_config_keys_accept_legacy_spelling() {
        assert_eq!(
            object_list_class_alias_key("output.object_list_class_aliases.Hosts.os_version"),
            Some(("Hosts", "os_version"))
        );
        assert_eq!(
            object_list_class_alias_key("output.object_list_class_meta.Hosts.os_version"),
            Some(("Hosts", "os_version"))
        );
        assert!(config_key_names().contains(&"output.object_list_class_aliases"));
        assert!(config_key_names().contains(&"output.object_class_computed_fields"));
        assert!(!config_key_names().contains(&"output.object_list_class_meta"));
        assert_eq!(
            object_class_computed_fields_key("output.object_class_computed_fields.Hosts"),
            Some("Hosts")
        );
    }

    #[test]
    #[serial]
    fn source_resolution_prefers_env_over_user_file() {
        clear_env();
        set_var("HUBUUM_CLI__SERVER__HOSTNAME", "env.example.com");

        let descriptor = descriptor_for_key("server.hostname").expect("missing descriptor");
        let user_path = Path::new("/tmp/user.toml");
        let user_toml: TomlValue = parse_toml(
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
            runtime_cli_args: None,
            matches: None,
        };
        let (source, detail) = resolve_config_source(descriptor, &context);

        assert_eq!(source, ConfigSource::Environment);
        assert_eq!(detail.as_deref(), Some("HUBUUM_CLI__SERVER__HOSTNAME"));

        clear_env();
    }

    #[test]
    #[serial]
    fn reload_runtime_config_keeps_startup_cli_overrides() {
        clear_env();

        let pid = id();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let config_path = temp_dir().join(format!("hubuum-cli-{pid}-{unique}.toml"));

        write(
            &config_path,
            r#"
[server]
port = 8080
username = "default_user"
protocol = "https"

[output]
object_show_data = false
"#,
        )
        .expect("should write test config file");

        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--config",
                config_path.to_str().expect("path should be valid utf8"),
                "--port",
                "7070",
                "--username",
                "admin",
                "--protocol",
                "http",
            ])
            .expect("cli should parse");

        let mut initial = load_config(Some(config_path.clone())).expect("config should load");
        update_config_from_cli(&mut initial, &matches);
        init_config(initial.clone()).expect("should initialize config");
        init_config_state(inspect_config_state(
            &initial,
            Some(config_path.clone()),
            &matches,
        ))
        .expect("should initialize config state");

        reload_runtime_config().expect("reload should work");

        let cfg = get_config();
        assert_eq!(cfg.server.port, 7070);
        assert_eq!(cfg.server.username, "admin");
        assert_eq!(cfg.server.protocol, Protocol::Http);

        let state = get_config_state();
        assert_eq!(
            state.entry("server.port").map(|entry| entry.source.clone()),
            Some(ConfigSource::CliOption)
        );
        assert_eq!(
            state
                .entry("server.username")
                .map(|entry| entry.source.clone()),
            Some(ConfigSource::CliOption)
        );
        assert_eq!(
            state
                .entry("server.protocol")
                .map(|entry| entry.source.clone()),
            Some(ConfigSource::CliOption)
        );

        let _ = remove_file(config_path);
        clear_env();
    }

    #[test]
    fn imported_preferences_preserve_local_and_server_specific_values() {
        let mut root: TomlValue = parse_toml(
            r#"
[server]
hostname = "local.example.com"

[cache]
time = 123

[settings]
store_on_server = true

[output]
theme = "old-theme"
theme_file = "/machine/specific/themes.toml"
"#,
        )
        .expect("test TOML should parse");
        let mut config = AppConfig::default();
        config.output.theme = "hubuum-light".to_string();
        let preferences = UserPreferences::from_config(&config);

        merge_user_preferences(&mut root, &preferences).expect("preferences should merge");

        assert_eq!(
            toml_get(&root, "server.hostname").and_then(TomlValue::as_str),
            Some("local.example.com")
        );
        assert_eq!(
            toml_get(&root, "cache.time").and_then(TomlValue::as_integer),
            Some(123)
        );
        assert_eq!(
            toml_get(&root, "settings.store_on_server").and_then(TomlValue::as_bool),
            Some(true)
        );
        assert_eq!(
            toml_get(&root, "output.theme").and_then(TomlValue::as_str),
            Some("hubuum-light")
        );
        assert_eq!(
            toml_get(&root, "output.theme_file").and_then(TomlValue::as_str),
            Some("/machine/specific/themes.toml")
        );
    }
}
