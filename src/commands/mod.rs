use log::trace;
use std::any::TypeId;
use std::collections::HashSet;

mod builder;
mod class;
mod group;
mod help;
mod imports;
mod namespace;
mod object;
mod relations;
mod report;
mod task;
mod user;

pub use builder::build_command_catalog;

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

pub trait CommandArgs: Sized + Default + Send + Sync + 'static {
    fn options() -> Vec<CliOption>;

    fn parse_tokens(tokens: &CommandTokenizer) -> Result<Self, AppError>;
}

pub trait CliCommand: CommandArgs + Send + Sync {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError>;
}

pub fn standard_options() -> Vec<CliOption> {
    vec![
        CliOption {
            name: "help".to_string(),
            short: Some("-h".to_string()),
            long: Some("--help".to_string()),
            flag: true,
            help: "Prints help information".to_string(),
            field_type: TypeId::of::<bool>(),
            field_type_help: "bool".to_string(),
            required: false,
            autocomplete: None,
        },
        CliOption {
            name: "json".to_string(),
            short: Some("-j".to_string()),
            long: Some("--json".to_string()),
            flag: true,
            help: "Output as JSON".to_string(),
            field_type: TypeId::of::<bool>(),
            field_type_help: "bool".to_string(),
            required: false,
            autocomplete: None,
        },
    ]
}

pub fn command_options<C: CommandArgs>() -> Vec<CliOption> {
    let mut options = C::options();
    options.extend(standard_options());
    options
}

pub fn validate_command_args<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    validate_unknown_options::<C>(tokens)?;
    validate_not_both_short_and_long_set::<C>(tokens)?;
    validate_missing_options::<C>(tokens)?;
    validate_flag_options::<C>(tokens)?;
    Ok(())
}

pub fn validate_unknown_options<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let mut known_options = HashSet::new();
    for opt in command_options::<C>() {
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

pub fn validate_missing_options<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let tokenpairs = tokens.get_options();
    let mut missing_options = Vec::new();

    for opt in command_options::<C>() {
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
        return Err(AppError::MissingOptions(missing_options));
    }

    Ok(())
}

pub fn validate_not_both_short_and_long_set<C: CommandArgs>(
    tokens: &CommandTokenizer,
) -> Result<(), AppError> {
    let tokenpairs = tokens.get_options();
    let mut duplicate_options = Vec::new();

    for opt in command_options::<C>() {
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
pub fn validate_flag_options<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let tokenpairs = tokens.get_options();
    let mut populated_flag_options = Vec::new();

    for opt in command_options::<C>() {
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

pub fn desired_format(tokens: &CommandTokenizer) -> crate::models::OutputFormat {
    if want_json(tokens) {
        crate::models::OutputFormat::Json
    } else {
        crate::models::OutputFormat::Text
    }
}

pub fn want_json(tokens: &CommandTokenizer) -> bool {
    let opts = tokens.get_options();
    opts.contains_key("j") || opts.contains_key("json")
}

#[allow(dead_code)]
pub fn want_help(tokens: &CommandTokenizer) -> bool {
    let opts = tokens.get_options();
    opts.contains_key("h") || opts.contains_key("help")
}
