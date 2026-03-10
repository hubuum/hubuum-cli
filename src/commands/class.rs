use cli_command_derive::CliCommand;
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::autocomplete::{bool, classes, namespaces};
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::{AppServices, ClassFilter, CreateClassInput};
use crate::tokenizer::CommandTokenizer;

trait GetClassname {
    fn classname(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Create a new class",
    long_about = "Create a new class with the specified properties.",
    examples = r#"-n MyClass -N namespace_1 -d "My class description"
--name MyClass --namespace namespace_1 --description 'My class' --schema '{\"type\": \"object\"}'"#
)]
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
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let result = services.gateway().create_class(CreateClassInput {
            name: new.name,
            namespace: new.namespace,
            description: new.description,
            json_schema: new.json_schema,
            validate_schema: new.validate_schema,
        })?;

        match self.desired_format(tokens) {
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

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut query = self.new_from_tokens(tokens)?;
        query.name = classname_or_pos(&query, tokens, 0)?;
        let details = services
            .gateway()
            .class_details(&query.name.clone().unwrap())?;

        match self.desired_format(tokens) {
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

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let query = self.new_from_tokens(tokens)?;
        let name = match classname_or_pos(&query, tokens, 0)? {
            Some(name) => name,
            None => return Err(AppError::MissingOptions(vec!["name".to_string()])),
        };

        services.gateway().delete_class(&name)?;

        let message = format!("Class '{name}' deleted successfully");

        match self.desired_format(tokens) {
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

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;
        let classes = services.gateway().list_classes(ClassFilter {
            name: new.name,
            description: new.description,
        })?;

        match self.desired_format(tokens) {
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
