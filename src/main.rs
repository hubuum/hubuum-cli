use config::AppConfig;
use errors::AppError;
use output::flush_output;
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
use crate::output::{add_error, add_warning, clear_filter, set_filter};

use log::{debug, trace};

pub fn build_repl_commands() -> CommandList {
    let mut cli = CommandList::new();
    let class_commands = cli.add_scope("class");
    class_commands.add_command("create", commands::ClassNew::default());
    let namespace_commands = cli.add_scope("namespace");
    namespace_commands.add_command("create", commands::NamespaceNew::default());
    cli.add_command("help", commands::Help::default());
    cli
}

fn process_filter(line: &str) -> Result<String, AppError> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() > 1 {
        let filter = parts[1].trim();
        let invert = filter.starts_with('!');
        let pattern = if invert { &filter[1..] } else { filter }.trim();
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

fn main() -> Result<(), AppError> {
    env_logger::init();

    let matches = cli::build_cli().get_matches();
    let cli_config_path = cli::get_cli_config_path(&matches);
    let mut config = config::load_config(cli_config_path)?;

    cli::update_config_from_cli(&mut config, &matches);

    let cli = build_repl_commands();
    let repl_config = rustyline::Config::builder()
        .history_ignore_space(true)
        .completion_type(rustyline::CompletionType::List)
        .build();

    let mut rl = Editor::with_config(repl_config)?;
    rl.set_helper(Some(&cli));
    rl.load_history(&get_history_file()?)?;

    loop {
        let readline = rl.readline(&prompt(&config));
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                rl.save_history(&get_history_file()?)?;
                let line = process_filter(line.as_str())?;
                let parts = match shlex::split(&line) {
                    Some(parts) => parts,
                    None => {
                        add_error("Parsing input failed")?;
                        flush_output()?;
                        continue;
                    }
                };
                trace!("Parts: {:?}", parts);
                if parts.is_empty() {
                    continue;
                }

                let mut current_scope = &cli;
                let mut command = None;
                let mut context = Vec::new();
                let mut cmd_name = None;

                for part in parts.iter() {
                    if let Some(scope) = current_scope.get_scope(part) {
                        context.push(part.to_string());
                        current_scope = scope;
                    } else if let Some(cmd) = current_scope.get_command(part) {
                        command = Some(cmd);
                        cmd_name = Some(part);
                        break;
                    } else {
                        add_error(format!("Invalid command: {}", part))?;
                        flush_output()?;
                        break;
                    }
                }

                if let Some(cmd) = command {
                    let cmd = cmd.as_ref();
                    debug!("Executing command: {:?} {}", context, cmd_name.unwrap());
                    match tokenizer::CommandTokenizer::new(line.as_str(), cmd_name.unwrap()) {
                        Ok(tokens) => {
                            debug!("Tokens: {:?}", tokens);

                            let options = tokens.get_options();
                            if options.contains_key("help") || options.contains_key("h") {
                                cmd.help(cmd_name.unwrap(), &context)?;
                            } else {
                                let result = cmd.execute(&tokens);
                                match result {
                                    Ok(_) => debug!("Command executed successfully"),
                                    Err(err) => {
                                        add_error(format!("Error executing command: {:?}", err))?
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            log::error!("Error parsing input: {:?}", err);
                            add_error(format!("Error parsing input: {:?}", err))?;
                        }
                    }
                } else {
                    add_warning(format!("Command not found: {}", parts.join(" ")))?;
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
        flush_output()?;
    }
    Ok(())
}
