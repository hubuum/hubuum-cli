use hubuum_client::{Authenticated, SyncClient};
use log::trace;
use rustyline::completion::Pair;
use std::any::TypeId;

mod builder;
mod class;
mod group;
mod help;
mod namespace;
mod user;

use crate::output::append_line;

pub use builder::build_repl_commands;
pub use class::*;
pub use group::*;
#[allow(unused_imports)]
pub use help::Help;
pub use namespace::*;
pub use user::*;

use crate::{errors::AppError, tokenizer::CommandTokenizer};

#[allow(dead_code)]
#[derive(Debug)]
pub struct CliOption {
    pub name: String,
    pub short: Option<String>,
    pub long: Option<String>,
    pub flag: bool,
    pub help: String,
    pub field_type: TypeId,
    pub field_type_help: String,
    pub required: bool,
}

impl CliOption {
    pub fn short_without_dash(&self) -> Option<String> {
        self.short.as_ref().map(|s| s[1..].to_string())
    }

    pub fn long_without_dashes(&self) -> Option<String> {
        self.long.as_ref().map(|l| l[2..].to_string())
    }
}

pub trait CliCommandInfo {
    fn options(&self) -> Vec<CliOption>;
    fn name(&self) -> String;
    fn about(&self) -> Option<String>;
    fn long_about(&self) -> Option<String>;
    fn examples(&self) -> Option<String>;
}

pub trait CliCommand: CliCommandInfo {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError>;

    fn validate(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        self.validate_not_both_short_and_long_set(tokens)?;
        self.validate_missing_options(tokens)?;
        self.validate_flag_options(tokens)?;
        Ok(())
    }

    fn validate_missing_options(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let tokenpairs = tokens.get_options();
        let mut missing_options = Vec::new();

        // Check if either opt.short or opt.long is a key in tokenpairs
        for opt in self.options() {
            if !opt.required {
                continue;
            }

            if let Some(short) = &opt.short_without_dash() {
                if tokenpairs.contains_key(short) {
                    continue;
                }
                trace!("Short not found: {}", short);
            }
            if let Some(long) = &opt.long_without_dashes() {
                if tokenpairs.contains_key(long) {
                    continue;
                }
                trace!("Long not found: {}", long);
            }

            missing_options.push(opt.name.clone());
        }

        if !missing_options.is_empty() {
            return Err(AppError::MissingOptions(missing_options))?;
        }

        Ok(())
    }

    fn validate_not_both_short_and_long_set(
        &self,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let tokenpairs = tokens.get_options();
        let mut duplicate_options = Vec::new();

        for opt in self.options() {
            if let Some(short) = &opt.short {
                if let Some(long) = &opt.long {
                    if tokenpairs.contains_key(short) && tokenpairs.contains_key(long) {
                        duplicate_options.push(opt.name.clone());
                    }
                }
            }
        }

        if !duplicate_options.is_empty() {
            return Err(AppError::DuplicateOptions(duplicate_options));
        }

        Ok(())
    }

    /// Flag options are not allowed to have values, but are boolean flags. In the tokenizer
    /// they are represented as a key with an empty ("") value. We alert if we find any flag
    /// options with a value.
    fn validate_flag_options(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let tokenpairs = tokens.get_options();
        let mut populated_flag_options = Vec::new();

        for opt in self.options() {
            if opt.flag {
                if let Some(short) = &opt.short_without_dash() {
                    if tokenpairs.contains_key(short) && !tokenpairs.get(short).unwrap().is_empty()
                    {
                        populated_flag_options.push(short.clone());
                    }
                }
                if let Some(long) = &opt.long_without_dashes() {
                    if tokenpairs.contains_key(long) && !tokenpairs.get(long).unwrap().is_empty() {
                        populated_flag_options.push(long.clone());
                    }
                }
            }
        }

        if !populated_flag_options.is_empty() {
            return Err(AppError::PopulatedFlagOptions(populated_flag_options));
        }

        Ok(())
    }

    fn get_option_completions(&self, prefix: &str, options_seen: &[String]) -> Vec<Pair> {
        let mut completions = Vec::new();

        for opt in self.options() {
            let mut display = String::new();
            if prefix.is_empty() {
                if let Some(short) = &opt.short {
                    if options_seen.contains(short) {
                        continue;
                    }
                    display.clone_from(short)
                }
            }
            if let Some(long) = &opt.long {
                if options_seen.contains(long) {
                    continue;
                }
                if prefix.is_empty() || long.starts_with(prefix) {
                    if !display.is_empty() {
                        display.push_str(", ");
                    }
                    display.push_str(long);
                }
            }

            if !display.is_empty() {
                completions.push(Pair {
                    display: format!("{} <{}> {}", display, opt.field_type_help, opt.help),
                    replacement: opt.long.unwrap_or_default(),
                });
            }
        }

        completions
    }

    fn help(&self, command_name: &String, context: &[String]) -> Result<(), AppError> {
        let mut help = String::new();
        let fq_name = format!("{} {}", context.join(" "), command_name);
        if let Some(about) = self.about() {
            help.push_str(&format!("{} - {} \n\n", fq_name, about));
        } else {
            help.push_str(&fq_name);
        }
        if let Some(long_about) = self.long_about() {
            help.push_str(&format!("{}\n\n", long_about));
        }
        let options = self.options();
        if !options.is_empty() {
            help.push_str("Options:\n");

            // Find the maximum width for each column
            let max_short_width = options
                .iter()
                .map(|opt| opt.short.as_ref().map_or(0, |s| s.len()))
                .max()
                .unwrap_or(0);
            let max_long_width = options
                .iter()
                .map(|opt| opt.long.as_ref().map_or(0, |l| l.len()))
                .max()
                .unwrap_or(0);
            let max_type_width = options
                .iter()
                .map(|opt| opt.field_type_help.len())
                .max()
                .unwrap_or(0);

            for opt in self.options() {
                let short = opt
                    .short
                    .as_ref()
                    .map_or(String::new(), |s| format!("{},", s));
                let long = opt
                    .long
                    .as_ref()
                    .map_or(String::new(), |l| format!("{},", l));
                let flag = if opt.flag { " (flag)" } else { "" };

                help.push_str(&format!(
                    "  {:<width_short$} {:<width_long$} {:<width_type$} {}{}\n",
                    short,
                    long,
                    format!("<{}>", opt.field_type_help),
                    opt.help,
                    flag,
                    width_short = max_short_width + 3, // +3 for "-x,"
                    width_long = max_long_width + 4,   // +4 for "--xx,"
                    width_type = max_type_width + 2    // +2 for "<>"
                ));
            }
            help.push('\n');
        }
        if let Some(examples) = self.examples() {
            help.push_str("Examples:\n");

            for line in examples.lines() {
                help.push_str(&format!("  {} {}\n", fq_name, line));
            }
        }
        for line in help.lines() {
            append_line(line)?;
        }
        Ok(())
    }
}
