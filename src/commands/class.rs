use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::CommandCatalogBuilder;

use crate::autocomplete::{bool, classes, namespaces};
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::{AppServices, ClassFilter, ClassUpdateInput, CreateClassInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["class"],
            catalog_command(
                "create",
                ClassNew::default(),
                CommandDocs {
                    about: Some("Create a new class"),
                    long_about: Some("Create a new class with the specified properties."),
                    examples: Some(
                        r#"-n MyClass -N namespace_1 -d "My class description"
--name MyClass --namespace namespace_1 --description 'My class' --schema '{\"type\": \"object\"}'"#,
                    ),
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "list",
                ClassList::default(),
                CommandDocs {
                    about: Some("List classes"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "delete",
                ClassDelete::default(),
                CommandDocs {
                    about: Some("Delete a class"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "show",
                ClassInfo::default(),
                CommandDocs {
                    about: Some("Show class details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "modify",
                ClassModify::default(),
                CommandDocs {
                    about: Some("Modify a class"),
                    long_about: Some("Update an existing class by name."),
                    examples: Some(
                        r#"modify my-class --rename new-class
modify --name my-class --description "Updated description" --namespace other-ns"#,
                    ),
                },
            ),
        );
}

trait GetClassname {
    fn classname(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassNew {
    #[option(short = "n", long = "name", help = "Name of the class")]
    pub name: String,
    #[option(
        short = "N",
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: String,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: String,
    #[option(short = "s", long = "schema", help = "JSON schema for the class")]
    pub json_schema: Option<serde_json::Value>,
    #[option(
        short = "v",
        long = "validate",
        help = "Validate against schema, requires schema to be set",
        autocomplete = "bool"
    )]
    pub validate_schema: Option<bool>,
}

impl CliCommand for ClassNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let result = services.gateway().create_class(CreateClassInput {
            name: new.name,
            namespace: new.namespace,
            description: new.description,
            json_schema: new.json_schema,
            validate_schema: new.validate_schema,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => result.format_json_noreturn()?,
            OutputFormat::Text => result.format_noreturn()?,
        }

        Ok(())
    }
}

impl GetClassname for &ClassInfo {
    fn classname(&self) -> Option<String> {
        self.name.clone()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassInfo {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
}

impl CliCommand for ClassInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = classname_or_pos(&query, tokens, 0)?;
        let details = services
            .gateway()
            .class_details(&query.name.clone().unwrap())?;

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(serde_json::to_string_pretty(&details)?)?;
            }
            OutputFormat::Text => {
                details.class.format()?;
                append_key_value("Objects", details.objects.len(), 14)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
}

impl CliCommand for ClassDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = match classname_or_pos(&query, tokens, 0)? {
            Some(name) => name,
            None => return Err(AppError::MissingOptions(vec!["name".to_string()])),
        };

        services.gateway().delete_class(&name)?;

        let message = format!("Class '{name}' deleted successfully");

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

impl GetClassname for &ClassDelete {
    fn classname(&self) -> Option<String> {
        self.name.clone()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassModify {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the class")]
    pub rename: Option<String>,
    #[option(
        short = "N",
        long = "namespace",
        help = "Move the class to another namespace",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "New description of the class"
    )]
    pub description: Option<String>,
    #[option(short = "s", long = "schema", help = "JSON schema for the class")]
    pub json_schema: Option<serde_json::Value>,
    #[option(
        short = "v",
        long = "validate",
        help = "Set schema validation",
        autocomplete = "bool"
    )]
    pub validate_schema: Option<bool>,
}

impl GetClassname for &ClassModify {
    fn classname(&self) -> Option<String> {
        self.name.clone()
    }
}

impl CliCommand for ClassModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = classname_or_pos(&query, tokens, 0)?;
        let name = query
            .name
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;

        let updated = services.gateway().update_class(ClassUpdateInput {
            name,
            rename: query.rename,
            namespace: query.namespace,
            description: query.description,
            json_schema: query.json_schema,
            validate_schema: query.validate_schema,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => updated.format_json_noreturn()?,
            OutputFormat::Text => updated.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassList {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: Option<String>,
}

impl CliCommand for ClassList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let classes = services.gateway().list_classes(ClassFilter {
            name: new.name,
            description: new.description,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => classes.format_json_noreturn()?,
            OutputFormat::Text => classes.format_noreturn()?,
        }

        Ok(())
    }
}

fn classname_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<String>, AppError>
where
    U: GetClassname,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.classname().is_none() {
        if pos0.is_none() {
            return Err(AppError::MissingOptions(vec!["name".to_string()]));
        }
        return Ok(pos0.cloned());
    };
    Ok(query.classname().clone())
}
