use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use hubuum_client::{Authenticated, FilterOperator, SyncClient};
use rustyline::highlight::Highlighter;
use rustyline::{hint::Hinter, validate::Validator, Helper};

use log::{debug, trace, warn};
use rustyline::completion::{Completer, Pair};
use rustyline::Context;

use crate::commands::{CliCommand, CliOption};

pub struct CommandList {
    commands: HashMap<String, Box<dyn CliCommand>>,
    scopes: HashMap<String, CommandList>,
    client: Arc<SyncClient<Authenticated>>,
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
    pub fn new(client: Arc<SyncClient<Authenticated>>) -> Self {
        CommandList {
            commands: HashMap::new(),
            scopes: HashMap::new(),
            client,
        }
    }

    pub fn add_command<T: CliCommand + 'static>(&mut self, name: &str, command: T) -> &mut Self {
        debug!("Adding command: {}", name);
        self.commands.insert(name.to_string(), Box::new(command));
        self
    }

    pub fn add_scope(&mut self, name: &str) -> &mut CommandList {
        debug!("Adding scope: {}", name);
        self.scopes
            .entry(name.to_string())
            .or_insert_with(|| CommandList::new(self.client.clone()))
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
            .map_or((0, &line[..pos]), |(l, r)| (l.len() + 1, r));
        let mut completions = Vec::new();
        trace!(
            "Completing. Line: {}, Pos: {}, Start: {}, Word: {}",
            line,
            pos,
            start,
            word,
        );
        let parts = shlex::split(&line[..pos]);
        if parts.is_none() {
            return Ok((start, completions));
        }
        let parts = parts.unwrap();
        trace!("Parts: {:?}", parts);

        let mut current_scope = self;
        let mut command = None;

        for part in &parts {
            if let Some(scope) = current_scope.get_scope(part) {
                current_scope = scope;
            } else if let Some(cmd) = current_scope.get_command(part) {
                command = Some(cmd);
                break;
            } else {
                // Invalid part, stop completion
                break;
            }
        }

        if let Some(command) = command {
            let option_defs = command.options();

            // Track options already seen
            let options_seen: Vec<String> = parts
                .iter()
                .filter(|part| part.starts_with('-'))
                .filter_map(|s| {
                    option_defs
                        .iter()
                        .find(|opt| {
                            opt.long.as_deref() == Some(s) || opt.short.as_deref() == Some(s)
                        })
                        .map(|opt| opt.name.clone())
                })
                .collect();
            trace!("Options seen: {:?}", options_seen);

            let last_token = word;

            // Determine if the last token is an option or a value
            if let Some(current_token) = parts.iter().rev().nth(0) {
                trace!("Current token: {}", current_token);
                if current_token.starts_with('-') {
                    trace!("Current token is an option");
                    // Find the option definition
                    let opt_def = option_defs.iter().find(|opt| {
                        opt.long.as_deref() == Some(current_token)
                            || opt.short.as_deref() == Some(current_token)
                    });

                    if let Some(opt_def) = opt_def {
                        trace!("Current option is known with definition: {:?}", opt_def);
                        if !opt_def.flag {
                            trace!("Option is not a flag");
                            // Option expects a value
                            if let Some(autocomplete_fn) = opt_def.autocomplete {
                                trace!("Option has autocomplete function");
                                let suggestions = autocomplete_fn(&self, last_token);
                                completions.extend(suggestions.into_iter().map(|s| Pair {
                                    display: s.clone(),
                                    replacement: s,
                                }));
                            } else {
                                trace!("Option lacks autocomplete function");
                            }
                        } else {
                            // Option is a flag, does not expect a value
                            // Suggest options
                            suggest_options(
                                &option_defs,
                                &options_seen,
                                last_token,
                                &mut completions,
                            );
                        }
                    } else {
                        // Previous token is not a recognized option
                        // Suggest options
                        suggest_options(&option_defs, &options_seen, last_token, &mut completions);
                    }
                } else {
                    trace!("Testing previous token");

                    if let Some(prev_token) = parts.iter().rev().nth(1) {
                        if prev_token.starts_with('-') {
                            trace!("Previous token is an option, expanding completions for option");

                            let opt_def = option_defs.iter().find(|opt| {
                                opt.long.as_deref() == Some(prev_token)
                                    || opt.short.as_deref() == Some(prev_token)
                            });

                            if let Some(opt_def) = opt_def {
                                trace!("Previous option is known with definition: {:?}", opt_def);
                                if !opt_def.flag {
                                    trace!("Option is not a flag");
                                    // Option expects a value
                                    if let Some(autocomplete_fn) = opt_def.autocomplete {
                                        trace!("Option has autocomplete function");
                                        let suggestions = autocomplete_fn(&self, last_token);

                                        if suggestions.contains(&current_token.to_string()) {
                                            // The previous token matches one of the suggestions, so move on to the rest of the options.
                                            suggest_options(
                                                &option_defs,
                                                &options_seen,
                                                last_token,
                                                &mut completions,
                                            );
                                        } else {
                                            completions.extend(suggestions.into_iter().map(|s| {
                                                Pair {
                                                    display: s.clone(),
                                                    replacement: s,
                                                }
                                            }));
                                        }
                                    } else {
                                        trace!("Option lacks autocomplete function");
                                    }
                                } else {
                                    // Option is a flag, does not expect a value
                                    // Suggest options
                                    suggest_options(
                                        &option_defs,
                                        &options_seen,
                                        last_token,
                                        &mut completions,
                                    );
                                }
                            } else {
                                // Previous token is not a recognized option
                                // Do nothing
                            }
                        } else {
                            suggest_options(
                                &option_defs,
                                &options_seen,
                                last_token,
                                &mut completions,
                            );
                        }
                    } else {
                        // Previous token is not an option, suggest options
                        suggest_options(&option_defs, &options_seen, last_token, &mut completions);
                    }
                }
            }
        } else {
            completions.extend(current_scope.get_completions(word));
        }

        trace!("Completions: {:?}", display_pairs(&completions));

        Ok((start, completions))
    }
}

fn suggest_options(
    option_defs: &[CliOption],
    options_seen: &[String],
    last_token: &str,
    completions: &mut Vec<Pair>,
) {
    completions.extend(
        option_defs
            .iter()
            .filter(|opt| {
                !options_seen.contains(&opt.name)
                    && (opt
                        .long
                        .as_deref()
                        .map_or(false, |long| long.starts_with(last_token))
                        || opt
                            .short
                            .as_deref()
                            .map_or(false, |short| short.starts_with(last_token)))
            })
            .map(|opt| {
                let extra_info = if opt.flag {
                    opt.help.clone()
                } else {
                    format!("<{}> {}", opt.field_type_help, opt.help)
                };
                if let Some(long) = &opt.long {
                    Pair {
                        display: format!("{} {}", long, extra_info),
                        replacement: format!("{}", long),
                    }
                } else if let Some(short) = &opt.short {
                    Pair {
                        display: format!("{} {}", short, extra_info),
                        replacement: format!("{}", short),
                    }
                } else {
                    Pair {
                        display: opt.name.clone(),
                        replacement: opt.name.clone(),
                    }
                }
            }),
    );
}

fn display_pairs<I>(pairs: &I) -> String
where
    I: IntoIterator<Item = Pair> + Clone,
{
    pairs
        .clone()
        .into_iter()
        .map(|p| format!("{} <{}>", p.display.clone(), p.replacement.clone()))
        .collect::<Vec<String>>()
        .join("\n")
}

pub fn classes(cmdlist: &CommandList, prefix: &str) -> Vec<String> {
    let mut cmd = cmdlist.client.classes().find();

    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "name",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }
    match cmd.execute() {
        Ok(classes) => classes.into_iter().map(|c| c.name).collect(),
        Err(_) => {
            warn!("Failed to fetch classes for autocomplete");
            Vec::new()
        }
    }
}

pub fn namespaces(cmdlist: &CommandList, prefix: &str) -> Vec<String> {
    let mut cmd = cmdlist.client.namespaces().find();

    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "name",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }
    match cmd.execute() {
        Ok(classes) => classes.into_iter().map(|c| c.name).collect(),
        Err(_) => {
            warn!("Failed to fetch classes for autocomplete");
            Vec::new()
        }
    }
}
