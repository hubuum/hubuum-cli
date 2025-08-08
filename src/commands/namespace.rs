use cli_command_derive::CliCommand;
use hubuum_client::types::Permissions;
use hubuum_client::{Authenticated, FilterOperator, NamespacePost, SyncClient};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::autocomplete::{groups, namespaces};
use crate::errors::AppError;
use crate::formatting::{append_json_message, FormattedGroupPermissions, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::tokenizer::CommandTokenizer;

trait GetNamespace {
    fn namespace(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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

impl NamespaceNew {
    fn into_post(self, group_id: i32) -> NamespacePost {
        NamespacePost {
            name: self.name.clone(),
            description: self.description.clone(),
            group_id,
        }
    }
}

impl CliCommand for NamespaceNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;

        let group = client
            .groups()
            .find()
            .add_filter_name_exact(new.owner.clone())
            .execute_expecting_single_result()?;

        let post = new.into_post(group.id);

        let namespace = client.namespaces().create(post)?;

        match self.desired_format(tokens) {
            OutputFormat::Json => namespace.format_json_noreturn()?,
            OutputFormat::Text => namespace.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;

        let search = client.namespaces().find();

        let search = match &new.name {
            Some(name) => search.add_filter(
                "name",
                FilterOperator::Contains { is_negated: false },
                name.clone(),
            ),
            None => search,
        };

        let search = match &new.description {
            Some(description) => search.add_filter(
                "description",
                FilterOperator::Contains { is_negated: false },
                description.clone(),
            ),
            None => search,
        };

        let namespaces = search.execute()?;

        match self.desired_format(tokens) {
            OutputFormat::Json => namespaces.format_json_noreturn()?,
            OutputFormat::Text => namespaces.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut new = self.new_from_tokens(tokens)?;

        new.name = namespace_or_pos(&new, tokens, 0)?;

        if new.name.is_none() {
            return Err(AppError::MissingOptions(vec!["namespace".to_string()]));
        }

        let namespace = client
            .namespaces()
            .select_by_name(new.name.as_ref().unwrap())?;

        match self.desired_format(tokens) {
            OutputFormat::Json => namespace.format_json_noreturn()?,
            OutputFormat::Text => namespace.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut new = self.new_from_tokens(tokens)?;

        new.name = namespace_or_pos(&new, tokens, 0)?;

        if new.name.is_none() {
            return Err(AppError::MissingOptions(vec!["namespace".to_string()]));
        }

        let namespace = client
            .namespaces()
            .select_by_name(new.name.as_ref().unwrap())?;

        client.namespaces().delete(namespace.id())?;

        let message = format!("Namespace '{}' deleted", namespace.resource().name);

        match self.desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut new = self.new_from_tokens(tokens)?;

        new.name = namespace_or_pos(&new, tokens, 0)?;

        let name = match &new.name {
            Some(name) => name,
            None => return Err(AppError::MissingOptions(vec!["namespace".to_string()])),
        };

        let permissions = client
            .namespaces()
            .select_by_name(name)?
            .permissions()?
            .iter()
            .map(|p| FormattedGroupPermissions::from(p.clone()))
            .collect::<Vec<_>>();

        let empty_message = format!("No permissions found for namespace '{name}'");

        match (self.desired_format(tokens), permissions.is_empty()) {
            (OutputFormat::Json, true) => append_json_message(&empty_message)?,
            (OutputFormat::Json, false) => permissions.format_json_noreturn()?,
            (OutputFormat::Text, true) => append_line(empty_message)?,
            (OutputFormat::Text, false) => permissions.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct NamespacePermissionsGrant {
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

impl GetNamespace for &NamespacePermissionsGrant {
    fn namespace(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for NamespacePermissionsGrant {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        // 1) parse the raw args
        let mut new = self.new_from_tokens(tokens)?;

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
        let perms: Vec<Permissions> = if new.all.is_some() {
            Permissions::iter().collect()
        } else {
            let mut v = Vec::new();
            if new.read_collection.is_some() {
                v.push(Permissions::ReadCollection);
            }
            if new.update_collection.is_some() {
                v.push(Permissions::UpdateCollection);
            }
            if new.delete_collection.is_some() {
                v.push(Permissions::DeleteCollection);
            }
            if new.delegate_collection.is_some() {
                v.push(Permissions::DelegateCollection);
            }
            if new.create_class.is_some() {
                v.push(Permissions::CreateClass);
            }
            if new.read_class.is_some() {
                v.push(Permissions::ReadClass);
            }
            if new.update_class.is_some() {
                v.push(Permissions::UpdateClass);
            }
            if new.delete_class.is_some() {
                v.push(Permissions::DeleteClass);
            }
            if new.create_object.is_some() {
                v.push(Permissions::CreateObject);
            }
            if new.read_object.is_some() {
                v.push(Permissions::ReadObject);
            }
            if new.update_object.is_some() {
                v.push(Permissions::UpdateObject);
            }
            if new.delete_object.is_some() {
                v.push(Permissions::DeleteObject);
            }
            if new.create_class_relation.is_some() {
                v.push(Permissions::CreateClassRelation);
            }
            if new.read_class_relation.is_some() {
                v.push(Permissions::ReadClassRelation);
            }
            if new.update_class_relation.is_some() {
                v.push(Permissions::UpdateClassRelation);
            }
            if new.delete_class_relation.is_some() {
                v.push(Permissions::DeleteClassRelation);
            }
            if new.create_object_relation.is_some() {
                v.push(Permissions::CreateObjectRelation);
            }
            if new.read_object_relation.is_some() {
                v.push(Permissions::ReadObjectRelation);
            }
            if new.update_object_relation.is_some() {
                v.push(Permissions::UpdateObjectRelation);
            }
            if new.delete_object_relation.is_some() {
                v.push(Permissions::DeleteObjectRelation);
            }
            v
        };

        if perms.is_empty() {
            return Err(AppError::MissingOptions(vec!["permission".to_string()]));
        }

        // 4) turn them into strings (or send the enum directly if your API accepts it)
        let perm_names: Vec<String> = perms
            .iter()
            .map(|p| p.to_string()) // or `p.to_string()` if you impl Display
            .collect();

        // 5) lookup namespace & group, then call the client
        let namespace = client
            .namespaces()
            .select_by_name(new.name.as_ref().unwrap())?;
        let group = client.groups().select_by_name(&new.group)?;

        namespace.grant_permissions(group.id(), perm_names)?;

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

        match self.desired_format(tokens) {
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
