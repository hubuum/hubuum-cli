use cli_command_derive::CliCommand;
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct ClassNew {
    #[option(short = "n", long = "name", help = "Name of the class")]
    pub name: String,
    #[option(short = "i", long = "namespace-id", help = "Namespace ID")]
    pub namespace_id: u32,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: String,
    #[option(short = "s", long = "schema", help = "JSON schema for the class")]
    pub json_schema: Option<serde_json::Value>,
    #[option(
        short = "v",
        long = "validate",
        help = "Validate against schema, requires schema to be set"
    )]
    pub validate_schema: Option<bool>,
}

/*
impl ClassNew {
    pub fn new_from_tokens(tokens: &CommandTokenizer) -> Result<Self, AppError> {
        let mut class = ClassNew::default();

        println!("Options: {:?}", class.options());

        class.validate(tokens)?;

        for (key, value) in tokens.get_options() {
            // key will be the option name without the dashes. We are post-validation here
            // so we know there is only either a short or long option.
            for option in class.options() {
                let short_opt = option.short_without_dash(); // name
                let long_opt = option.long_without_dashes(); // n
                let name = option.name.clone(); // field name in the struct.
            }

            match key.as_str() {
                "name" => class.name = value.clone(),
                "namespace-id" => class.namespace_id = value.parse()?,
                "description" => class.description = value.clone(),
                "schema" => class.json_schema = Some(serde_json::from_str(&value)?),
                "validate" => class.validate_schema = Some(value.parse()?),
                _ => Err(AppError::InvalidOption(key.clone()))?,
            }
        }
        Ok(class)
    }
}
    */

impl CliCommand for ClassNew {
    fn execute(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        println!("Creating new class: {:?}", self);
        println!("Tokens: {:?}", tokens);
        let new = &self.new_from_tokens(tokens)?;
        println!("New class: {:?}", new);
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct ClassInfo {}
impl CliCommand for ClassInfo {
    fn execute(&self, _tokens: &CommandTokenizer) -> Result<(), AppError> {
        println!("Info about class: {:?}", self);
        Ok(())
    }
}
