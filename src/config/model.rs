use hubuum_theme::{RoleStyleOverride, ThemeRole};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use toml::Value as TomlValue;

use crate::defaults::Defaults;
use crate::domain::{CommandAliases, ComputedFieldSet};
use crate::files::{get_system_extension_root, get_user_extension_root};
use crate::models::{
    EmptyResult, ObjectListDataColumns, OutputColor, OutputFormat, Protocol, SessionStorage,
    TableBands, TableHeaders, TableStyle, TableWidth, TableWrap,
};

pub type ThemeOverrides = BTreeMap<String, BTreeMap<ThemeRole, RoleStyleOverride>>;

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
    #[serde(skip)]
    pub display_value: String,
    pub source: ConfigSource,
    pub source_detail: Option<String>,
    pub sensitive: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    Default,
    Discovered,
    SystemFile,
    UserFile,
    CustomFile,
    Environment,
    CliOption,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub aliases: CommandAliases,
    pub server: ServerConfig,
    pub cache: CacheConfig,
    #[serde(default)]
    pub settings: SettingsConfig,
    #[serde(default)]
    pub self_update: SelfUpdateConfig,
    pub completion: CompletionConfig,
    pub background: BackgroundConfig,
    pub repl: ReplConfig,
    pub relations: RelationsConfig,
    pub output: OutputConfig,
    #[serde(default)]
    pub extensions: ExtensionsConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtensionsConfig {
    #[serde(default = "default_system_extension_roots")]
    pub system_roots: Vec<PathBuf>,
    #[serde(default = "default_user_extension_roots")]
    pub user_roots: Vec<PathBuf>,
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, TomlValue>,
}

impl Default for ExtensionsConfig {
    fn default() -> Self {
        Self {
            system_roots: default_system_extension_roots(),
            user_roots: default_user_extension_roots(),
            disabled: Vec::new(),
            config: BTreeMap::new(),
        }
    }
}

pub(super) fn default_system_extension_roots() -> Vec<PathBuf> {
    vec![get_system_extension_root()]
}

pub(super) fn default_user_extension_roots() -> Vec<PathBuf> {
    vec![get_user_extension_root()]
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SettingsConfig {
    pub store_on_server: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct SelfUpdateConfig {
    enabled: bool,
    check_on_startup: bool,
}

impl Default for SelfUpdateConfig {
    fn default() -> Self {
        Self {
            enabled: Defaults::SELF_UPDATE_ENABLED,
            check_on_startup: Defaults::SELF_UPDATE_CHECK_ON_STARTUP,
        }
    }
}

impl SelfUpdateConfig {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn check_on_startup(&self) -> bool {
        self.check_on_startup
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserPreferences {
    #[serde(default)]
    pub aliases: CommandAliases,
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
    #[serde(default)]
    pub theme_overrides: ThemeOverrides,
    pub padding: i8,
    pub table_style: TableStyle,
    #[serde(default)]
    pub table_headers: TableHeaders,
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
            aliases: config.aliases.clone(),
            completion: config.completion.clone(),
            background: config.background.clone(),
            repl: config.repl.clone(),
            relations: config.relations.clone(),
            output: UserOutputPreferences {
                format: config.output.format.clone(),
                color: config.output.color,
                theme: config.output.theme.clone(),
                theme_overrides: config.output.theme_overrides.clone(),
                padding: config.output.padding,
                table_style: config.output.table_style.clone(),
                table_headers: config.output.table_headers,
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
    #[serde(default = "Defaults::max_response_body_bytes")]
    pub max_response_body_bytes: NonZeroUsize,
    pub api_version: String,
    #[serde(default)]
    pub identity_scope: Option<String>,
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub token_file: Option<String>,
    #[serde(default)]
    pub session_storage: SessionStorage,
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
    #[serde(default)]
    pub theme_overrides: ThemeOverrides,
    pub padding: i8,
    pub table_style: TableStyle,
    pub table_headers: TableHeaders,
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
    pub(super) legacy_object_list_class_meta: HashMap<String, HashMap<String, Vec<String>>>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            aliases: CommandAliases::default(),
            server: ServerConfig {
                hostname: Defaults::SERVER_HOSTNAME.to_string(),
                port: Defaults::SERVER_PORT,
                ssl_validation: Defaults::SERVER_SSL_VALIDATION,
                max_response_body_bytes: Defaults::max_response_body_bytes(),
                api_version: Defaults::API_VERSION.to_string(),
                identity_scope: None,
                username: Defaults::USER_USERNAME.to_string(),
                password: None,
                token_file: None,
                session_storage: SessionStorage::default(),
                protocol: Defaults::PROTOCOL,
            },
            cache: CacheConfig {
                time: Defaults::CACHE_TIME,
                size: Defaults::CACHE_SIZE,
                disable: Defaults::CACHE_DISABLE,
            },
            settings: SettingsConfig::default(),
            self_update: SelfUpdateConfig::default(),
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
                theme_overrides: ThemeOverrides::new(),
                padding: Defaults::OUTPUT_PADDING,
                table_style: Defaults::OUTPUT_TABLE_STYLE,
                table_headers: Defaults::OUTPUT_TABLE_HEADERS,
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
            extensions: ExtensionsConfig::default(),
        }
    }
}

impl ConfigState {
    pub fn entry(&self, key: &str) -> Option<&ConfigEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }
}
