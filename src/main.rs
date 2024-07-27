use std::collections::HashMap;
use std::fmt::Display;

use rustyline::highlight::Highlighter;
use rustyline::{hint::Hinter, validate::Validator, Editor, Helper};

use rustyline::completion::{Completer, Pair};
use rustyline::Context;

mod commands;
mod errors;
mod tokenizer;

use commands::CliCommand;
use log::{debug, trace};

struct CommandList {
    commands: HashMap<String, Box<dyn CliCommand>>,
    scopes: HashMap<String, CommandList>,
}

impl Display for CommandList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let command_names: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        let scope_names: Vec<&str> = self.scopes.keys().map(String::as_str).collect();

        write!(
            f,
            "Commands: {}. Scopes: {}.",
            command_names.join(", "),
            scope_names.join(", ")
        )
    }
}

impl CommandList {
    pub fn new() -> Self {
        CommandList {
            commands: HashMap::new(),
            scopes: HashMap::new(),
        }
    }

    pub fn add_command<T: CliCommand + 'static>(&mut self, name: &str, command: T) {
        debug!("Adding command: {}", name);
        self.commands.insert(name.to_string(), Box::new(command));
    }

    pub fn add_scope(&mut self, name: &str) -> &mut CommandList {
        debug!("Adding scope: {}", name);
        self.scopes
            .entry(name.to_string())
            .or_insert_with(CommandList::new)
    }

    pub fn get_command(&self, name: &str) -> Option<&Box<dyn CliCommand>> {
        debug!("Getting command: {}", name);
        self.commands.get(name)
    }

    pub fn get_scope(&self, name: &str) -> Option<&CommandList> {
        debug!("Getting scope: {}", name);
        self.scopes.get(name)
    }

    pub fn get_completions(&self, prefix: &str) -> Vec<Pair> {
        let mut commands_completions: Vec<Pair> = self
            .commands
            .keys()
            .filter(|name| name.starts_with(prefix))
            .map(|name| Pair {
                display: name.to_string(),
                replacement: name.to_string(),
            })
            .collect();

        let scopes_completions: Vec<Pair> = self
            .scopes
            .keys()
            .filter(|name| name.starts_with(prefix))
            .map(|name| Pair {
                display: name.to_string(),
                replacement: name.to_string(),
            })
            .collect();

        commands_completions.extend(scopes_completions);
        commands_completions
    }
}

fn build_cli() -> CommandList {
    let mut cli = CommandList::new();
    let class_commands = cli.add_scope("class");
    class_commands.add_command("create", commands::ClassNew::default());
    let namespace_commands = cli.add_scope("namespace");
    namespace_commands.add_command("create", commands::NamespaceNew::default());
    cli
}

impl Validator for CommandList {}
impl Helper for CommandList {}
impl Highlighter for CommandList {}
impl Hinter for CommandList {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Completer for CommandList {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let (start, word) = line[..pos]
            .rsplit_once(char::is_whitespace)
            .map_or((0, line), |(_, w)| (pos - w.len(), w));
        let mut completions = Vec::new();
        trace!(
            "Completing. Line: {}, Pos: {}, Start: {}, Word: {}",
            line,
            pos,
            start,
            word,
        );
        let parts = shlex::split(&line[..pos]);
        // If we can't split the line, return no completions. This typically happens if
        // we're in the middle of a quoted string.
        if parts.is_none() {
            return Ok((start, completions));
        }
        let parts = parts.unwrap();
        trace!("Parts: {:?}", parts);

        let mut current_scope = self;
        let mut command = None;

        // First, try to complete the root commands
        /*        if parts.len() == 1 {
            trace!("Completing root commands from {}", word);
            completions.extend(self.get_root_completion(word));
            return Ok((start, completions));
        } */

        let mut options_start_at = 0;
        for (i, part) in parts.iter().enumerate() {
            if let Some(scope) = current_scope.get_scope(part) {
                current_scope = scope;
            } else if let Some(cmd) = current_scope.get_command(part) {
                command = Some(cmd);
                options_start_at = i;
                break;
            } else {
                trace!("Invalid part: {}", part);
                // Invalid part, stop completion
                break;
            }
        }

        let mut options_seen: Vec<String> = Vec::new();
        if command.is_some() {
            for part in parts.iter().skip(options_start_at) {
                if part.starts_with("-") {
                    options_seen.push(part.to_string());
                }
            }
        }

        if command.is_some() {
            // If we have a command but no completions, suggest option completions
            trace!("Completing options from command: {}", word);
            completions.extend(command.unwrap().get_option_completions(word, &options_seen));
        } else {
            // If we have no completions, suggest everything in the current scope
            completions.extend(current_scope.get_completions(word));
        }

        Ok((start, completions))
    }
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
                let parts: Vec<&str> = line.split_whitespace().collect();
                trace!("Parts: {:?}", parts);
                if parts.is_empty() {
                    continue;
                }

                let mut current_scope = &cli;
                let mut command = None;

                for part in parts.iter() {
                    if let Some(scope) = current_scope.get_scope(part) {
                        current_scope = scope;
                    } else if let Some(cmd) = current_scope.get_command(part) {
                        command = Some(cmd);
                        break;
                    } else {
                        println!("Invalid command: {}", part);
                        break;
                    }
                }

                if let Some(cmd) = command {
                    let cmd = cmd.as_ref();
                    trace!("Executing command: {}", cmd.name());
                    let tokens = match tokenizer::CommandTokenizer::new(line.as_str()) {
                        Ok(tokens) => tokens,
                        Err(err) => {
                            println!("Error parsing input: {:?}", err);
                            continue;
                        }
                    };
                    let result = cmd.execute(&tokens);
                    match result {
                        Ok(_) => println!("Command executed successfully"),
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
