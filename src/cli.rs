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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

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
    fn cli_rejects_invalid_port_type() {
        let result = build_cli().try_get_matches_from(["hubuum-cli", "--port", "not-a-number"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_updates_typed_values() {
        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--port",
                "4321",
                "--ssl-validation",
                "false",
                "--cache-time",
                "99",
                "--cache-size",
                "42",
                "--cache-disable",
                "true",
                "--completion-api-disable",
                "true",
                "--background-poll-interval",
                "17",
            ])
            .expect("valid CLI args should parse");

        let mut cfg = AppConfig::default();
        update_config_from_cli(&mut cfg, &matches);

        assert_eq!(cfg.server.port, 4321);
        assert!(!cfg.server.ssl_validation);
        assert_eq!(cfg.cache.time, 99);
        assert_eq!(cfg.cache.size, 42);
        assert!(cfg.cache.disable);
        assert!(cfg.completion.disable_api_related);
        assert_eq!(cfg.background.poll_interval_seconds, 17);
    }

    #[test]
    fn cli_accepts_boolish_one_zero_values() {
        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--ssl-validation",
                "0",
                "--cache-disable",
                "1",
            ])
            .expect("boolish values should parse");

        let mut cfg = AppConfig::default();
        update_config_from_cli(&mut cfg, &matches);

        assert!(!cfg.server.ssl_validation);
        assert!(cfg.cache.disable);
    }

    #[rstest]
    #[case(vec!["hubuum-cli", "--ssl-validation", "not-bool"])]
    #[case(vec!["hubuum-cli", "--protocol", "ftp"])]
    #[case(vec![
        "hubuum-cli",
        "--command",
        "class list",
        "--source",
        "commands.txt",
    ])]
    fn cli_rejects_invalid_inputs(#[case] args: Vec<&str>) {
        let result = build_cli().try_get_matches_from(args);
        assert!(result.is_err());
    }
}
