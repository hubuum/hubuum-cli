use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::CommandCatalogBuilder;

use crate::autocomplete::{groups, namespaces};
use crate::domain::NamespacePermission;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_json, append_line};
use crate::services::{AppServices, CreateNamespaceInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["namespace"],
            catalog_command("create", NamespaceNew::default(), CommandDocs::default()),
        )
        .add_command(
            &["namespace"],
            catalog_command("list", NamespaceList::default(), CommandDocs::default()),
        )
        .add_command(
            &["namespace"],
            catalog_command("delete", NamespaceDelete::default(), CommandDocs::default()),
        )
        .add_command(
            &["namespace"],
            catalog_command("info", NamespaceInfo::default(), CommandDocs::default()),
        )
        .add_command(
            &["namespace", "permissions"],
            catalog_command(
                "list",
                NamespacePermissions::default(),
                CommandDocs::default(),
            ),
        )
        .add_command(
            &["namespace", "permissions"],
            catalog_command(
                "set",
                NamespacePermissionsSet::default(),
                CommandDocs::default(),
            ),
        );
}

trait GetNamespace {
    fn namespace(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct NamespaceNew {
    #[option(short = "n", long = "name", help = "Name of the namespace")]
    pub name: String,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the namespace"
    )]
    pub description: String,
    #[option(
        short = "o",
        long = "owner",
        help = "Name of the group owning namespace"
    )]
    pub owner: String,
}

impl CliCommand for NamespaceNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let namespace = services.gateway().create_namespace(CreateNamespaceInput {
            name: new.name,
            description: new.description,
            owner: new.owner,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => namespace.format_json_noreturn()?,
            OutputFormat::Text => namespace.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct NamespaceList {
    #[option(short = "n", long = "name", help = "Name of the namespace")]
    pub name: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the namespace"
    )]
    pub description: Option<String>,
}

impl CliCommand for NamespaceList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let namespaces = services
            .gateway()
            .list_namespaces(new.name, new.description)?;

        match desired_format(tokens) {
            OutputFormat::Json => namespaces.format_json_noreturn()?,
            OutputFormat::Text => namespaces.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct NamespaceInfo {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the namespace",
        autocomplete = "namespaces"
    )]
    pub name: Option<String>,
}

impl GetNamespace for &NamespaceInfo {
    fn namespace(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for NamespaceInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut new = Self::parse_tokens(tokens)?;

        new.name = namespace_or_pos(&new, tokens, 0)?;

        if new.name.is_none() {
            return Err(AppError::MissingOptions(vec!["namespace".to_string()]));
        }

        let namespace = services
            .gateway()
            .get_namespace(new.name.as_ref().unwrap())?;

        match desired_format(tokens) {
            OutputFormat::Json => namespace.format_json_noreturn()?,
            OutputFormat::Text => namespace.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct NamespaceDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the namespace",
        autocomplete = "namespaces"
    )]
    pub name: Option<String>,
}

impl GetNamespace for &NamespaceDelete {
    fn namespace(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for NamespaceDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut new = Self::parse_tokens(tokens)?;

        new.name = namespace_or_pos(&new, tokens, 0)?;

        if new.name.is_none() {
            return Err(AppError::MissingOptions(vec!["namespace".to_string()]));
        }

        let namespace_name = new.name.as_ref().unwrap().clone();
        services.gateway().delete_namespace(&namespace_name)?;

        let message = format!("Namespace '{}' deleted", namespace_name);

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct NamespacePermissions {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the namespace",
        autocomplete = "namespaces"
    )]
    pub name: Option<String>,
}

impl GetNamespace for &NamespacePermissions {
    fn namespace(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for NamespacePermissions {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut new = Self::parse_tokens(tokens)?;

        new.name = namespace_or_pos(&new, tokens, 0)?;

        let name = match &new.name {
            Some(name) => name,
            None => return Err(AppError::MissingOptions(vec!["namespace".to_string()])),
        };

        let permissions = services.gateway().list_namespace_permissions(name)?;

        let empty_message = format!("No permissions found for namespace '{name}'");

        match (desired_format(tokens), permissions.entries.is_empty()) {
            (OutputFormat::Json, true) => append_json_message(&empty_message)?,
            (OutputFormat::Json, false) => append_json(&permissions.entries)?,
            (OutputFormat::Text, true) => append_line(empty_message)?,
            (OutputFormat::Text, false) => permissions.summary.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct NamespacePermissionsSet {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the namespace",
        autocomplete = "namespaces"
    )]
    pub name: Option<String>,

    #[option(
        short = "g",
        long = "group",
        help = "Group to grant permissions to",
        autocomplete = "groups"
    )]
    pub group: String,

    #[option(
        long = "all",
        short = "a",
        help = "Grant all permissions to the group",
        flag = true
    )]
    pub all: Option<bool>,

    #[option(
        long = "ReadCollection",
        help = "Grant ReadCollection permissions on the namespace to the group",
        flag = true
    )]
    pub read_collection: Option<bool>,

    #[option(
        long = "UpdateCollection",
        help = "Grant UpdateCollection permissions on the namespace to the group",
        flag = true
    )]
    pub update_collection: Option<bool>,

    #[option(
        long = "DeleteCollection",
        help = "Grant DeleteCollection permissions on the namespace to the group",
        flag = true
    )]
    pub delete_collection: Option<bool>,

    #[option(
        long = "DelegateCollection",
        help = "Grant DelegateCollection permissions on the namespace to the group",
        flag = true
    )]
    pub delegate_collection: Option<bool>,

    #[option(
        long = "CreateClass",
        help = "Grant CreateClass permissions on the namespace to the group",
        flag = true
    )]
    pub create_class: Option<bool>,

    #[option(
        long = "ReadClass",
        help = "Grant ReadClass permissions on the namespace to the group",
        flag = true
    )]
    pub read_class: Option<bool>,

    #[option(
        long = "UpdateClass",
        help = "Grant UpdateClass permissions on the namespace to the group",
        flag = true
    )]
    pub update_class: Option<bool>,

    #[option(
        long = "DeleteClass",
        help = "Grant DeleteClass permissions on the namespace to the group",
        flag = true
    )]
    pub delete_class: Option<bool>,

    #[option(
        long = "CreateObject",
        help = "Grant CreateObject permissions on the namespace to the group",
        flag = true
    )]
    pub create_object: Option<bool>,

    #[option(
        long = "ReadObject",
        help = "Grant ReadObject permissions on the namespace to the group",
        flag = true
    )]
    pub read_object: Option<bool>,

    #[option(
        long = "UpdateObject",
        help = "Grant UpdateObject permissions on the namespace to the group",
        flag = true
    )]
    pub update_object: Option<bool>,

    #[option(
        long = "DeleteObject",
        help = "Grant DeleteObject permissions on the namespace to the group",
        flag = true
    )]
    pub delete_object: Option<bool>,

    #[option(
        long = "CreateClassRelation",
        help = "Grant CreateClassRelation permissions on the namespace to the group",
        flag = true
    )]
    pub create_class_relation: Option<bool>,

    #[option(
        long = "ReadClassRelation",
        help = "Grant ReadClassRelation permissions on the namespace to the group",
        flag = true
    )]
    pub read_class_relation: Option<bool>,

    #[option(
        long = "UpdateClassRelation",
        help = "Grant UpdateClassRelation permissions on the namespace to the group",
        flag = true
    )]
    pub update_class_relation: Option<bool>,

    #[option(
        long = "DeleteClassRelation",
        help = "Grant DeleteClassRelation permissions on the namespace to the group",
        flag = true
    )]
    pub delete_class_relation: Option<bool>,

    #[option(
        long = "CreateObjectRelation",
        help = "Grant CreateObjectRelation permissions on the namespace to the group",
        flag = true
    )]
    pub create_object_relation: Option<bool>,

    #[option(
        long = "ReadObjectRelation",
        help = "Grant ReadObjectRelation permissions on the namespace to the group",
        flag = true
    )]
    pub read_object_relation: Option<bool>,

    #[option(
        long = "UpdateObjectRelation",
        help = "Grant UpdateObjectRelation permissions on the namespace to the group",
        flag = true
    )]
    pub update_object_relation: Option<bool>,

    #[option(
        long = "DeleteObjectRelation",
        help = "Grant DeleteObjectRelation permissions on the namespace to the group",
        flag = true
    )]
    pub delete_object_relation: Option<bool>,
}

impl GetNamespace for &NamespacePermissionsSet {
    fn namespace(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for NamespacePermissionsSet {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        // 1) parse the raw args
        let mut new = Self::parse_tokens(tokens)?;

        // 2) figure out namespace (or positional)
        new.name = namespace_or_pos(&new, tokens, 0)?;
        if new.name.is_none() {
            return Err(AppError::MissingOptions(vec!["namespace".to_string()]));
        }

        // 3) collect all Permission enum variants into a Vec<Permission>
        //
        //    assume you have:
        //      #[derive(EnumIter, AsRefStr)] // or similar
        //      enum Permission { ReadObject, CreateObject, … }
        //
        let perms: Vec<NamespacePermission> = if new.all.is_some() {
            NamespacePermission::iter().collect()
        } else {
            let mut v = Vec::new();
            if new.read_collection.is_some() {
                v.push(NamespacePermission::ReadCollection);
            }
            if new.update_collection.is_some() {
                v.push(NamespacePermission::UpdateCollection);
            }
            if new.delete_collection.is_some() {
                v.push(NamespacePermission::DeleteCollection);
            }
            if new.delegate_collection.is_some() {
                v.push(NamespacePermission::DelegateCollection);
            }
            if new.create_class.is_some() {
                v.push(NamespacePermission::CreateClass);
            }
            if new.read_class.is_some() {
                v.push(NamespacePermission::ReadClass);
            }
            if new.update_class.is_some() {
                v.push(NamespacePermission::UpdateClass);
            }
            if new.delete_class.is_some() {
                v.push(NamespacePermission::DeleteClass);
            }
            if new.create_object.is_some() {
                v.push(NamespacePermission::CreateObject);
            }
            if new.read_object.is_some() {
                v.push(NamespacePermission::ReadObject);
            }
            if new.update_object.is_some() {
                v.push(NamespacePermission::UpdateObject);
            }
            if new.delete_object.is_some() {
                v.push(NamespacePermission::DeleteObject);
            }
            if new.create_class_relation.is_some() {
                v.push(NamespacePermission::CreateClassRelation);
            }
            if new.read_class_relation.is_some() {
                v.push(NamespacePermission::ReadClassRelation);
            }
            if new.update_class_relation.is_some() {
                v.push(NamespacePermission::UpdateClassRelation);
            }
            if new.delete_class_relation.is_some() {
                v.push(NamespacePermission::DeleteClassRelation);
            }
            if new.create_object_relation.is_some() {
                v.push(NamespacePermission::CreateObjectRelation);
            }
            if new.read_object_relation.is_some() {
                v.push(NamespacePermission::ReadObjectRelation);
            }
            if new.update_object_relation.is_some() {
                v.push(NamespacePermission::UpdateObjectRelation);
            }
            if new.delete_object_relation.is_some() {
                v.push(NamespacePermission::DeleteObjectRelation);
            }
            v
        };

        if perms.is_empty() {
            return Err(AppError::MissingOptions(vec!["permission".to_string()]));
        }

        // 4) turn them into strings (or send the enum directly if your API accepts it)
        services.gateway().grant_namespace_permissions(
            new.name.as_ref().unwrap(),
            &new.group,
            &perms,
        )?;

        let perm_string = if new.all.is_some() {
            "all permissions".to_string()
        } else {
            perms
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        };

        let message = format!(
            "Granted {} to group '{}' on namespace '{}'",
            perm_string,
            new.group,
            new.name.as_ref().unwrap()
        );

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

fn namespace_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<String>, AppError>
where
    U: GetNamespace,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.namespace().is_none() {
        if pos0.is_none() {
            return Err(AppError::MissingOptions(vec!["namespace".to_string()]));
        }
        return Ok(pos0.cloned());
    };
    Ok(query.namespace().clone())
}
