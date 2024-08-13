use cli_command_derive::CliCommand;
use hubuum_client::{Authenticated, SyncClient};

use crate::errors::AppError;
use crate::tokenizer::CommandTokenizer;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

#[allow(dead_code)]
#[derive(Debug, Default, CliCommand)]
pub struct Help {
    #[option(short = "t", long = "tree", help = "Command tree", flag = "true")]
    pub tree: Option<bool>,
}

impl CliCommand for Help {
    fn execute(
        &self,
        _client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let options = tokens.get_options();
        if options.get("tree").is_some() {
            println!("{}\n", crate::commands::build_repl_commands().show_tree());
            return Ok(());
        }

        Ok(())
    }
}
