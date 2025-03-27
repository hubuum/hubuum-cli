use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use hubuum_client::{Authenticated, SyncClient};
use rustyline::highlight::Highlighter;
use rustyline::{hint::Hinter, validate::Validator, Helper};

// use colored::Colorize;
use log::{debug, trace};
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

    pub fn client(&self) -> Arc<SyncClient<Authenticated>> {
        self.client.clone()
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

        // Add commands, sort by name
        let mut command_names: Vec<_> = self.commands.keys().collect();
        command_names.sort();
        for command_name in command_names {
            let line = format!("{}{}{}{}\n", prefix, indent, branch, command_name);
            result.push_str(&line);
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
impl Validator for &CommandList {}
impl Helper for CommandList {}
impl Helper for &CommandList {}
impl Highlighter for CommandList {}
impl Highlighter for &CommandList {}
impl Hinter for CommandList {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}
impl Hinter for &CommandList {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Completer for &CommandList {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        CommandList::complete(self, line, pos, ctx)
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
            let options = command.options();
            let options_seen = options_seen(&parts, &options);
            trace!("Options seen: {:?}", options_seen);

            let last_token = word;

            // Determine if the last token is an option or a value
            if let Some(current_token) = parts.iter().rev().nth(0) {
                trace!("Current token: {}", current_token);
                if current_token.starts_with('-') {
                    trace!("Current token is an option");
                    let opt_def = option_definiton(&options, current_token);

                    if let Some(opt_def) = opt_def {
                        trace!("Current option is known with definition: {:?}", opt_def);
                        if opt_def.flag {
                            suggest_options(&options, &options_seen, last_token, &mut completions);
                        } else {
                            suggest_from_autocomplete(
                                &self,
                                &opt_def,
                                &last_token,
                                &parts,
                                &mut completions,
                            )
                        }
                    } else {
                        // Previous token is not a recognized option
                        suggest_options(&options, &options_seen, last_token, &mut completions);
                    }
                } else {
                    trace!("Testing previous token");

                    if let Some(prev_token) = parts.iter().rev().nth(1) {
                        if prev_token.starts_with('-') {
                            trace!("Previous token is an option, expanding completions for option");
                            let opt_def = option_definiton(&options, &prev_token);

                            if let Some(opt_def) = opt_def {
                                trace!("Previous option is known with definition: {:?}", opt_def);
                                if opt_def.flag {
                                    // Option is a flag, does not expect a value
                                    suggest_options(
                                        &options,
                                        &options_seen,
                                        last_token,
                                        &mut completions,
                                    );
                                } else {
                                    trace!("Option is not a flag");
                                    if let Some(autocomplete_fn) = opt_def.autocomplete {
                                        trace!("Option has autocomplete function");
                                        let suggestions =
                                            autocomplete_fn(&self, last_token, &parts);

                                        if suggestions.contains(&current_token.to_string()) {
                                            // The previous token matches one of the suggestions, so move on to the rest of the options.
                                            suggest_options(
                                                &options,
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
                                }
                            } else {
                                // Previous token is not a recognized option
                                // Do nothing
                            }
                        } else {
                            suggest_options(&options, &options_seen, last_token, &mut completions);
                        }
                    } else {
                        // Previous token is not an option, suggest options
                        suggest_options(&options, &options_seen, last_token, &mut completions);
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

/// Find the options that have been seen in the input
fn options_seen(parts: &[String], options: &[CliOption]) -> Vec<String> {
    parts
        .iter()
        .filter(|part| part.starts_with('-'))
        .filter_map(|s| {
            options
                .iter()
                .find(|opt| opt.long.as_deref() == Some(s) || opt.short.as_deref() == Some(s))
                .map(|opt| opt.name.clone())
        })
        .collect()
}

/// Find the definition of an option by its long or short name
fn option_definiton<'a>(option_defs: &'a [CliOption], token: &str) -> Option<&'a CliOption> {
    option_defs
        .iter()
        .find(|opt| opt.long.as_deref() == Some(token) || opt.short.as_deref() == Some(token))
}

/// Suggest completions for an option based on its autocomplete function
fn suggest_from_autocomplete(
    cmdlist: &CommandList,
    opt_def: &CliOption,
    last_token: &str,
    tokens: &[String],
    completions: &mut Vec<Pair>,
) {
    use colored::Colorize;
    if let Some(autocomplete_fn) = opt_def.autocomplete {
        trace!("Option has autocomplete function");
        let suggestions = autocomplete_fn(&cmdlist, last_token, &tokens);
        completions.extend(suggestions.into_iter().map(|s| Pair {
            display: s.clone().bold().italic().green().to_string(),
            replacement: s,
        }));
    } else {
        trace!("Option lacks autocomplete function");
    }
}

/// Suggest completions for options based on their long and short names, removing those already seen
fn suggest_options(
    option_defs: &[CliOption],
    options_seen: &[String],
    last_token: &str,
    completions: &mut Vec<Pair>,
) {
    let options_left = option_defs.iter().filter(|opt| {
        !options_seen.contains(&opt.name)
            && (opt
                .long
                .as_deref()
                .map_or(false, |long| long.starts_with(last_token))
                || opt
                    .short
                    .as_deref()
                    .map_or(false, |short| short.starts_with(last_token)))
    });

    let max_short_width = options_left
        .clone()
        .map(|opt| opt.short.as_ref().map_or(0, |s| s.len()))
        .max()
        .unwrap_or(0);
    let max_long_width = options_left
        .clone()
        .map(|opt| opt.long.as_ref().map_or(0, |l| l.len()))
        .max()
        .unwrap_or(0);
    let max_type_width = options_left
        .clone()
        .map(|opt| opt.field_type_help.len())
        .max()
        .unwrap_or(0);

    for opt in options_left {
        let short = opt
            .short
            .as_ref()
            .map_or("".to_string(), |s| format!("{},", s));
        let long = opt
            .long
            .as_ref()
            .map_or("".to_string(), |l| format!("{}", l));
        let short_padding = " ".repeat(max_short_width + 2 - short.len());
        let long_padding = " ".repeat(max_long_width + 2 - long.len());
        let type_padding = " ".repeat(max_type_width + 2 - opt.field_type_help.len());

        let diplay = format!(
            "{}{}{}{}{}{}{}{}",
            short,
            short_padding,
            long,
            long_padding,
            type_padding,
            format!("<{}>", opt.field_type_help),
            " ".repeat(2),
            opt.help
        );

        completions.push(Pair {
            display: diplay,
            replacement: long.clone(),
        });
    }
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
