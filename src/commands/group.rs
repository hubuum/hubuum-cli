use cli_command_derive::CliCommand;
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::domain::GroupDetails;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, CreateGroupInput, GroupFilter};
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct GroupNew {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: String,
}

impl CliCommand for GroupNew {
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let group = services.gateway().create_group(CreateGroupInput {
            groupname: new.groupname,
            description: new.description,
        })?;

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
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        services
            .gateway()
            .add_user_to_group(&new.groupname, &new.username)?;

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
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        services
            .gateway()
            .remove_user_from_group(&new.groupname, &new.username)?;

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
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let details: GroupDetails = services.gateway().group_details(&new.groupname)?;

        match self.desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&details)?)?,
            OutputFormat::Text => {
                details.group.format()?;
                details.members.format_noreturn()?
            }
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

impl CliCommand for GroupList {
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let groups = services.gateway().list_groups(GroupFilter {
            name: new.name,
            name_startswith: new.name_startswith,
            name_endswith: new.name_endswith,
            description: new.description,
        })?;

        match self.desired_format(tokens) {
            OutputFormat::Json => groups.format_json_noreturn()?,
            OutputFormat::Text => groups.format_noreturn()?,
        }

        Ok(())
    }
}
