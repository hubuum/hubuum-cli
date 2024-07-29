use config::AppConfig;
use errors::AppError;
use log::{debug, trace};
use output::{add_error, add_warning, clear_filter, flush_output, set_filter};
use rustyline::history::FileHistory;
use rustyline::Editor;

mod cli;
mod commandlist;
mod commands;
mod config;
mod defaults;
mod errors;
mod files;
mod models;
mod output;
mod tokenizer;

use crate::commandlist::CommandList;
use crate::files::get_history_file;

fn build_repl_commands() -> CommandList {
    let mut cli = CommandList::new();
    cli.add_scope("class")
        .add_command("create", commands::ClassNew::default());
    cli.add_scope("namespace")
        .add_command("create", commands::NamespaceNew::default());
    cli.add_command("help", commands::Help::default());
    cli
}

fn process_filter(line: &str) -> Result<String, AppError> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() > 1 {
        let filter = parts[1].trim();
        let (invert, pattern) = if filter.starts_with('!') {
            (true, filter[1..].trim())
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
) -> Result<(), AppError> {
    let parts = shlex::split(line)
        .ok_or_else(|| AppError::ParseError("Parsing input failed".to_string()))?;
    if parts.is_empty() {
        return Ok(());
    }

    let (command, cmd_name) = find_command(cli, &parts, context)?;

    if let Some(cmd) = command {
        execute_command(cmd, cmd_name, line, context)
    } else {
        add_warning(format!("Command not found: {}", parts.join(" ")))
    }
}

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

fn execute_command(
    cmd: &Box<dyn commands::CliCommand>,
    cmd_name: Option<&str>,
    line: &str,
    context: &[String],
) -> Result<(), AppError> {
    debug!("Executing command: {:?} {}", context, cmd_name.unwrap());
    let tokens = tokenizer::CommandTokenizer::new(line, cmd_name.unwrap())?;
    trace!("Tokens: {:?}", tokens);

    let options = tokens.get_options();
    if options.contains_key("help") || options.contains_key("h") {
        cmd.help(&cmd_name.unwrap().to_string(), context)
    } else {
        cmd.execute(&tokens).map_err(|err| {
            AppError::CommandExecutionError(format!("Error executing command: {:?}", err))
        })
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

fn main() -> Result<(), AppError> {
    env_logger::init();

    let matches = cli::build_cli().get_matches();
    let cli_config_path = cli::get_cli_config_path(&matches);
    let mut config = config::load_config(cli_config_path)?;
    cli::update_config_from_cli(&mut config, &matches);

    let cli = build_repl_commands();
    let mut rl = create_editor(&cli)?;

    loop {
        match rl.readline(&prompt(&config)) {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                rl.save_history(&get_history_file()?)?;
                let line = process_filter(line.as_str())?;
                let mut context = Vec::new();
                if let Err(err) = handle_command(&cli, &line, &mut context) {
                    add_error(err.to_string())?;
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(err) => return Err(AppError::from(err)),
        }
        flush_output()?;
    }
    Ok(())
}
