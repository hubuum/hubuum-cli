use cli_command_derive::CliCommand;
use hubuum_client::{Authenticated, FilterOperator, Group, GroupPost, SyncClient};
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::formatting::{OutputFormatter, OutputFormatterWithPadding};
use crate::tokenizer::CommandTokenizer;
use crate::traits::SingleItemOrWarning;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupNew {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: String,
}

impl GroupNew {
    fn into_post(&self) -> GroupPost {
        GroupPost {
            groupname: self.groupname.clone(),
            description: self.description.clone(),
        }
    }
}

impl CliCommand for GroupNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;

        let group = client.groups().create(new.into_post())?;
        group.format(15)?;

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupList {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub name: String,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: String,
}

impl CliCommand for GroupList {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let groups = client.groups().find().execute()?;
        groups.format()?;

        Ok(())
    }
}
