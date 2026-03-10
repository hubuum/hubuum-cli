use cli_command_derive::CliCommand;

use crate::errors::AppError;
use crate::output::append_line;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

#[allow(dead_code)]
#[derive(Debug, Default, Clone, CliCommand)]
pub struct Help {
    #[option(short = "t", long = "tree", help = "Command tree", flag = "true")]
    pub tree: Option<bool>,
}

impl CliCommand for Help {
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let options = tokens.get_options();
        if options.get("tree").is_some() {
            let _ = services;
            append_line(crate::commands::build_command_catalog().render_tree())?;
            return Ok(());
        }

        Ok(())
    }
}
