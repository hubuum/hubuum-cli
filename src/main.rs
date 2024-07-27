use rustyline::Editor;

mod commandlist;
mod commands;
mod errors;
mod tokenizer;

use crate::commandlist::CommandList;

use log::{debug, trace};

pub fn build_cli() -> CommandList {
    let mut cli = CommandList::new();
    let class_commands = cli.add_scope("class");
    class_commands.add_command("create", commands::ClassNew::default());
    let namespace_commands = cli.add_scope("namespace");
    namespace_commands.add_command("create", commands::NamespaceNew::default());
    cli.add_command("help", commands::Help::default());
    debug!("CLI: {}", cli);
    cli
}

fn main() -> rustyline::Result<()> {
    env_logger::init();

    let cli = build_cli();
    let repl_config = rustyline::Config::builder()
        .history_ignore_space(true)
        .completion_type(rustyline::CompletionType::List)
        .build();

    let mut rl = Editor::with_config(repl_config)?;
    rl.set_helper(Some(&cli));

    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                let parts = match shlex::split(&line) {
                    Some(parts) => parts,
                    None => {
                        println!("Error parsing input");
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
                        println!("Invalid command: {}", part);
                        break;
                    }
                }

                if let Some(cmd) = command {
                    let cmd = cmd.as_ref();
                    debug!("Executing command: {:?} {}", context, cmd_name.unwrap());
                    let tokens =
                        match tokenizer::CommandTokenizer::new(line.as_str(), cmd_name.unwrap()) {
                            Ok(tokens) => tokens,
                            Err(err) => {
                                println!("Error parsing input: {:?}", err);
                                continue;
                            }
                        };

                    debug!("Tokens: {:?}", tokens);

                    let options = tokens.get_options();
                    if options.contains_key("help") {
                        println!("{}", cmd.help(cmd_name.unwrap(), &context));
                        continue;
                    }

                    let result = cmd.execute(&tokens);
                    match result {
                        Ok(_) => debug!("Command executed successfully"),
                        Err(err) => println!("Error executing command: {:?}", err),
                    }
                } else {
                    println!("Command not found: {}", parts.join(" "));
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
    }
    Ok(())
}
