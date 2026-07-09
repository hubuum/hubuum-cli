use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, desired_format, render_list_page, required_option_or_pos, CliCommand,
};
use crate::catalog::CommandCatalogBuilder;

use crate::autocomplete::{
    collection_sort, collection_where, collections, groups, principal_kinds, principal_names,
};
use crate::domain::CollectionPermission;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::list_query::filter_clause;
use crate::models::OutputFormat;
use crate::output::{append_json, append_line};
use crate::services::{AppServices, CollectionUpdateInput, CreateCollectionInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["collection"],
            catalog_command(
                "create",
                CollectionNew::default(),
                CommandDocs {
                    about: Some("Create a collection"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["collection"],
            catalog_command(
                "list",
                CollectionList::default(),
                CommandDocs {
                    about: Some("List collections"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["collection"],
            catalog_command(
                "delete",
                CollectionDelete::default(),
                CommandDocs {
                    about: Some("Delete a collection"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["collection"],
            catalog_command(
                "show",
                CollectionInfo::default(),
                CommandDocs {
                    about: Some("Show collection details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["collection"],
            catalog_command(
                "modify",
                CollectionModify::default(),
                CommandDocs {
                    about: Some("Modify a collection"),
                    long_about: Some("Update an existing collection by name."),
                    examples: Some(
                        r#"modify my-collection --rename other-ns
modify --name my-collection --description "Updated description""#,
                    ),
                },
            ),
        )
        .add_command(
            &["collection", "permissions"],
            catalog_command(
                "list",
                CollectionPermissions::default(),
                CommandDocs {
                    about: Some("List permissions for a collection"),
                    long_about: Some(
                        "Show collection permissions for a single collection. Pass the collection as the first positional argument or with --name.",
                    ),
                    examples: Some(
                        r#"list my-collection
list --name my-collection"#,
                    ),
                },
            ),
        )
        .add_command(
            &["collection", "permissions"],
            catalog_command(
                "set",
                CollectionPermissionsSet::default(),
                CommandDocs {
                    about: Some("Grant permissions on a collection"),
                    long_about: Some(
                        "Grant collection permissions to a group. Pass the collection as the first positional argument or with --name, then select permissions with --all or individual permission flags.",
                    ),
                    examples: Some(
                        r#"set my-collection --group editors --all
set --name my-collection --group readers --ReadCollection --ReadClass --ReadObject"#,
                    ),
                },
            ),
        )
        .add_command(
            &["collection"],
            catalog_command(
                "principal-permissions",
                CollectionPrincipalPermissions::default(),
                CommandDocs {
                    about: Some("List principal permissions for a collection"),
                    long_about: Some(
                        "Show collection permissions for a given principal. Pass the collection as the first positional argument or with --name, and identify the principal with --principal-kind and --principal.",
                    ),
                    examples: Some(
                        r#"principal-permissions my-collection --principal-kind group --principal admins
principal-permissions --name my-collection --principal-kind user --principal alice"#,
                    ),
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionNew {
    #[option(short = "n", long = "name", help = "Name of the collection")]
    pub name: String,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the collection"
    )]
    pub description: String,
    #[option(
        short = "o",
        long = "owner",
        help = "Name of the group owning collection"
    )]
    pub owner: String,
}

impl CliCommand for CollectionNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let collection = services
            .gateway()
            .create_collection(CreateCollectionInput {
                name: new.name,
                description: new.description,
                owner: new.owner,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => collection.format_json_noreturn()?,
            OutputFormat::Text => collection.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionList {
    #[option(short = "n", long = "name", help = "Name of the collection")]
    pub name: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the collection"
    )]
    pub description: Option<String>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "collection_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "collection_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for CollectionList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [
                query.name.map(|value| {
                    filter_clause(
                        "name",
                        hubuum_client::FilterOperator::Contains { is_negated: false },
                        value,
                    )
                }),
                query.description.map(|value| {
                    filter_clause(
                        "description",
                        hubuum_client::FilterOperator::Contains { is_negated: false },
                        value,
                    )
                }),
            ]
            .into_iter()
            .flatten(),
        )?;
        let collections = services.gateway().list_collections(&list_query)?;
        render_list_page(tokens, &collections)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionInfo {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the collection",
        autocomplete = "collections"
    )]
    pub name: Option<String>,
}

impl CliCommand for CollectionInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "collection")?;
        let collection = services.gateway().get_collection(&name)?;

        match desired_format(tokens) {
            OutputFormat::Json => collection.format_json_noreturn()?,
            OutputFormat::Text => collection.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the collection",
        autocomplete = "collections"
    )]
    pub name: Option<String>,
}

impl CliCommand for CollectionDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let collection_name = required_option_or_pos(query.name, tokens, 0, "collection")?;
        services.gateway().delete_collection(&collection_name)?;

        let message = format!("Collection '{}' deleted", collection_name);

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionModify {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the collection",
        autocomplete = "collections"
    )]
    pub name: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the collection")]
    pub rename: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the collection"
    )]
    pub description: Option<String>,
}

impl CliCommand for CollectionModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "collection")?;

        let collection = services
            .gateway()
            .update_collection(CollectionUpdateInput {
                name,
                rename: query.rename,
                description: query.description,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => collection.format_json_noreturn()?,
            OutputFormat::Text => collection.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionPermissions {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the collection",
        autocomplete = "collections"
    )]
    pub name: Option<String>,
}

impl CliCommand for CollectionPermissions {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "collection")?;

        let permissions = services.gateway().list_collection_permissions(&name)?;

        let empty_message = format!("No permissions found for collection '{name}'");

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
pub struct CollectionPermissionsSet {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the collection",
        autocomplete = "collections"
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
        help = "Grant ReadCollection permissions on the collection to the group",
        flag = true
    )]
    pub read_collection: Option<bool>,

    #[option(
        long = "UpdateCollection",
        help = "Grant UpdateCollection permissions on the collection to the group",
        flag = true
    )]
    pub update_collection: Option<bool>,

    #[option(
        long = "DeleteCollection",
        help = "Grant DeleteCollection permissions on the collection to the group",
        flag = true
    )]
    pub delete_collection: Option<bool>,

    #[option(
        long = "DelegateCollection",
        help = "Grant DelegateCollection permissions on the collection to the group",
        flag = true
    )]
    pub delegate_collection: Option<bool>,

    #[option(
        long = "CreateClass",
        help = "Grant CreateClass permissions on the collection to the group",
        flag = true
    )]
    pub create_class: Option<bool>,

    #[option(
        long = "ReadClass",
        help = "Grant ReadClass permissions on the collection to the group",
        flag = true
    )]
    pub read_class: Option<bool>,

    #[option(
        long = "UpdateClass",
        help = "Grant UpdateClass permissions on the collection to the group",
        flag = true
    )]
    pub update_class: Option<bool>,

    #[option(
        long = "DeleteClass",
        help = "Grant DeleteClass permissions on the collection to the group",
        flag = true
    )]
    pub delete_class: Option<bool>,

    #[option(
        long = "CreateObject",
        help = "Grant CreateObject permissions on the collection to the group",
        flag = true
    )]
    pub create_object: Option<bool>,

    #[option(
        long = "ReadObject",
        help = "Grant ReadObject permissions on the collection to the group",
        flag = true
    )]
    pub read_object: Option<bool>,

    #[option(
        long = "UpdateObject",
        help = "Grant UpdateObject permissions on the collection to the group",
        flag = true
    )]
    pub update_object: Option<bool>,

    #[option(
        long = "DeleteObject",
        help = "Grant DeleteObject permissions on the collection to the group",
        flag = true
    )]
    pub delete_object: Option<bool>,

    #[option(
        long = "CreateClassRelation",
        help = "Grant CreateClassRelation permissions on the collection to the group",
        flag = true
    )]
    pub create_class_relation: Option<bool>,

    #[option(
        long = "ReadClassRelation",
        help = "Grant ReadClassRelation permissions on the collection to the group",
        flag = true
    )]
    pub read_class_relation: Option<bool>,

    #[option(
        long = "UpdateClassRelation",
        help = "Grant UpdateClassRelation permissions on the collection to the group",
        flag = true
    )]
    pub update_class_relation: Option<bool>,

    #[option(
        long = "DeleteClassRelation",
        help = "Grant DeleteClassRelation permissions on the collection to the group",
        flag = true
    )]
    pub delete_class_relation: Option<bool>,

    #[option(
        long = "CreateObjectRelation",
        help = "Grant CreateObjectRelation permissions on the collection to the group",
        flag = true
    )]
    pub create_object_relation: Option<bool>,

    #[option(
        long = "ReadObjectRelation",
        help = "Grant ReadObjectRelation permissions on the collection to the group",
        flag = true
    )]
    pub read_object_relation: Option<bool>,

    #[option(
        long = "UpdateObjectRelation",
        help = "Grant UpdateObjectRelation permissions on the collection to the group",
        flag = true
    )]
    pub update_object_relation: Option<bool>,

    #[option(
        long = "DeleteObjectRelation",
        help = "Grant DeleteObjectRelation permissions on the collection to the group",
        flag = true
    )]
    pub delete_object_relation: Option<bool>,
}

impl CliCommand for CollectionPermissionsSet {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let collection = required_option_or_pos(new.name, tokens, 0, "collection")?;

        // Collect the explicit permission flags into the enum values expected by the API.
        let perms: Vec<CollectionPermission> = if new.all.is_some() {
            CollectionPermission::iter().collect()
        } else {
            let mut v = Vec::new();
            if new.read_collection.is_some() {
                v.push(CollectionPermission::ReadCollection);
            }
            if new.update_collection.is_some() {
                v.push(CollectionPermission::UpdateCollection);
            }
            if new.delete_collection.is_some() {
                v.push(CollectionPermission::DeleteCollection);
            }
            if new.delegate_collection.is_some() {
                v.push(CollectionPermission::DelegateCollection);
            }
            if new.create_class.is_some() {
                v.push(CollectionPermission::CreateClass);
            }
            if new.read_class.is_some() {
                v.push(CollectionPermission::ReadClass);
            }
            if new.update_class.is_some() {
                v.push(CollectionPermission::UpdateClass);
            }
            if new.delete_class.is_some() {
                v.push(CollectionPermission::DeleteClass);
            }
            if new.create_object.is_some() {
                v.push(CollectionPermission::CreateObject);
            }
            if new.read_object.is_some() {
                v.push(CollectionPermission::ReadObject);
            }
            if new.update_object.is_some() {
                v.push(CollectionPermission::UpdateObject);
            }
            if new.delete_object.is_some() {
                v.push(CollectionPermission::DeleteObject);
            }
            if new.create_class_relation.is_some() {
                v.push(CollectionPermission::CreateClassRelation);
            }
            if new.read_class_relation.is_some() {
                v.push(CollectionPermission::ReadClassRelation);
            }
            if new.update_class_relation.is_some() {
                v.push(CollectionPermission::UpdateClassRelation);
            }
            if new.delete_class_relation.is_some() {
                v.push(CollectionPermission::DeleteClassRelation);
            }
            if new.create_object_relation.is_some() {
                v.push(CollectionPermission::CreateObjectRelation);
            }
            if new.read_object_relation.is_some() {
                v.push(CollectionPermission::ReadObjectRelation);
            }
            if new.update_object_relation.is_some() {
                v.push(CollectionPermission::UpdateObjectRelation);
            }
            if new.delete_object_relation.is_some() {
                v.push(CollectionPermission::DeleteObjectRelation);
            }
            v
        };

        if perms.is_empty() {
            return Err(AppError::MissingOptions(vec!["permission".to_string()]));
        }

        services
            .gateway()
            .grant_collection_permissions(&collection, &new.group, &perms)?;

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
            "Granted {} to group '{}' on collection '{}'",
            perm_string, new.group, collection
        );

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct CollectionPrincipalPermissions {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the collection",
        autocomplete = "collections"
    )]
    pub name: Option<String>,

    #[option(
        long = "principal-kind",
        help = "Principal kind: user, group, or service-account",
        autocomplete = "principal_kinds"
    )]
    pub principal_kind: String,
    #[option(
        short = "p",
        long = "principal",
        help = "Principal name",
        autocomplete = "principal_names"
    )]
    pub principal: String,
}

impl CliCommand for CollectionPrincipalPermissions {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(new.name, tokens, 0, "collection")?;

        let principal_id = principal_id_by_name(services, &new.principal_kind, &new.principal)?;
        let permissions = services
            .gateway()
            .principal_collection_permissions(&name, principal_id)?;

        let empty_message = format!(
            "No permissions found for principal '{}' in collection '{}'",
            new.principal, name
        );

        match (desired_format(tokens), permissions.is_empty()) {
            (OutputFormat::Json, true) => append_json_message(&empty_message)?,
            (OutputFormat::Json, false) => append_json(&permissions)?,
            (OutputFormat::Text, true) => append_line(empty_message)?,
            (OutputFormat::Text, false) => {
                use crate::domain::GroupPermissionsSummary;
                let summary: Vec<GroupPermissionsSummary> = permissions
                    .into_iter()
                    .map(|p| GroupPermissionsSummary::from(p.0))
                    .collect();
                summary.format_noreturn()?;
            }
        }

        Ok(())
    }
}

fn principal_id_by_name(services: &AppServices, kind: &str, name: &str) -> Result<i32, AppError> {
    match kind {
        "user" => services.gateway().user_id_by_name(name),
        "group" => services.gateway().group_id_by_name(name),
        "service-account" => services.gateway().service_account_id_by_name(name),
        other => Err(AppError::InvalidOption(format!("principal-kind={other}"))),
    }
}
