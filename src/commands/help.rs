use cli_command_derive::CliCommand;

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
    fn execute(&self, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let options = tokens.get_options();
        if options.get("tree").is_some() {
            println!("{}\n", crate::build_cli().show_tree());
            return Ok(());
        }

        Ok(())
    }
}
