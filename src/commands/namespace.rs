use cli_command_derive::CliCommand;
use hubuum_client::{Authenticated, SyncClient};
use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::tokenizer::CommandTokenizer;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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

impl CliCommand for NamespaceNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
        Ok(())
    }
}
