use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{build_list_query, contains_clause, desired_format, render_list_page, CliCommand};
use crate::autocomplete::{group_sort, group_where};
use crate::catalog::CommandCatalogBuilder;

use crate::domain::GroupDetails;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, CreateGroupInput, GroupUpdateInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["group"],
            catalog_command(
                "create",
                GroupNew::default(),
                CommandDocs {
                    about: Some("Create a group"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["group"],
            catalog_command(
                "list",
                GroupList::default(),
                CommandDocs {
                    about: Some("List groups"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["group"],
            catalog_command(
                "add_user",
                GroupAddUser::default(),
                CommandDocs {
                    about: Some("Add a user to a group"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["group"],
            catalog_command(
                "remove_user",
                GroupRemoveUser::default(),
                CommandDocs {
                    about: Some("Remove a user from a group"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["group"],
            catalog_command(
                "show",
                GroupInfo::default(),
                CommandDocs {
                    about: Some("Show group details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["group"],
            catalog_command(
                "modify",
                GroupModify::default(),
                CommandDocs {
                    about: Some("Modify a group"),
                    long_about: Some("Update an existing group by group name."),
                    examples: Some(
                        r#"modify my-group --rename other-group
modify --groupname my-group --description "Updated description""#,
                    ),
                },
            ),
        );
}

trait GetGroupname {
    fn groupname(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct GroupNew {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: String,
}

impl CliCommand for GroupNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let group = services.gateway().create_group(CreateGroupInput {
            groupname: new.groupname,
            description: new.description,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => group.format_json_noreturn()?,
            OutputFormat::Text => group.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct GroupAddUser {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
    #[option(short = "u", long = "username", help = "Username to add to the group")]
    pub username: String,
}
impl CliCommand for GroupAddUser {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        services
            .gateway()
            .add_user_to_group(&new.groupname, &new.username)?;

        let message = format!("User '{}' added to group '{}'", new.username, new.groupname);

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
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
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        services
            .gateway()
            .remove_user_from_group(&new.groupname, &new.username)?;

        let message = format!(
            "User '{}' removed from group '{}'",
            new.username, new.groupname
        );

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct GroupInfo {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: String,
}
impl CliCommand for GroupInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let details: GroupDetails = services.gateway().group_details(&new.groupname)?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&details)?)?,
            OutputFormat::Text => {
                details.group.format()?;
                details.members.format_noreturn()?
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct GroupModify {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub groupname: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the group")]
    pub rename: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: Option<String>,
}

impl GetGroupname for &GroupModify {
    fn groupname(&self) -> Option<String> {
        self.groupname.clone()
    }
}

impl CliCommand for GroupModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.groupname = groupname_or_pos(&query, tokens, 0)?;
        let groupname = query
            .groupname
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["groupname".to_string()]))?;

        let group = services.gateway().update_group(GroupUpdateInput {
            groupname,
            rename: query.rename,
            description: query.description,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => group.format_json_noreturn()?,
            OutputFormat::Text => group.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct GroupList {
    #[option(short = "g", long = "groupname", help = "Name of the group")]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the group")]
    pub description: Option<String>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "group_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "group_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for GroupList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [
                query.name.map(|value| contains_clause("groupname", value)),
                query
                    .description
                    .map(|value| contains_clause("description", value)),
            ]
            .into_iter()
            .flatten(),
        )?;
        let groups = services.gateway().list_groups(&list_query)?;
        render_list_page(tokens, &groups)
    }
}

fn groupname_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<String>, AppError>
where
    U: GetGroupname,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.groupname().is_none() {
        if pos0.is_none() {
            return Err(AppError::MissingOptions(vec!["groupname".to_string()]));
        }
        return Ok(pos0.cloned());
    }
    Ok(query.groupname().clone())
}

#[cfg(test)]
mod tests {
    use crate::commands::command_options;
    use crate::errors::AppError;
    use crate::tokenizer::CommandTokenizer;

    use super::GroupList;

    #[test]
    fn simple_group_alias_still_parses() {
        let tokens = CommandTokenizer::new(
            "group list --groupname admins",
            "list",
            &command_options::<GroupList>(),
        )
        .expect("tokenization should succeed");
        let parsed = GroupList::parse_tokens(&tokens).expect("group list should parse");

        assert_eq!(parsed.name.as_deref(), Some("admins"));
    }

    #[test]
    fn double_underscore_flags_are_rejected() {
        let tokens = CommandTokenizer::new(
            "group list --groupname__startswith adm",
            "list",
            &command_options::<GroupList>(),
        )
        .expect("tokenization should succeed");
        let err = GroupList::parse_tokens(&tokens).expect_err("removed flag should be rejected");

        assert!(matches!(err, AppError::InvalidOption(_)));
    }
}
