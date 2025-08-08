use cli_command_derive::CliCommand;
use hubuum_client::{
    Authenticated, FilterOperator, Group, GroupPost, IntoResourceFilter, QueryFilter, SyncClient,
};
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
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

        match self.desired_format(tokens) {
            OutputFormat::Json => group.format_json_noreturn()?,
            OutputFormat::Text => group.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupAddUser {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(short = "u", long = "username", help = "Username to add to the group")]
    pub username: String,
}
impl CliCommand for GroupAddUser {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let group = client.groups().select_by_name(&new.groupname)?;
        let user = client.users().select_by_name(&new.username)?;

        group.add_user(user.id())?;

        let message = format!("User '{}' added to group '{}'", new.username, new.groupname);

        match self.desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupRemoveUser {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(
        short = "u",
        long = "username",
        help = "Username to remove from the group"
    )]
    pub username: String,
}
impl CliCommand for GroupRemoveUser {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let group = client.groups().select_by_name(&new.groupname)?;
        let user = client.users().select_by_name(&new.username)?;

        group.remove_user(user.id())?;

        let message = format!(
            "User '{}' removed from group '{}'",
            new.username, new.groupname
        );

        match self.desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupInfo {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
}
impl CliCommand for GroupInfo {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let group = client.groups().select_by_name(&new.groupname)?;

        match self.desired_format(tokens) {
            OutputFormat::Json => {
                let mut json_class = serde_json::to_value(group.resource())?;
                json_class["members"] = serde_json::to_value(group.members()?)?;

                append_line(serde_json::to_string_pretty(&json_class)?)?;
            }
            OutputFormat::Text => group.format()?.members()?.format_noreturn()?,
        }

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

        match self.desired_format(tokens) {
            OutputFormat::Json => groups.format_json_noreturn()?,
            OutputFormat::Text => groups.format_noreturn()?,
        }

        Ok(())
    }
}
