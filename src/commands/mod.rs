use rustyline::completion::Pair;
use std::any::TypeId;

mod class;
mod namespace;

pub use class::ClassNew;
pub use namespace::NamespaceNew;

use crate::{errors::AppError, tokenizer::CommandTokenizer};
use log::trace;

#[allow(dead_code)]
#[derive(Debug)]
pub struct CliOption {
    pub name: String,
    pub short: Option<String>,
    pub long: Option<String>,
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
}

pub trait CliCommand: CliCommandInfo {
    fn execute(&self, tokens: &CommandTokenizer) -> Result<(), AppError>;

    fn validate(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        self.validate_not_both_short_and_long_set(tokens)?;
        self.validate_missing_options(tokens)?;
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

    fn get_option_completions(&self, prefix: &str, options_seen: &[String]) -> Vec<Pair> {
        let mut completions = Vec::new();

        for opt in self.options() {
            let mut display = String::new();
            if prefix.is_empty() {
                if let Some(short) = &opt.short {
                    if options_seen.contains(short) {
                        continue;
                    }
                    display = short.clone();
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
}
