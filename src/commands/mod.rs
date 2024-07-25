use rustyline::completion::Pair;
use std::any::TypeId;

mod class;
mod namespace;

pub use class::ClassNew;
pub use namespace::NamespaceNew;

use crate::errors::AppError;

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

pub trait CliCommandInfo {
    fn options(&self) -> Vec<CliOption>;
    fn name(&self) -> String;
}

pub trait CliCommand: CliCommandInfo {
    fn execute(&self) -> Result<(), AppError>;
    fn populate(&mut self) -> Result<(), AppError>;

    fn get_option_completions(&self, prefix: &str, options_seen: &Vec<String>) -> Vec<Pair> {
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
