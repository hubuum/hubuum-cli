use cli_command_derive::CliCommand;
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Create a new class",
    long_about = "Create a new class with the specified properties.",
    examples = r#"-n MyClass -i 1 -d "My class description"
--name MyClass --namespace-id 1 --description 'My class' --schema '{\"type\": \"object\"}'"#
)]
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

impl CliCommand for ClassNew {
    fn execute(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
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
