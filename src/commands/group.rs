use cli_command_derive::CliCommand;
use hubuum_client::{
    Authenticated, FilterOperator, Group, GroupPost, IntoResourceFilter, QueryFilter, SyncClient,
};
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::formatting::{OutputFormatter, OutputFormatterWithPadding};
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupNew {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: String,
}

impl GroupNew {
    fn into_post(self) -> GroupPost {
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
    #[option(
        short = "gs",
        long = "groupname__startswith",
        help = "Name of the group starts with"
    )]
    pub name_startswith: String,
    #[option(
        short = "ge",
        long = "groupname__endswith",
        help = "Name of the group ends with"
    )]
    pub name_endswith: String,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: String,
}

impl IntoResourceFilter<Group> for &GroupList {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];

        if !self.name.is_empty() {
            filters.push(QueryFilter {
                key: "groupname".to_string(),
                value: self.name.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        if !self.name_startswith.is_empty() {
            filters.push(QueryFilter {
                key: "groupname__startswith".to_string(),
                value: self.name_startswith.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        if !self.name_endswith.is_empty() {
            filters.push(QueryFilter {
                key: "groupname__endswith".to_string(),
                value: self.name_endswith.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        if !self.description.is_empty() {
            filters.push(QueryFilter {
                key: "description".to_string(),
                value: self.description.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }

        filters
    }
}

impl CliCommand for GroupList {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let groups = client.groups().filter(&new)?;
        groups.format()?;

        Ok(())
    }
}
