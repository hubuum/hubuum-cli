// src/cli.rs
use crate::config::AppConfig;
use crate::models::{
    EmptyResult, OutputColor, Protocol, TableBands, TableStyle, TableWidth, TableWrap,
};
use clap::builder::BoolishValueParser;
use clap::parser::ValueSource;
use clap::{value_parser, Arg, ArgMatches, Command};
use shlex::try_quote;
use std::path::PathBuf;

pub fn build_cli() -> Command {
    Command::new("Hubuum CLI")
        .version(crate::build_info::VERSION)
        .disable_version_flag(false)
        .after_help(
            "Commands:\n  hubuum-cli <command...>        Run one command and exit\n  hubuum-cli script <file>       Run commands from a file and exit\n\nExamples:\n  hubuum-cli object list --limit 5\n  hubuum-cli config show \\| P key value \\| L 5\n  hubuum-cli object list --class Hosts \\| G os_version AS \"OS Version\" \\| A count AS Hosts\n  hubuum-cli object list --json --class Hosts \\| P Name os_version \\> each:/tmp/host-{Name}.json\n  hubuum-cli config show '>>' config.log\n  hubuum-cli theme list\n  hubuum-cli help --tree\n\nIn POSIX shells, escape or quote |, >, and >> so the operators reach Hubuum CLI.",
        )
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
            Arg::new("identity_scope")
                .long("identity-scope")
                .value_name("PROVIDER")
                .env("HUBUUM_CLI__SERVER__IDENTITY_SCOPE")
                .help("Set the authentication provider or identity scope"),
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
            Arg::new("color")
                .long("color")
                .value_name("WHEN")
                .value_parser(["auto", "always", "never"])
                .env("HUBUUM_CLI__OUTPUT__COLOR")
                .help("Control colored output (auto, always, never)"),
        )
        .arg(
            Arg::new("theme")
                .long("theme")
                .value_name("NAME")
                .env("HUBUUM_CLI__OUTPUT__THEME")
                .help("Set the color theme"),
        )
        .arg(
            Arg::new("theme_file")
                .long("theme-file")
                .value_name("FILE")
                .env("HUBUUM_CLI__OUTPUT__THEME_FILE")
                .help("Load additional color themes from a TOML file"),
        )
        .arg(
            Arg::new("table_style")
                .long("table-style")
                .value_name("STYLE")
                .value_parser(["ascii", "compact", "dense", "markdown", "plain", "rounded"])
                .env("HUBUUM_CLI__OUTPUT__TABLE_STYLE")
                .help("Set table borders (ascii, compact, dense, markdown, plain, rounded)"),
        )
        .arg(
            Arg::new("table_width")
                .long("table-width")
                .value_name("WIDTH")
                .env("HUBUUM_CLI__OUTPUT__TABLE_WIDTH")
                .help("Set table width (auto, full, or a number)"),
        )
        .arg(
            Arg::new("table_wrap")
                .long("table-wrap")
                .value_name("WIDTH")
                .env("HUBUUM_CLI__OUTPUT__TABLE_WRAP")
                .help("Set table cell wrapping (auto, never, or a number)"),
        )
        .arg(
            Arg::new("table_bands")
                .long("table-bands")
                .value_name("WHEN")
                .value_parser(["auto", "always", "never"])
                .env("HUBUUM_CLI__OUTPUT__TABLE_BANDS")
                .help("Set dense table row bands (auto, always, never)"),
        )
        .arg(
            Arg::new("empty_result")
                .long("empty-result")
                .value_name("MODE")
                .value_parser(["message", "silent"])
                .env("HUBUUM_CLI__OUTPUT__EMPTY_RESULT")
                .help("Set empty table output (message or silent)"),
        )
        .arg(
            Arg::new("command")
                .long("command")
                .value_name("COMMAND")
                .conflicts_with("source")
                .hide(true)
                .help("Run a command and exit"),
        )
        .arg(
            Arg::new("source")
                .long("source")
                .value_name("FILE")
                .conflicts_with("command")
                .hide(true)
                .help("Run commands from a file and exit"),
        )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Repl,
    Command(String),
    Script(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupArgs {
    pub clap_args: Vec<String>,
    pub mode: StartupMode,
}

pub fn split_startup_args<I, S>(args: I) -> StartupArgs
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let Some(program) = args.first().cloned() else {
        return StartupArgs {
            clap_args: vec!["hubuum-cli".to_string()],
            mode: StartupMode::Repl,
        };
    };

    if has_legacy_execution_arg(&args) {
        return StartupArgs {
            clap_args: args,
            mode: StartupMode::Repl,
        };
    }

    let mut clap_args = vec![program];
    let mut idx = 1;
    while idx < args.len() {
        let arg = &args[idx];
        if is_help_or_version(arg) {
            clap_args.extend(args[idx..].iter().cloned());
            return StartupArgs {
                clap_args,
                mode: StartupMode::Repl,
            };
        }

        if is_global_option_with_value(arg) {
            clap_args.push(arg.clone());
            if !arg.contains('=') {
                if let Some(value) = args.get(idx + 1) {
                    clap_args.push(value.clone());
                    idx += 1;
                }
            }
            idx += 1;
            continue;
        }

        if is_global_bool_option(arg) {
            clap_args.push(arg.clone());
            if !arg.contains('=')
                && args
                    .get(idx + 1)
                    .is_some_and(|value| parse_boolish(value).is_some())
            {
                clap_args.push(args[idx + 1].clone());
                idx += 1;
            }
            idx += 1;
            continue;
        }

        let command_args = &args[idx..];
        if command_args.first().is_some_and(|arg| arg == "script") {
            return StartupArgs {
                clap_args,
                mode: command_args
                    .get(1)
                    .cloned()
                    .map(StartupMode::Script)
                    .unwrap_or_else(|| StartupMode::Command(join_command_args(command_args))),
            };
        }

        return StartupArgs {
            clap_args,
            mode: StartupMode::Command(join_command_args(command_args)),
        };
    }

    StartupArgs {
        clap_args,
        mode: StartupMode::Repl,
    }
}

pub fn execution_mode(matches: &ArgMatches, startup_mode: StartupMode) -> StartupMode {
    if let Some(command) = matches.get_one::<String>("command") {
        return StartupMode::Command(command.clone());
    }

    if let Some(filename) = matches.get_one::<String>("source") {
        return StartupMode::Script(filename.clone());
    }

    startup_mode
}

fn has_legacy_execution_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--command"
            || arg.starts_with("--command=")
            || arg == "--source"
            || arg.starts_with("--source=")
    })
}

fn is_help_or_version(arg: &str) -> bool {
    matches!(arg, "-h" | "--help" | "-V" | "--version")
}

fn is_global_option_with_value(arg: &str) -> bool {
    let key = arg.split('=').next().unwrap_or(arg);
    matches!(
        key,
        "--config"
            | "--hostname"
            | "--port"
            | "--protocol"
            | "--identity-scope"
            | "--username"
            | "--password"
            | "--cache-time"
            | "--cache-size"
            | "--background-poll-interval"
            | "--relations-max-depth"
            | "--color"
            | "--theme"
            | "--theme-file"
            | "--table-style"
            | "--table-width"
            | "--table-wrap"
            | "--table-bands"
            | "--empty-result"
    )
}

fn is_global_bool_option(arg: &str) -> bool {
    let key = arg.split('=').next().unwrap_or(arg);
    matches!(
        key,
        "--ssl-validation"
            | "--cache-disable"
            | "--completion-api-disable"
            | "--relations-ignore-same-class"
            | "--output-object-show-data"
    )
}

fn parse_boolish(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Some(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn join_command_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if matches!(arg.as_str(), "|" | ">" | ">>") {
                return arg.clone();
            }
            try_quote(arg)
                .map(|quoted| quoted.into_owned())
                .unwrap_or_else(|_| arg.replace('\0', ""))
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    if let Some(identity_scope) = get_command_line_value::<String>(matches, "identity_scope") {
        config.server.identity_scope = Some(identity_scope.to_string());
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
    if let Some(color) = get_command_line_value::<String>(matches, "color") {
        config.output.color = color.parse().unwrap_or(OutputColor::Auto);
    }
    if let Some(theme) = get_command_line_value::<String>(matches, "theme") {
        config.output.theme = theme.to_string();
    }
    if let Some(theme_file) = get_command_line_value::<String>(matches, "theme_file") {
        config.output.theme_file = theme_file.to_string();
    }
    if let Some(table_style) = get_command_line_value::<String>(matches, "table_style") {
        config.output.table_style = table_style.parse().unwrap_or(TableStyle::Rounded);
    }
    if let Some(table_width) = get_command_line_value::<String>(matches, "table_width") {
        config.output.table_width = table_width.parse().unwrap_or(TableWidth::Auto);
    }
    if let Some(table_wrap) = get_command_line_value::<String>(matches, "table_wrap") {
        config.output.table_wrap = table_wrap.parse().unwrap_or(TableWrap::Auto);
    }
    if let Some(table_bands) = get_command_line_value::<String>(matches, "table_bands") {
        config.output.table_bands = table_bands.parse().unwrap_or(TableBands::Auto);
    }
    if let Some(empty_result) = get_command_line_value::<String>(matches, "empty_result") {
        config.output.empty_result = empty_result.parse().unwrap_or(EmptyResult::Message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env::{remove_var, set_var};

    #[test]
    #[serial]
    fn update_config_from_cli_ignores_env_backed_values() {
        set_var("HUBUUM_CLI__SERVER__HOSTNAME", "env.example.com");

        let matches = build_cli()
            .try_get_matches_from(["hubuum-cli"])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert_eq!(config.server.hostname, AppConfig::default().server.hostname);

        remove_var("HUBUUM_CLI__SERVER__HOSTNAME");
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
    fn update_config_from_cli_applies_identity_scope() {
        let matches = build_cli()
            .try_get_matches_from(["hubuum-cli", "--identity-scope", "corp-directory"])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert_eq!(
            config.server.identity_scope.as_deref(),
            Some("corp-directory")
        );
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

    #[test]
    fn update_config_from_cli_applies_color_flag() {
        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--color",
                "never",
                "--theme",
                "catppuccin-mocha",
                "--theme-file",
                "/tmp/themes.toml",
            ])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert_eq!(config.output.color, OutputColor::Never);
        assert_eq!(config.output.theme, "catppuccin-mocha");
        assert_eq!(config.output.theme_file, "/tmp/themes.toml");
    }

    #[test]
    fn update_config_from_cli_applies_table_flags() {
        let matches = build_cli()
            .try_get_matches_from([
                "hubuum-cli",
                "--table-style",
                "plain",
                "--table-width",
                "100",
                "--table-wrap",
                "never",
                "--table-bands",
                "always",
                "--empty-result",
                "silent",
            ])
            .expect("cli should parse");
        let mut config = AppConfig::default();
        update_config_from_cli(&mut config, &matches);

        assert_eq!(config.output.table_style, TableStyle::Plain);
        assert_eq!(config.output.table_width, TableWidth::Fixed(100));
        assert_eq!(config.output.table_wrap, TableWrap::Never);
        assert_eq!(config.output.table_bands, TableBands::Always);
        assert_eq!(config.output.empty_result, EmptyResult::Silent);
    }

    #[test]
    fn split_startup_args_extracts_direct_command_after_global_flags() {
        let startup = split_startup_args([
            "hubuum-cli",
            "--hostname",
            "api.example.com",
            "--identity-scope",
            "corp-directory",
            "--table-style",
            "plain",
            "object",
            "list",
            "--limit",
            "5",
        ]);

        assert_eq!(
            startup.clap_args,
            vec![
                "hubuum-cli",
                "--hostname",
                "api.example.com",
                "--identity-scope",
                "corp-directory",
                "--table-style",
                "plain"
            ]
        );
        assert_eq!(
            startup.mode,
            StartupMode::Command("object list --limit 5".to_string())
        );
    }

    #[test]
    fn split_startup_args_preserves_pipe_token_for_direct_command() {
        let startup = split_startup_args([
            "hubuum-cli",
            "object",
            "list",
            "--class",
            "Hosts",
            "|",
            "tornar",
        ]);

        assert_eq!(
            startup.mode,
            StartupMode::Command("object list --class Hosts | tornar".to_string())
        );
    }

    #[test]
    fn split_startup_args_preserves_redirect_tokens_for_direct_command() {
        let truncate = split_startup_args(["hubuum-cli", "help", ">", "help.txt"]);
        assert_eq!(
            truncate.mode,
            StartupMode::Command("help > help.txt".to_string())
        );

        let append = split_startup_args(["hubuum-cli", "config", "show", ">>", "config.log"]);
        assert_eq!(
            append.mode,
            StartupMode::Command("config show >> config.log".to_string())
        );
    }

    #[test]
    fn split_startup_args_supports_script_mode() {
        let startup = split_startup_args(["hubuum-cli", "script", "commands.hubuum"]);

        assert_eq!(startup.clap_args, vec!["hubuum-cli"]);
        assert_eq!(
            startup.mode,
            StartupMode::Script("commands.hubuum".to_string())
        );
    }

    #[test]
    fn legacy_command_flag_remains_clap_handled() {
        let startup = split_startup_args(["hubuum-cli", "--command", "help"]);

        assert_eq!(startup.clap_args, vec!["hubuum-cli", "--command", "help"]);
        assert_eq!(startup.mode, StartupMode::Repl);

        let matches = build_cli()
            .try_get_matches_from(startup.clap_args)
            .expect("legacy command should parse");
        assert_eq!(
            execution_mode(&matches, startup.mode),
            StartupMode::Command("help".to_string())
        );
    }
}
