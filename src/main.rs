use std::str::FromStr;

use config::AppConfig;
use errors::AppError;
use files::get_log_file;
use hubuum_client::{ApiError, Authenticated, Credentials, SyncClient, Token, Unauthenticated};
use log::{debug, trace};
use logger::with_timing;
use output::{add_error, add_warning, clear_filter, flush_output, set_filter};
use rustyline::history::FileHistory;
use rustyline::Editor;
use tracing_subscriber::EnvFilter;

mod cli;
mod commandlist;
mod commands;
mod config;
mod defaults;
mod errors;
mod files;
mod formatting;
mod logger;
mod models;
mod output;
mod tokenizer;
mod traits;

use crate::commandlist::CommandList;
use crate::files::get_history_file;
use crate::models::internal::TokenEntry;

fn process_filter(line: &str) -> Result<String, AppError> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() > 1 {
        let filter = parts[1].trim();
        let (invert, pattern) = if let Some(stripped) = filter.strip_prefix('!') {
            (true, stripped.trim())
        } else {
            (false, filter.trim())
        };
        set_filter(pattern.to_string(), invert)?;
        Ok(parts[0].trim().to_string())
    } else {
        clear_filter()?;
        Ok(line.to_string())
    }
}

fn prompt(config: &AppConfig) -> String {
    format!(
        "{}@{}:{} > ",
        config.server.username, config.server.hostname, config.server.port
    )
}

fn handle_command(
    cli: &CommandList,
    line: &str,
    context: &mut Vec<String>,
    client: &SyncClient<Authenticated>,
) -> Result<(), AppError> {
    let parts = shlex::split(line)
        .ok_or_else(|| AppError::ParseError("Parsing input failed".to_string()))?;
    if parts.is_empty() {
        return Ok(());
    }

    let (command, cmd_name) = find_command(cli, &parts, context)?;

    if let Some(cmd) = command {
        let command_string = format!("Command {:?}", parts.join(" "));
        with_timing(&command_string, || {
            execute_command(cmd, cmd_name, line, context, client)
        })
    } else {
        add_warning(format!("Command not found: {}", parts.join(" ")))
    }
}

#[allow(clippy::type_complexity)]
fn find_command<'a>(
    cli: &'a CommandList,
    parts: &'a [String],
    context: &mut Vec<String>,
) -> Result<(Option<&'a Box<dyn commands::CliCommand>>, Option<&'a str>), AppError> {
    let mut current_scope = cli;
    let mut command = None;
    let mut cmd_name = None;

    for part in parts {
        if let Some(scope) = current_scope.get_scope(part) {
            context.push(part.to_string());
            current_scope = scope;
        } else if let Some(cmd) = current_scope.get_command(part) {
            command = Some(cmd);
            cmd_name = Some(part.as_str());
            break;
        } else {
            return Err(AppError::CommandNotFound(format!(
                "Command not found: {}",
                part
            )));
        }
    }

    Ok((command, cmd_name))
}

#[allow(clippy::borrowed_box)]
fn execute_command(
    cmd: &Box<dyn commands::CliCommand>,
    cmd_name: Option<&str>,
    line: &str,
    context: &[String],
    client: &SyncClient<Authenticated>,
) -> Result<(), AppError> {
    debug!("Executing command: {:?} {}", context, cmd_name.unwrap());
    let tokens = tokenizer::CommandTokenizer::new(line, cmd_name.unwrap())?;
    trace!("Tokens: {:?}", tokens);

    let options = tokens.get_options();
    if options.contains_key("help") || options.contains_key("h") {
        cmd.help(&cmd_name.unwrap().to_string(), context)
    } else {
        cmd.execute(client, &tokens)
    }
}

fn create_editor(cli: &CommandList) -> Result<Editor<&CommandList, FileHistory>, AppError> {
    let repl_config = rustyline::Config::builder()
        .history_ignore_space(true)
        .completion_type(rustyline::CompletionType::List)
        .build();

    let mut rl = Editor::with_config(repl_config)?;
    rl.set_helper(Some(cli));
    rl.load_history(&get_history_file()?)?;
    Ok(rl)
}

fn login(
    client: hubuum_client::SyncClient<Unauthenticated>,
    username: &str,
    hostname: &str,
) -> Result<SyncClient<Authenticated>, AppError> {
    let token = files::get_token_from_tokenfile(hostname, username)?;
    if let Some(token) = token {
        debug!("Found existing token, testing validity...");
        match client.clone().login_with_token(Token { token }) {
            Ok(client) => return Ok(client.clone()),
            Err(err) => {
                add_warning(format!("Error logging in with existing token: {}", err))?;
                flush_output()?;
            }
        }
    }

    let password =
        rpassword::prompt_password(format!("Password for {} @ {}: ", username, hostname))?;
    let client = client
        .clone()
        .login(Credentials::new(username.to_string(), password))?;
    debug!("Logged in successfully, saving token...");
    files::write_token_to_tokenfile(TokenEntry {
        hostname: hostname.to_string(),
        username: username.to_string(),
        token: client.get_token().to_string(),
    })?;
    Ok(client)
}

fn main() -> Result<(), AppError> {
    let file = get_log_file()?;
    let file = std::fs::File::create(file).expect("Failed to create log file");

    // Set up the tracing subscriber
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let matches = cli::build_cli().get_matches();
    let cli_config_path = cli::get_cli_config_path(&matches);
    let mut config = config::load_config(cli_config_path)?;
    cli::update_config_from_cli(&mut config, &matches);

    let cli = crate::commands::build_repl_commands();
    let mut rl = create_editor(&cli)?;

    let baseurl = hubuum_client::BaseUrl::from_str(&format!(
        "{}://{}:{}",
        config.server.protocol, config.server.hostname, config.server.port
    ))?;
    let client = hubuum_client::SyncClient::new(baseurl);

    let client = login(
        client,
        config.server.username.as_str(),
        config.server.hostname.as_str(),
    )?;

    loop {
        match rl.readline(&prompt(&config)) {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                rl.save_history(&get_history_file()?)?;
                let line = process_filter(line.as_str())?;
                let mut context = Vec::new();
                match handle_command(&cli, &line, &mut context, &client) {
                    Ok(_) => {}
                    Err(AppError::Quiet) => {}
                    Err(AppError::EntityNotFound(entity)) => {
                        add_warning(format!("{}", entity))?;
                    }
                    Err(AppError::ApiError(ApiError::HttpWithBody { status, message })) => {
                        add_error(format!("API Error: Status {} - {}", status, message))?;
                    }
                    Err(err @ AppError::ApiError(_)) => {
                        add_error(format!("API Error: {}", err))?;
                    }
                    Err(err) => add_error(err)?,
                };
            }
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(err) => return Err(AppError::from(err)),
        }
        flush_output()?;
    }
    Ok(())
}
