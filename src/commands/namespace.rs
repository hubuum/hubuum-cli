use cli_command_derive::CliCommand;
use serde::{Deserialize, Serialize};

use crate::errors::AppError;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand)]
pub struct NamespaceNew {
    #[option(short = "n", long = "name", help = "Name of the namespace")]
    pub name: String,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the namespace"
    )]
    pub description: String,
}

impl Default for NamespaceNew {
    fn default() -> Self {
        NamespaceNew {
            name: String::new(),
            description: String::new(),
        }
    }
}

impl CliCommand for NamespaceNew {
    fn execute(&self) -> Result<(), AppError> {
        println!("Creating new namespace: {:?}", self);
        Ok(())
    }

    fn populate(&mut self) -> Result<(), AppError> {
        println!("Populating namespace: {:?}", self);
        for option in self.options() {
            println!("Option: {:?}", option);
        }
        Ok(())
    }
}
