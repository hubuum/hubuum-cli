use cli_command_derive::CliCommand;
use hubuum_client::{
    Authenticated, FilterOperator, Group, GroupPost, IntoResourceFilter, QueryFilter, SyncClient,
};
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::formatting::{OutputFormatter, OutputFormatterWithPadding};
use crate::output::append_line;
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
    pub name: Option<String>,
    #[option(
        short = "gs",
        long = "groupname__startswith",
        help = "Name of the group starts with"
    )]
    pub name_startswith: Option<String>,
    #[option(
        short = "ge",
        long = "groupname__endswith",
        help = "Name of the group ends with"
    )]
    pub name_endswith: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: Option<String>,
    #[option(short = "j", long = "json", help = "Output as JSON", flag = "true")]
    pub rawjson: Option<bool>,
}

impl IntoResourceFilter<Group> for &GroupList {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];

        if let Some(name) = &self.name {
            filters.push(QueryFilter {
                key: "groupname".to_string(),
                value: name.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }
        if let Some(name_startswith) = &self.name_startswith {
            filters.push(QueryFilter {
                key: "groupname".to_string(),
                value: name_startswith.clone(),
                operator: FilterOperator::StartsWith { is_negated: false },
            });
        }

        if let Some(name_endswith) = &self.name_endswith {
            filters.push(QueryFilter {
                key: "groupname".to_string(),
                value: name_endswith.clone(),
                operator: FilterOperator::EndsWith { is_negated: false },
            });
        }
        if let Some(description) = &self.description {
            filters.push(QueryFilter {
                key: "description".to_string(),
                value: description.clone(),
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

        if new.rawjson.is_some() {
            append_line(serde_json::to_string_pretty(&groups)?)?;
            return Ok(());
        }

        groups.format()?;

        Ok(())
    }
}
