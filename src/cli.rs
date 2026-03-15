// src/cli.rs
use crate::config::AppConfig;
use crate::models::Protocol;
use clap::builder::BoolishValueParser;
use clap::parser::ValueSource;
use clap::{value_parser, Arg, ArgMatches, Command};
use std::path::PathBuf;

pub fn build_cli() -> Command {
    Command::new("Hubuum CLI")
        .arg(
            Arg::new("config")
                .long("config")
                .value_name("FILE")
                .help("Specify a custom configuration file"),
        )
        .arg(
            Arg::new("hostname")
                .long("hostname")
                .value_name("HOST")
                .env("HUBUUM_CLI__SERVER__HOSTNAME")
                .help("Set the server hostname"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .value_name("PORT")
                .value_parser(value_parser!(u16))
                .env("HUBUUM_CLI__SERVER__PORT")
                .help("Set the server port"),
        )
        .arg(
            Arg::new("protocol")
                .long("protocol")
                .value_name("PROTOCOL")
                .env("HUBUUM_CLI__SERVER__PROTOCOL")
                .ignore_case(true)
                .value_parser(["http", "https"])
                .help("Set the server protocol (http or https)"),
        )
        .arg(
            Arg::new("ssl_validation")
                .long("ssl-validation")
                .value_name("BOOL")
                .value_parser(BoolishValueParser::new())
                .env("HUBUUM_CLI__SERVER__SSL_VALIDATION")
                .help("Enable or disable SSL validation"),
        )
        .arg(
            Arg::new("username")
                .long("username")
                .value_name("NAME")
                .env("HUBUUM_CLI__SERVER__USERNAME")
                .help("Set the username"),
        )
        .arg(
            Arg::new("password")
                .long("password")
                .value_name("PASSWORD")
                .env("HUBUUM_CLI__SERVER__PASSWORD")
                .help("Set the password (ideally use ENV)"),
        )
        .arg(
            Arg::new("cache_time")
                .long("cache-time")
                .value_name("SECONDS")
                .value_parser(value_parser!(u64))
                .env("HUBUUM_CLI__CACHE__TIME")
                .help("Set the cache time in seconds"),
        )
        .arg(
            Arg::new("cache_size")
                .long("cache-size")
                .value_name("BYTES")
                .value_parser(value_parser!(i32))
                .env("HUBUUM_CLI__CACHE__SIZE")
                .help("Set the cache size in bytes"),
        )
        .arg(
            Arg::new("cache_disable")
                .long("cache-disable")
                .value_name("BOOL")
                .value_parser(BoolishValueParser::new())
                .env("HUBUUM_CLI__CACHE__DISABLE")
                .help("Enable or disable caching"),
        )
        .arg(
            Arg::new("completion_disable_api")
                .long("completion-api-disable")
                .value_name("BOOL")
                .value_parser(BoolishValueParser::new())
                .env("HUBUUM_CLI__COMPLETION__DISABLE_API_RELATED")
                .help("Disable API-related completions"),
        )
        .arg(
            Arg::new("background_poll_interval")
                .long("background-poll-interval")
                .value_name("SECONDS")
                .value_parser(value_parser!(u64))
                .env("HUBUUM_CLI__BACKGROUND__POLL_INTERVAL_SECONDS")
                .help("Set the background task poll interval in seconds"),
        )
        .arg(
            Arg::new("relations_ignore_same_class")
                .long("relations-ignore-same-class")
                .value_name("BOOL")
                .value_parser(BoolishValueParser::new())
                .env("HUBUUM_CLI__RELATIONS__IGNORE_SAME_CLASS")
                .help("Set whether same-class relations are ignored by default"),
        )
        .arg(
            Arg::new("relations_max_depth")
                .long("relations-max-depth")
                .value_name("DEPTH")
                .value_parser(value_parser!(i32))
                .env("HUBUUM_CLI__RELATIONS__MAX_DEPTH")
                .help("Set the default relation traversal depth"),
        )
        .arg(
            Arg::new("output_object_show_data")
                .long("output-object-show-data")
                .value_name("BOOL")
                .value_parser(BoolishValueParser::new())
                .env("HUBUUM_CLI__OUTPUT__OBJECT_SHOW_DATA")
                .help("Set whether object show expands object data by default"),
        )
        .arg(
            Arg::new("command")
                .long("command")
                .value_name("COMMAND")
                .conflicts_with("source")
                .help("Run a command and exit"),
        )
        .arg(
            Arg::new("source")
                .long("source")
                .value_name("FILE")
                .conflicts_with("command")
                .help("Run commands from a file and exit"),
        )
}

pub fn get_cli_config_path(matches: &ArgMatches) -> Option<PathBuf> {
    matches.get_one::<String>("config").map(PathBuf::from)
}

fn get_command_line_value<'a, T: Clone + Send + Sync + 'static>(
    matches: &'a ArgMatches,
    arg: &str,
) -> Option<&'a T> {
    (matches.value_source(arg) == Some(ValueSource::CommandLine))
        .then(|| matches.get_one::<T>(arg))
        .flatten()
}

pub fn update_config_from_cli(config: &mut AppConfig, matches: &ArgMatches) {
    if let Some(hostname) = get_command_line_value::<String>(matches, "hostname") {
        config.server.hostname = hostname.to_string();
    }
    if let Some(port) = get_command_line_value::<u16>(matches, "port") {
        config.server.port = *port;
    }
    if let Some(protocol) = get_command_line_value::<String>(matches, "protocol") {
        config.server.protocol = match protocol.as_str() {
            "http" => Protocol::Http,
            "https" => Protocol::Https,
            _ => config.server.protocol.clone(),
        };
    }
    if let Some(ssl_validation) = get_command_line_value::<bool>(matches, "ssl_validation") {
        config.server.ssl_validation = *ssl_validation;
    }
    if let Some(username) = get_command_line_value::<String>(matches, "username") {
        config.server.username = username.to_string();
    }
    if let Some(password) = get_command_line_value::<String>(matches, "password") {
        config.server.password = Some(password.to_string());
    }
    if let Some(cache_time) = get_command_line_value::<u64>(matches, "cache_time") {
        config.cache.time = *cache_time;
    }
    if let Some(cache_size) = get_command_line_value::<i32>(matches, "cache_size") {
        config.cache.size = *cache_size;
    }
    if let Some(cache_disable) = get_command_line_value::<bool>(matches, "cache_disable") {
        config.cache.disable = *cache_disable;
    }
    if let Some(completion_disable_api) =
        get_command_line_value::<bool>(matches, "completion_disable_api")
    {
        config.completion.disable_api_related = *completion_disable_api;
    }
    if let Some(background_poll_interval) =
        get_command_line_value::<u64>(matches, "background_poll_interval")
    {
        config.background.poll_interval_seconds = *background_poll_interval;
    }
    if let Some(ignore_same_class) =
        get_command_line_value::<bool>(matches, "relations_ignore_same_class")
    {
        config.relations.ignore_same_class = *ignore_same_class;
    }
    if let Some(max_depth) = get_command_line_value::<i32>(matches, "relations_max_depth") {
        config.relations.max_depth = *max_depth;
    }
    if let Some(object_show_data) =
        get_command_line_value::<bool>(matches, "output_object_show_data")
    {
        config.output.object_show_data = *object_show_data;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    #[test]
    #[serial]
    fn update_config_from_cli_ignores_env_backed_values() {
        env::set_var("HUBUUM_CLI__SERVER__HOSTNAME", "env.example.com");

        let matches = build_cli()
            .try_get_matches_from(["hubuum-cli"])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert_eq!(config.server.hostname, AppConfig::default().server.hostname);

        env::remove_var("HUBUUM_CLI__SERVER__HOSTNAME");
    }

    #[test]
    fn update_config_from_cli_applies_explicit_flags() {
        let matches = build_cli()
            .try_get_matches_from(["hubuum-cli", "--hostname", "cli.example.com"])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert_eq!(config.server.hostname, "cli.example.com");
    }

    #[test]
    fn update_config_from_cli_applies_relation_and_output_flags() {
        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--relations-ignore-same-class",
                "false",
                "--relations-max-depth",
                "5",
                "--output-object-show-data",
                "true",
            ])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert!(!config.relations.ignore_same_class);
        assert_eq!(config.relations.max_depth, 5);
        assert!(config.output.object_show_data);
    }
}
