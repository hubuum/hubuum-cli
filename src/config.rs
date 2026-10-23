mod model;
use model::{default_system_extension_roots, default_user_extension_roots};
pub use model::{
    AppConfig, ConfigEntry, ConfigPaths, ConfigSource, ConfigState, OutputConfig, SelfUpdateConfig,
    ServerConfig, UserPreferences,
};
mod descriptors;
use descriptors::{ConfigKeyDescriptor, ConfigValueKind, CONFIG_KEYS};
mod loading;
pub use loading::load_config;
mod persistence;
pub use persistence::{
    persist_command_alias, persist_user_preferences, set_persisted_value, unset_persisted_value,
};

use clap::{parser::ValueSource, ArgMatches};
use hubuum_theme::{catalog as theme_catalog, theme_names};
use once_cell::sync::Lazy;
use serde_json::to_string as to_json_string;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env::var_os;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use toml::map::Map as TomlMap;
use toml::{from_str as parse_toml, Value as TomlValue};

use crate::defaults::Defaults;
use crate::domain::{CommandAliases, ComputedFieldSet};
use crate::errors::AppError;
use crate::files::{get_system_config_path, get_user_config_path};
use crate::models::{
    EmptyResult, ObjectListDataColumns, OutputColor, OutputFormat, Protocol, SessionStorage,
    TableBands, TableHeaders, TableStyle, TableWidth, TableWrap,
};

static CONFIG: Lazy<RwLock<Arc<AppConfig>>> =
    Lazy::new(|| RwLock::new(Arc::new(AppConfig::default())));
static CONFIG_STATE: Lazy<RwLock<Option<ConfigState>>> = Lazy::new(|| RwLock::new(None));

pub fn init_config(cfg: impl Into<Arc<AppConfig>>) -> Result<(), AppError> {
    *CONFIG
        .write()
        .map_err(|_| AppError::GeneralConfigError("Failed to update config".to_string()))? =
        cfg.into();
    Ok(())
}

pub fn get_config() -> Arc<AppConfig> {
    Arc::clone(&CONFIG.read().expect("config lock should not be poisoned"))
}

pub fn init_config_state(state: ConfigState) -> Result<(), AppError> {
    *CONFIG_STATE
        .write()
        .map_err(|_| AppError::GeneralConfigError("Failed to update config state".to_string()))? =
        Some(state);
    Ok(())
}

pub fn update_runtime_server_port(port: u16) -> Result<(), AppError> {
    let mut state = CONFIG_STATE
        .write()
        .map_err(|_| AppError::GeneralConfigError("Failed to update config state".to_string()))?;
    let state = state.as_mut().ok_or_else(|| {
        AppError::GeneralConfigError(
            "Config state not initialized before selecting a server port".to_string(),
        )
    })?;
    let entry = state
        .entries
        .iter_mut()
        .find(|entry| entry.key == "server.port")
        .ok_or_else(|| {
            AppError::GeneralConfigError("Server port is missing from config state".to_string())
        })?;
    if entry.source == ConfigSource::Default && port != Defaults::SERVER_PORT {
        mark_discovered_server_port(entry, port);
    } else {
        update_server_port_value(entry, port);
    }
    Ok(())
}

fn update_server_port_value(entry: &mut ConfigEntry, port: u16) {
    let value = port.to_string();
    entry.value.clone_from(&value);
    entry.display_value = value;
}

fn mark_discovered_server_port(entry: &mut ConfigEntry, port: u16) {
    update_server_port_value(entry, port);
    entry.source = ConfigSource::Discovered;
    entry.source_detail = Some("selected from default ports".to_string());
}

pub fn get_config_state() -> ConfigState {
    CONFIG_STATE
        .read()
        .expect("config state lock should not be poisoned")
        .clone()
        .expect("Config state not initialized. Call init_config_state(...) in main after loading.")
}

pub fn config_key_names() -> Vec<&'static str> {
    CONFIG_KEYS
        .iter()
        .map(|descriptor| descriptor.key)
        .collect()
}

pub fn is_user_preference_key(key: &str) -> bool {
    (key == "aliases"
        || key.starts_with("aliases.")
        || key.starts_with("completion.")
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
        ConfigValueKind::SessionStorage => strings(&["file", "keychain", "none"]),
        ConfigValueKind::OutputFormat => strings(&["text", "json"]),
        ConfigValueKind::OutputColor => strings(&["auto", "always", "never"]),
        ConfigValueKind::ThemeName => theme_value_candidates(),
        ConfigValueKind::TableStyle => {
            strings(&["ascii", "compact", "dense", "markdown", "plain", "rounded"])
        }
        ConfigValueKind::TableHeaders => strings(&["grouped", "full", "none"]),
        ConfigValueKind::TableWidth => strings(&["auto", "full"]),
        ConfigValueKind::TableWrap => strings(&["auto", "never"]),
        ConfigValueKind::TableBands => strings(&["auto", "always", "never"]),
        ConfigValueKind::EmptyResult => strings(&["message", "silent"]),
        ConfigValueKind::ObjectListDataColumns => strings(&["auto", "preview", "all"]),
        ConfigValueKind::StringMap
        | ConfigValueKind::StringListMap
        | ConfigValueKind::StringNestedListMap
        | ConfigValueKind::ComputedFieldSetMap
        | ConfigValueKind::PathList
        | ConfigValueKind::StringList
        | ConfigValueKind::TomlTable => Vec::new(),
        ConfigValueKind::String
        | ConfigValueKind::U16
        | ConfigValueKind::U64
        | ConfigValueKind::PositiveUsize
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
                value: serialize_config_value(
                    config_value(config, descriptor.key),
                    descriptor.sensitive,
                ),
                display_value: display_config_value(
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

pub fn reload_runtime_config() -> Result<(), AppError> {
    let previous_state = get_config_state();
    let discovered_server_port = discovered_server_port(&previous_state);
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

    let mut state = if runtime_cli_args.is_empty() {
        inspect_config_state_without_cli(&config, custom)
    } else {
        inspect_config_state_with_runtime_cli(&config, custom, &runtime_cli_args)
    };
    preserve_discovered_server_port(discovered_server_port, &mut config, &mut state)?;
    init_config_state(state)?;
    init_config(config)?;
    Ok(())
}

fn discovered_server_port(state: &ConfigState) -> Option<u16> {
    state
        .entry("server.port")
        .filter(|entry| entry.source == ConfigSource::Discovered)
        .and_then(|entry| entry.value.parse::<u16>().ok())
}

fn preserve_discovered_server_port(
    discovered_port: Option<u16>,
    config: &mut AppConfig,
    state: &mut ConfigState,
) -> Result<(), AppError> {
    let Some(port) = discovered_port else {
        return Ok(());
    };
    let entry = state
        .entries
        .iter_mut()
        .find(|entry| entry.key == "server.port")
        .ok_or_else(|| {
            AppError::GeneralConfigError("Server port is missing from config state".to_string())
        })?;
    if entry.source == ConfigSource::Default {
        config.server.port = port;
        mark_discovered_server_port(entry, port);
    }
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
            "server.token_file" => target.server.token_file = source.server.token_file.clone(),
            "server.session_storage" => {
                target.server.session_storage = source.server.session_storage
            }
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
            "output.table_headers" => target.output.table_headers = source.output.table_headers,
            "output.table_width" => target.output.table_width = source.output.table_width.clone(),
            "output.table_wrap" => target.output.table_wrap = source.output.table_wrap.clone(),
            "output.table_bands" => target.output.table_bands = source.output.table_bands,
            "output.empty_result" => target.output.empty_result = source.output.empty_result,
            _ => {}
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
        "token_file" => Some("--token-file"),
        "session_storage" => Some("--session-storage"),
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
        "table_headers" => Some("--table-headers"),
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
        "aliases" => ConfigValueRef::CommandAliases(&config.aliases),
        "extensions.system_roots" => ConfigValueRef::PathList(&config.extensions.system_roots),
        "extensions.user_roots" => ConfigValueRef::PathList(&config.extensions.user_roots),
        "extensions.disabled" => ConfigValueRef::StringList(&config.extensions.disabled),
        "extensions.config" => ConfigValueRef::TomlTable(&config.extensions.config),
        "server.hostname" => ConfigValueRef::String(&config.server.hostname),
        "server.port" => ConfigValueRef::U16(config.server.port),
        "server.ssl_validation" => ConfigValueRef::Bool(config.server.ssl_validation),
        "server.max_response_body_bytes" => {
            ConfigValueRef::U64(config.server.max_response_body_bytes.get() as u64)
        }
        "server.api_version" => ConfigValueRef::String(&config.server.api_version),
        "server.identity_scope" => {
            ConfigValueRef::OptionalString(config.server.identity_scope.as_deref())
        }
        "server.username" => ConfigValueRef::String(&config.server.username),
        "server.password" => ConfigValueRef::OptionalString(config.server.password.as_deref()),
        "server.token_file" => ConfigValueRef::OptionalString(config.server.token_file.as_deref()),
        "server.session_storage" => ConfigValueRef::SessionStorage(config.server.session_storage),
        "server.protocol" => ConfigValueRef::Protocol(&config.server.protocol),
        "cache.time" => ConfigValueRef::U64(config.cache.time),
        "cache.size" => ConfigValueRef::I32(config.cache.size),
        "cache.disable" => ConfigValueRef::Bool(config.cache.disable),
        "settings.store_on_server" => ConfigValueRef::Bool(config.settings.store_on_server),
        "self_update.enabled" => ConfigValueRef::Bool(config.self_update.enabled()),
        "self_update.check_on_startup" => {
            ConfigValueRef::Bool(config.self_update.check_on_startup())
        }
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
        "output.table_headers" => ConfigValueRef::TableHeaders(&config.output.table_headers),
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
    SessionStorage(SessionStorage),
    OutputFormat(&'a OutputFormat),
    OutputColor(&'a OutputColor),
    TableStyle(&'a TableStyle),
    TableHeaders(&'a TableHeaders),
    TableWidth(&'a TableWidth),
    TableWrap(&'a TableWrap),
    TableBands(&'a TableBands),
    EmptyResult(&'a EmptyResult),
    ObjectListDataColumns(&'a ObjectListDataColumns),
    CommandAliases(&'a CommandAliases),
    StringListMap(&'a HashMap<String, Vec<String>>),
    StringNestedListMap(&'a HashMap<String, HashMap<String, Vec<String>>>),
    ComputedFieldSetMap(&'a HashMap<String, ComputedFieldSet>),
    PathList(&'a [PathBuf]),
    StringList(&'a [String]),
    TomlTable(&'a BTreeMap<String, TomlValue>),
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
        ConfigValueRef::SessionStorage(value) => value.to_string(),
        ConfigValueRef::OutputFormat(value) => match value {
            OutputFormat::Json => "json".to_string(),
            OutputFormat::Text => "text".to_string(),
        },
        ConfigValueRef::OutputColor(value) => value.to_string(),
        ConfigValueRef::TableStyle(value) => value.to_string(),
        ConfigValueRef::TableHeaders(value) => value.to_string(),
        ConfigValueRef::TableWidth(value) => value.to_string(),
        ConfigValueRef::TableWrap(value) => value.to_string(),
        ConfigValueRef::TableBands(value) => value.to_string(),
        ConfigValueRef::EmptyResult(value) => value.to_string(),
        ConfigValueRef::ObjectListDataColumns(value) => value.to_string(),
        ConfigValueRef::CommandAliases(value) => format_command_aliases(value),
        ConfigValueRef::StringListMap(value) => format_string_list_map(value),
        ConfigValueRef::StringNestedListMap(value) => format_string_nested_list_map(value),
        ConfigValueRef::ComputedFieldSetMap(value) => format_computed_field_set_map(value),
        ConfigValueRef::PathList(value) => value
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        ConfigValueRef::StringList(value) => value.join("\n"),
        ConfigValueRef::TomlTable(value) => {
            toml::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
        }
    }
}

fn serialize_config_value(value: ConfigValueRef<'_>, sensitive: bool) -> String {
    if sensitive {
        return display_config_value(value, true);
    }

    match value {
        ConfigValueRef::CommandAliases(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::StringListMap(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::StringNestedListMap(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::ComputedFieldSetMap(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::PathList(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::StringList(value) => to_json_string(value).unwrap_or_default(),
        ConfigValueRef::TomlTable(value) => to_json_string(value).unwrap_or_default(),
        value => display_config_value(value, sensitive),
    }
}

fn format_command_aliases(aliases: &CommandAliases) -> String {
    if aliases.is_empty() {
        return "{}".to_string();
    }

    aliases
        .names()
        .map(|name| {
            format!(
                "{name}: {}",
                aliases.description(name).unwrap_or("<no description>")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_string_list_map(values: &HashMap<String, Vec<String>>) -> String {
    let values = values.iter().collect::<BTreeMap<_, _>>();
    format_list_map(values.into_iter().map(|(name, items)| {
        (
            name.as_str(),
            items.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    }))
}

fn format_computed_field_set_map(values: &HashMap<String, ComputedFieldSet>) -> String {
    let values = values.iter().collect::<BTreeMap<_, _>>();
    format_list_map(values.into_iter().map(|(name, fields)| {
        (
            name.as_str(),
            fields
                .selectors()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        )
    }))
}

fn format_list_map<'a, T, I>(values: I) -> String
where
    T: AsRef<str>,
    I: IntoIterator<Item = (&'a str, Vec<T>)>,
{
    let mut lines = Vec::new();
    for (name, items) in values {
        if items.is_empty() {
            lines.push(format!("{name}: []"));
            continue;
        }
        lines.push(format!("{name}:"));
        lines.extend(
            items
                .into_iter()
                .map(|item| format!("  - {}", item.as_ref())),
        );
    }
    if lines.is_empty() {
        "{}".to_string()
    } else {
        lines.join("\n")
    }
}

fn format_string_nested_list_map(values: &HashMap<String, HashMap<String, Vec<String>>>) -> String {
    if values.is_empty() {
        return "{}".to_string();
    }

    let mut lines = Vec::new();
    for (name, nested) in values.iter().collect::<BTreeMap<_, _>>() {
        if nested.is_empty() {
            lines.push(format!("{name}: {{}}"));
            continue;
        }
        lines.push(format!("{name}:"));
        for (nested_name, items) in nested.iter().collect::<BTreeMap<_, _>>() {
            if items.is_empty() {
                lines.push(format!("  {nested_name}: []"));
                continue;
            }
            lines.push(format!("  {nested_name}:"));
            lines.extend(items.iter().map(|item| format!("    - {item}")));
        }
    }
    lines.join("\n")
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

#[cfg(test)]
mod tests {
    use super::persistence::{
        merge_user_preferences, object_class_computed_fields_key, object_list_class_alias_key,
        parse_config_value,
    };
    use super::*;
    use crate::cli::{build_cli, update_config_from_cli};
    use crate::domain::{CommandAliasDescription, CommandAliasName, CommandAliasTarget};
    use crate::models::{
        EmptyResult, ObjectListDataColumns, OutputColor, Protocol, TableBands, TableHeaders,
        TableStyle, TableWidth, TableWrap,
    };
    use serial_test::serial;
    use std::env::{remove_var, set_var, temp_dir};
    use std::fs::{read_to_string, remove_file, write};
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
            "HUBUUM_CLI__SERVER__MAX_RESPONSE_BODY_BYTES",
            "HUBUUM_CLI__SERVER__API_VERSION",
            "HUBUUM_CLI__SERVER__USERNAME",
            "HUBUUM_CLI__SERVER__PASSWORD",
            "HUBUUM_CLI__SERVER__TOKEN_FILE",
            "HUBUUM_CLI__SERVER__SESSION_STORAGE",
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
            "HUBUUM_CLI__OUTPUT__TABLE_HEADERS",
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
        set_var("HUBUUM_CLI__SERVER__MAX_RESPONSE_BODY_BYTES", "67108864");
        set_var("HUBUUM_CLI__SERVER__API_VERSION", "v9");
        set_var("HUBUUM_CLI__SERVER__USERNAME", "env_user");
        set_var("HUBUUM_CLI__SERVER__PASSWORD", "hunter2");
        set_var(
            "HUBUUM_CLI__SERVER__TOKEN_FILE",
            "/run/secrets/hubuum-token",
        );
        set_var("HUBUUM_CLI__SERVER__PROTOCOL", "http");
        set_var("HUBUUM_CLI__SERVER__SESSION_STORAGE", "keychain");

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
        set_var("HUBUUM_CLI__OUTPUT__TABLE_HEADERS", "full");
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
        assert_eq!(cfg.server.max_response_body_bytes.get(), 67108864);
        assert_eq!(cfg.server.api_version, "v9");
        assert_eq!(cfg.server.username, "env_user");
        assert_eq!(cfg.server.password, Some("hunter2".into()));
        assert_eq!(
            cfg.server.token_file.as_deref(),
            Some("/run/secrets/hubuum-token")
        );
        assert_eq!(cfg.server.protocol, Protocol::Http);
        assert_eq!(cfg.server.session_storage, SessionStorage::Keychain);

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
        assert_eq!(cfg.output.table_headers, TableHeaders::Full);
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
    fn response_body_limit_rejects_zero_negative_and_overflow_values() {
        let descriptor = descriptor_for_key("server.max_response_body_bytes").unwrap();
        for value in ["0", "-1", "18446744073709551616"] {
            assert!(parse_config_value(descriptor, value).is_err());
        }
        assert_eq!(
            parse_config_value(descriptor, "67108864")
                .unwrap()
                .as_integer(),
            Some(67108864)
        );
        let mut server = serde_json::to_value(AppConfig::default().server).unwrap();
        server["max_response_body_bytes"] = serde_json::json!(0);
        assert!(serde_json::from_value::<ServerConfig>(server).is_err());
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
    fn structured_config_values_render_as_sorted_indented_trees() {
        let columns = HashMap::from([
            ("Services".to_string(), vec!["Name".to_string()]),
            (
                "Hosts".to_string(),
                vec![
                    "data.facts.network.default_ipv4.address".to_string(),
                    "data.facts.network.default_ipv6.address".to_string(),
                ],
            ),
        ]);
        assert_eq!(
            display_config_value(ConfigValueRef::StringListMap(&columns), false),
            "Hosts:\n  - data.facts.network.default_ipv4.address\n  - data.facts.network.default_ipv6.address\nServices:\n  - Name"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialize_config_value(
                ConfigValueRef::StringListMap(&columns),
                false,
            ))
            .expect("serialized columns should remain JSON"),
            serde_json::json!({
                "Hosts": [
                    "data.facts.network.default_ipv4.address",
                    "data.facts.network.default_ipv6.address"
                ],
                "Services": ["Name"]
            })
        );

        let aliases = HashMap::from([(
            "Hosts".to_string(),
            HashMap::from([
                (
                    "os_version".to_string(),
                    vec!["data.facts.operating_system.version".to_string()],
                ),
                (
                    "IP".to_string(),
                    vec![
                        "data.facts.network.default_ipv4.address".to_string(),
                        "data.facts.network.default_ipv6.address".to_string(),
                    ],
                ),
            ]),
        )]);
        assert_eq!(
            display_config_value(ConfigValueRef::StringNestedListMap(&aliases), false),
            "Hosts:\n  IP:\n    - data.facts.network.default_ipv4.address\n    - data.facts.network.default_ipv6.address\n  os_version:\n    - data.facts.operating_system.version"
        );
    }

    #[test]
    fn command_alias_config_value_uses_descriptions_instead_of_commands() {
        let aliases: CommandAliases = serde_json::from_value(serde_json::json!({
            "hosts": "object list --class Hosts | P Name",
            "outdated-kernels": {
                "command": "object list --class Hosts | JQ 'a very long filter'",
                "description": "Find hosts running an outdated kernel"
            }
        }))
        .expect("aliases should deserialize");

        assert_eq!(
            display_config_value(ConfigValueRef::CommandAliases(&aliases), false),
            "hosts: <no description>\noutdated-kernels: Find hosts running an outdated kernel"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialize_config_value(
                ConfigValueRef::CommandAliases(&aliases),
                false,
            ))
            .expect("serialized aliases should remain JSON"),
            serde_json::json!({
                "hosts": "object list --class Hosts | P Name",
                "outdated-kernels": {
                    "command": "object list --class Hosts | JQ 'a very long filter'",
                    "description": "Find hosts running an outdated kernel"
                }
            })
        );
    }

    #[test]
    #[serial]
    fn described_command_aliases_load_from_toml() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"
[aliases.outdated-kernels]
command = "object list --class Hosts | C"
description = "Find hosts running an outdated kernel"
"#,
        )
        .expect("write config");

        let cfg = load_config(Some(path)).expect("load config");

        assert_eq!(
            cfg.aliases.get("outdated-kernels"),
            Some("object list --class Hosts | C")
        );
        assert_eq!(
            cfg.aliases.description("outdated-kernels"),
            Some("Find hosts running an outdated kernel")
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn table_headers_none_can_be_persisted() {
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

        set_persisted_value("output.table_headers", "none").expect("header mode should persist");
        let configured = load_config(Some(path)).expect("persisted config should load");

        assert_eq!(configured.output.table_headers, TableHeaders::None);
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
    fn config_set_and_unset_persist_command_aliases() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let inherited_alias = load_config(None)
            .expect("inherited config should load")
            .aliases
            .get("outdated-kernels")
            .map(str::to_string);
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

        set_persisted_value("aliases.outdated-kernels", "object list | C")
            .expect("command alias should persist");
        let configured = load_config(Some(path.clone())).expect("persisted config should load");
        assert_eq!(
            configured.aliases.get("outdated-kernels"),
            Some("object list | C")
        );

        let name = CommandAliasName::new("outdated-kernels").expect("valid alias name");
        let target =
            CommandAliasTarget::new("object list --class Hosts | C").expect("valid alias command");
        let description = CommandAliasDescription::new("Find hosts running an outdated kernel")
            .expect("valid alias description");
        persist_command_alias(&name, &target, Some(&description))
            .expect("described command alias should persist");
        let configured = load_config(Some(path.clone())).expect("persisted config should load");
        assert_eq!(
            configured.aliases.get("outdated-kernels"),
            Some("object list --class Hosts | C")
        );
        assert_eq!(
            configured.aliases.description("outdated-kernels"),
            Some("Find hosts running an outdated kernel")
        );

        unset_persisted_value("aliases.outdated-kernels").expect("command alias should be removed");
        let persisted = read_to_string(&path).expect("updated config should be readable");
        let root = parse_toml(&persisted).expect("updated config should remain valid TOML");
        assert!(toml_get(&root, "aliases.outdated-kernels").is_none());
        let configured = load_config(Some(path)).expect("updated config should load");
        assert_eq!(
            configured.aliases.get("outdated-kernels"),
            inherited_alias.as_deref()
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn command_alias_config_keys_validate_names_and_targets() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        init_config_state(ConfigState {
            paths: ConfigPaths {
                system: dir.path().join("system.toml"),
                user: path.clone(),
                custom: Some(path.clone()),
                write_target: path,
            },
            entries: Vec::new(),
        })
        .expect("config state should initialize");

        assert!(set_persisted_value("aliases.2invalid", "help").is_err());
        assert!(set_persisted_value("aliases.valid", "").is_err());
        assert!(set_persisted_value("aliases.valid", "help\nversion").is_err());
        clear_env();
    }

    #[test]
    #[serial]
    fn config_set_and_unset_persist_class_aliases() {
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

        set_persisted_value(
            "output.object_list_class_aliases.Hosts.IPv4",
            "data.facts.network.default_ipv4.address",
        )
        .expect("class alias should persist");
        let configured = load_config(Some(path.clone())).expect("persisted config should load");
        assert_eq!(
            configured.output.object_list_class_aliases["Hosts"]["IPv4"],
            vec!["data.facts.network.default_ipv4.address"]
        );

        unset_persisted_value("output.object_list_class_aliases.Hosts.IPv4")
            .expect("class alias should be removed");
        let configured = load_config(Some(path)).expect("updated config should load");
        assert!(configured
            .output
            .object_list_class_aliases
            .get("Hosts")
            .and_then(|aliases| aliases.get("IPv4"))
            .is_none());
        clear_env();
    }

    #[test]
    #[serial]
    fn config_mutations_preserve_malformed_files() {
        clear_env();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let malformed = "[output\nunknown = true\n";
        write(&path, malformed).expect("malformed config should be written");
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

        let set_error = set_persisted_value("server.port", "8443")
            .expect_err("set should reject malformed config");
        assert!(set_error
            .to_string()
            .contains(path.to_string_lossy().as_ref()));
        assert_eq!(
            read_to_string(&path).expect("config should remain readable"),
            malformed
        );

        let unset_error =
            unset_persisted_value("server.port").expect_err("unset should reject malformed config");
        assert!(unset_error
            .to_string()
            .contains(path.to_string_lossy().as_ref()));
        assert_eq!(
            read_to_string(&path).expect("config should remain readable"),
            malformed
        );

        let preferences = UserPreferences::from_config(&AppConfig::default());
        let import_error = persist_user_preferences(&preferences)
            .expect_err("preference import should reject malformed config");
        assert!(import_error
            .to_string()
            .contains(path.to_string_lossy().as_ref()));
        assert_eq!(
            read_to_string(path).expect("config should remain readable"),
            malformed
        );
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
        assert_eq!(cfg.output.table_headers, baseline.output.table_headers);
        assert_eq!(cfg.output.table_width, baseline.output.table_width);
        assert_eq!(cfg.output.table_wrap, baseline.output.table_wrap);
        assert_eq!(cfg.output.table_bands, baseline.output.table_bands);
        assert_eq!(cfg.output.empty_result, baseline.output.empty_result);
        assert_eq!(
            cfg.output.object_list_data_columns,
            baseline.output.object_list_data_columns
        );
        assert_eq!(cfg.server.password, baseline.server.password);
        assert_eq!(cfg.server.token_file, baseline.server.token_file);

        clear_env();
    }

    #[test]
    fn config_value_candidates_expose_enum_values() {
        assert_eq!(
            config_value_candidates("output.table_style"),
            strings(&["ascii", "compact", "dense", "markdown", "plain", "rounded"])
        );
        assert_eq!(
            config_value_candidates("output.table_headers"),
            strings(&["grouped", "full", "none"])
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
        assert!(is_user_preference_key("output.table_headers"));
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
    fn discovered_server_port_survives_default_source_reload() {
        let mut previous_entry = ConfigEntry {
            key: "server.port".to_string(),
            value: Defaults::SERVER_PORT.to_string(),
            display_value: Defaults::SERVER_PORT.to_string(),
            source: ConfigSource::Default,
            source_detail: None,
            sensitive: false,
        };
        mark_discovered_server_port(&mut previous_entry, 8080);
        let previous_state = ConfigState {
            paths: ConfigPaths {
                system: PathBuf::from("/etc/hubuum_cli/config.toml"),
                user: PathBuf::from("/tmp/hubuum-cli-user.toml"),
                custom: None,
                write_target: PathBuf::from("/tmp/hubuum-cli-user.toml"),
            },
            entries: vec![previous_entry],
        };
        let mut reloaded_config = AppConfig::default();
        let mut reloaded_state = ConfigState {
            paths: previous_state.paths.clone(),
            entries: vec![ConfigEntry {
                key: "server.port".to_string(),
                value: Defaults::SERVER_PORT.to_string(),
                display_value: Defaults::SERVER_PORT.to_string(),
                source: ConfigSource::Default,
                source_detail: None,
                sensitive: false,
            }],
        };

        preserve_discovered_server_port(
            discovered_server_port(&previous_state),
            &mut reloaded_config,
            &mut reloaded_state,
        )
        .expect("discovered port should be preserved");

        assert_eq!(reloaded_config.server.port, 8080);
        let entry = reloaded_state
            .entry("server.port")
            .expect("server port state should exist");
        assert_eq!(entry.value, "8080");
        assert_eq!(entry.source, ConfigSource::Discovered);
    }

    #[test]
    fn configured_server_port_replaces_discovered_runtime_port_on_reload() {
        let mut config = AppConfig::default();
        config.server.port = 8443;
        let mut state = ConfigState {
            paths: ConfigPaths {
                system: PathBuf::from("/etc/hubuum_cli/config.toml"),
                user: PathBuf::from("/tmp/hubuum-cli-user.toml"),
                custom: None,
                write_target: PathBuf::from("/tmp/hubuum-cli-user.toml"),
            },
            entries: vec![ConfigEntry {
                key: "server.port".to_string(),
                value: "8443".to_string(),
                display_value: "8443".to_string(),
                source: ConfigSource::UserFile,
                source_detail: Some("/tmp/hubuum-cli-user.toml".to_string()),
                sensitive: false,
            }],
        };

        preserve_discovered_server_port(Some(8080), &mut config, &mut state)
            .expect("configured port should remain valid");

        assert_eq!(config.server.port, 8443);
        assert_eq!(
            state.entry("server.port").map(|entry| &entry.source),
            Some(&ConfigSource::UserFile)
        );
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

[self_update]
enabled = false
check_on_startup = true

[output]
theme = "old-theme"
theme_file = "/machine/specific/themes.toml"
"#,
        )
        .expect("test TOML should parse");
        let mut config = AppConfig {
            aliases: serde_json::from_value(serde_json::json!({
                "outdated-kernels": "object list --class Hosts | C"
            }))
            .expect("command aliases should deserialize"),
            ..AppConfig::default()
        };
        config.output.theme = "hubuum-light".to_string();
        let preferences = UserPreferences::from_config(&config);

        merge_user_preferences(&mut root, &preferences).expect("preferences should merge");

        assert_eq!(
            toml_get(&root, "self_update.enabled").and_then(TomlValue::as_bool),
            Some(false)
        );
        assert_eq!(
            toml_get(&root, "self_update.check_on_startup").and_then(TomlValue::as_bool),
            Some(true)
        );

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
        assert_eq!(
            toml_get(&root, "aliases.outdated-kernels").and_then(TomlValue::as_str),
            Some("object list --class Hosts | C")
        );
    }
}
