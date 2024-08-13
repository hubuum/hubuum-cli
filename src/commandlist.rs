use std::collections::HashMap;
use std::fmt::Display;

use rustyline::highlight::Highlighter;
use rustyline::{hint::Hinter, validate::Validator, Helper};

use rustyline::completion::{Completer, Pair};
use rustyline::Context;

use crate::commands::CliCommand;
use log::{debug, trace};

pub struct CommandList {
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

impl Default for CommandList {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandList {
    pub fn new() -> Self {
        CommandList {
            commands: HashMap::new(),
            scopes: HashMap::new(),
        }
    }

    pub fn add_command<T: CliCommand + 'static>(&mut self, name: &str, command: T) -> &mut Self {
        debug!("Adding command: {}", name);
        self.commands.insert(name.to_string(), Box::new(command));
        self
    }

    pub fn add_scope(&mut self, name: &str) -> &mut CommandList {
        debug!("Adding scope: {}", name);
        self.scopes.entry(name.to_string()).or_default()
    }

    #[allow(clippy::borrowed_box)]
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

    fn generate_tree(&self, prefix: &str, is_last: bool) -> String {
        let mut result = String::new();
        let indent = if prefix.is_empty() { "" } else { "  " };
        let branch = if is_last { "└─ " } else { "├─ " };

        // Add commands
        let command_count = self.commands.len();
        for (i, command_name) in self.commands.keys().enumerate() {
            let line = format!("{}{}{}{}\n", prefix, indent, branch, command_name);
            result.push_str(&line);
            if i < command_count - 1 || !self.scopes.is_empty() {
                result.push_str(&format!("{}{}│\n", prefix, indent));
            }
        }

        // Add scopes
        let scope_count = self.scopes.len();
        for (i, (scope_name, scope)) in self.scopes.iter().enumerate() {
            let line = format!("{}{}{}{}\n", prefix, indent, branch, scope_name);
            result.push_str(&line);
            let new_prefix = format!(
                "{}{}{}",
                prefix,
                indent,
                if i == scope_count - 1 { " " } else { "│" }
            );
            result.push_str(&scope.generate_tree(&new_prefix, i == scope_count - 1));
        }

        result
    }

    pub fn show_tree(&self) -> String {
        self.generate_tree("", true).to_string()
    }
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
                if part.starts_with('-') {
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
