use log::trace;
use std::any::TypeId;
use std::collections::HashSet;

mod builder;
mod class;
mod group;
mod help;
mod namespace;
mod object;
mod relations;
mod user;

pub use builder::build_command_catalog;
pub use class::*;
pub use group::*;
#[allow(unused_imports)]
pub use help::Help;
pub use namespace::*;
pub use object::*;
pub use relations::*;
pub use user::*;

use crate::{errors::AppError, services::AppServices, tokenizer::CommandTokenizer};

pub type AutoCompleter = fn(&crate::services::CompletionContext, &str, &[String]) -> Vec<String>;

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
    pub autocomplete: Option<AutoCompleter>,
}

impl CliOption {
    pub fn short_without_dash(&self) -> Option<String> {
        self.short.as_ref().map(|s| s[1..].to_string())
    }

    pub fn long_without_dashes(&self) -> Option<String> {
        self.long.as_ref().map(|l| l[2..].to_string())
    }
}

#[allow(dead_code)]
pub trait CliCommandInfo {
    fn options(&self) -> Vec<CliOption>;
    fn name(&self) -> String;
    fn about(&self) -> Option<String>;
    fn long_about(&self) -> Option<String>;
    fn examples(&self) -> Option<String>;
}

pub trait CliCommand: CliCommandInfo + Send + Sync {
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError>;

    fn validate(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        self.validate_unknown_options(tokens)?;
        self.validate_not_both_short_and_long_set(tokens)?;
        self.validate_missing_options(tokens)?;
        self.validate_flag_options(tokens)?;
        Ok(())
    }

    fn validate_unknown_options(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut known_options = HashSet::new();
        for opt in self.options() {
            if let Some(short) = opt.short_without_dash() {
                known_options.insert(short);
            }
            if let Some(long) = opt.long_without_dashes() {
                known_options.insert(long);
            }
        }

        let mut unknown_options: Vec<String> = tokens
            .get_options()
            .keys()
            .filter(|key| !known_options.contains(*key))
            .cloned()
            .collect();

        if unknown_options.is_empty() {
            return Ok(());
        }

        unknown_options.sort();
        Err(AppError::InvalidOption(unknown_options.join(", ")))
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
                trace!("Short not found: {short}");
            }
            if let Some(long) = &opt.long_without_dashes() {
                if tokenpairs.contains_key(long) {
                    continue;
                }
                trace!("Long not found: {long}");
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
            if let Some(short) = opt.short_without_dash() {
                if let Some(long) = opt.long_without_dashes() {
                    if tokenpairs.contains_key(&short) && tokenpairs.contains_key(&long) {
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
                    if let Some(value) = tokenpairs.get(short) {
                        if !value.is_empty() {
                            populated_flag_options.push(short.clone());
                        }
                    }
                }
                if let Some(long) = &opt.long_without_dashes() {
                    if let Some(value) = tokenpairs.get(long) {
                        if !value.is_empty() {
                            populated_flag_options.push(long.clone());
                        }
                    }
                }
            }
        }

        if !populated_flag_options.is_empty() {
            return Err(AppError::PopulatedFlagOptions(populated_flag_options));
        }

        Ok(())
    }
}
