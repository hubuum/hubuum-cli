use cli_command_derive::CliCommand;
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand)]
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

impl Default for ClassNew {
    fn default() -> Self {
        ClassNew {
            name: String::new(),
            namespace_id: 0,
            description: String::new(),
            json_schema: Some(serde_json::json!({})),
            validate_schema: Some(false),
        }
    }
}

impl CliCommand for ClassNew {
    fn execute(&self) -> Result<(), AppError> {
        println!("Creating new class: {:?}", self);
        Ok(())
    }

    fn populate(&mut self) -> Result<(), AppError> {
        println!("Populating class: {:?}", self);
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand)]
pub struct ClassInfo {}
impl CliCommand for ClassInfo {
    fn execute(&self) -> Result<(), AppError> {
        println!("Info about class: {:?}", self);
        Ok(())
    }

    fn populate(&mut self) -> Result<(), AppError> {
        println!("Populating class: {:?}", self);
        Ok(())
    }
}
