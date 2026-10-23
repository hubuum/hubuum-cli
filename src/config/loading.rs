use config::{Config, ConfigError, Environment, File};
use std::collections::HashMap;
use std::mem::take;
use std::path::PathBuf;

use crate::defaults::Defaults;
use crate::files::{get_system_config_path, get_user_config_path};

use super::{
    default_system_extension_roots, default_user_extension_roots, AppConfig, OutputConfig,
};

pub fn load_config(cli_config_path: Option<PathBuf>) -> Result<AppConfig, ConfigError> {
    let system_config = get_system_config_path();
    let user_config = get_user_config_path();

    let mut builder = Config::builder()
        // Start with default values
        .set_default("aliases", HashMap::<String, String>::new())?
        .set_default(
            "extensions.system_roots",
            default_system_extension_roots()
                .into_iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
        )?
        .set_default(
            "extensions.user_roots",
            default_user_extension_roots()
                .into_iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
        )?
        .set_default("extensions.disabled", Vec::<String>::new())?
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
            "output.table_headers",
            Defaults::OUTPUT_TABLE_HEADERS.to_string(),
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
        .set_default(
            "server.max_response_body_bytes",
            Defaults::max_response_body_bytes().get() as u64,
        )?
        .set_default("server.api_version", Defaults::API_VERSION)?
        .set_default("server.username", Defaults::USER_USERNAME)?
        .set_default("server.protocol", Defaults::PROTOCOL)?
        .set_default("cache.time", Defaults::CACHE_TIME)?
        .set_default("cache.size", Defaults::CACHE_SIZE)?
        .set_default("cache.disable", Defaults::CACHE_DISABLE)?
        .set_default("settings.store_on_server", false)?
        .set_default("self_update.enabled", Defaults::SELF_UPDATE_ENABLED)?
        .set_default(
            "self_update.check_on_startup",
            Defaults::SELF_UPDATE_CHECK_ON_STARTUP,
        )?
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

pub(super) fn merge_legacy_object_list_class_aliases(output: &mut OutputConfig) {
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
