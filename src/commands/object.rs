use std::collections::HashMap;

use cli_command_derive::CommandArgs;
use jqesque::Jqesque;
use jsonpath_rust::JsonPath;

use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, want_json, CliCommand};
use crate::catalog::CommandCatalogBuilder;

use crate::autocomplete::{classes, namespaces, objects_from_class};
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{add_warning, append_key_value, append_line};
use crate::services::{AppServices, CreateObjectInput, ObjectFilter, ObjectUpdateInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["object"],
            catalog_command(
                "create",
                ObjectNew::default(),
                CommandDocs {
                    about: Some("Create a object class"),
                    long_about: Some(
                        "Create a new object in a specific class with the specified properties.",
                    ),
                    examples: Some(
                        r#"-n MyObject -c MyClaass -N namespace_1 -d "My object description"
--name MyObject --class MyClass --namespace namespace_1 --description 'My object' --data '{"key": "val"}'"#,
                    ),
                },
            ),
        )
        .add_command(
            &["object"],
            catalog_command("list", ObjectList::default(), CommandDocs::default()),
        )
        .add_command(
            &["object"],
            catalog_command("delete", ObjectDelete::default(), CommandDocs::default()),
        )
        .add_command(
            &["object"],
            catalog_command(
                "modify",
                ObjectModify::default(),
                CommandDocs {
                    about: Some("Modify an object"),
                    long_about: Some(
                        "Modify an object in a specific class with the specified properties.",
                    ),
                    examples: Some(
                        r#"-n MyObject -c MyClaass -N namespace_1 -d "My object description"
--name MyObject --class MyClass --namespace namespace_1 --description 'My object' --data foo.bar=4"#,
                    ),
                },
            ),
        )
        .add_command(
            &["object"],
            catalog_command("info", ObjectInfo::default(), CommandDocs::default()),
        );
}

trait GetObjectname {
    fn objectname(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectNew {
    #[option(short = "n", long = "name", help = "Name of the object")]
    pub name: String,
    #[option(
        short = "c",
        long = "class",
        help = "Name of the class the object belongs to",
        autocomplete = "classes"
    )]
    pub class: String,
    #[option(
        short = "N",
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: String,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: String,
    #[option(
        short = "D",
        long = "data",
        help = "JSON data for the object the class"
    )]
    pub data: Option<serde_json::Value>,
}

impl CliCommand for ObjectNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let object = services.gateway().create_object(CreateObjectInput {
            name: new.name,
            class_name: new.class,
            namespace: new.namespace,
            description: new.description,
            data: new.data,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => object.format_json_noreturn()?,
            OutputFormat::Text => object.format_noreturn()?,
        }

        Ok(())
    }
}

impl GetObjectname for &ObjectInfo {
    fn objectname(&self) -> Option<String> {
        self.name.clone()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectInfo {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the object",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(
        short = "c",
        long = "class",
        help = "Class of the object",
        autocomplete = "classes"
    )]
    pub class: String,
    #[option(
        short = "d",
        long = "data",
        help = "Show flattned data (key=value) for the object",
        flag = "true"
    )]
    pub data: Option<bool>,
    #[option(
        short = "p",
        long = "path",
        help = "Path to display within the data, implies -d"
    )]
    pub jsonpath: Option<String>,
}

impl CliCommand for ObjectInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = objectname_or_pos(&query, tokens, 0)?;

        let object_name = query
            .name
            .as_ref()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        let object = services
            .gateway()
            .object_details(&query.class, object_name)?;

        if want_json(tokens) {
            append_line(serde_json::to_string_pretty(&object)?)?;
            return Ok(());
        }

        object.format()?;

        if query.jsonpath.is_none() && query.data.is_none() {
            return Ok(());
        }

        if object.data.is_none() {
            return Ok(());
        }

        let json_data = object.data.clone().unwrap();

        if let Some(jsonpath_expr) = &query.jsonpath {
            let results: Vec<_> = json_data
                .query_with_path(jsonpath_expr)
                .map_err(|e| AppError::JsonPathError(e.to_string()))?;
            if results.is_empty() {
                add_warning("JSONPath did not match any data")?;
                return Ok(());
            }

            let mut key_values = HashMap::new();
            for result in results {
                let pretty_path = prettify_slice_path(&result.clone().path());
                let value = result.val();
                key_values.insert(pretty_path, value);
            }

            let padding = key_values
                .keys()
                .map(|k| k.len())
                .max()
                .map_or(14, |len| len.max(14));

            for (key, value) in key_values {
                append_key_value(key, value, padding)?;
            }
        } else {
            let flattener = smooth_json::Flattener {
                ..Default::default()
            };

            let v = flattener.flatten(&json_data);

            if let serde_json::Value::Object(map) = v {
                let sorted_map: std::collections::BTreeMap<_, _> = map.into_iter().collect();
                let padding = sorted_map
                    .keys()
                    .map(|k| k.len())
                    .max()
                    .map_or(15, |len| len.max(15));

                for (key, value) in sorted_map {
                    append_key_value(key, value, padding)?;
                }
            } else {
                add_warning("JSON is not an object")?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the object",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(
        short = "c",
        long = "class",
        help = "Class of the object",
        autocomplete = "classes"
    )]
    pub class: Option<String>,
}

impl CliCommand for ObjectDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = objectname_or_pos(&query, tokens, 1)?;

        let class_name = query
            .class
            .as_ref()
            .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
        let object_name = query
            .name
            .as_ref()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        services.gateway().delete_object(class_name, object_name)?;

        let message = format!(
            "Object '{}' in class '{}' deleted successfully",
            object_name, class_name
        );

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

impl GetObjectname for &ObjectDelete {
    fn objectname(&self) -> Option<String> {
        self.name.clone()
    }
}

fn objectname_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<String>, AppError>
where
    U: GetObjectname,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.objectname().is_none() {
        if pos0.is_none() {
            return Err(AppError::MissingOptions(vec!["name".to_string()]));
        }
        return Ok(pos0.cloned());
    };
    Ok(query.objectname().clone())
}

fn prettify_slice_path(path: &str) -> String {
    path.trim_start_matches('$')
        .replace("']['", ".")
        .replace("['", "")
        .replace("']", "")
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectList {
    #[option(
        short = "c",
        long = "class",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub class: String,
    #[option(
        short = "n",
        long = "name",
        help = "Name of the object",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: Option<String>,
}

impl CliCommand for ObjectList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new: ObjectList = Self::parse_tokens(tokens)?;
        let objects = services.gateway().list_objects(ObjectFilter {
            class_name: new.class,
            name: new.name,
            description: new.description,
        })?;

        if objects.is_empty() {
            append_line("No objects found")?;
            return Ok(());
        }

        match desired_format(tokens) {
            OutputFormat::Json => objects.format_json_noreturn()?,
            OutputFormat::Text => objects.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectModify {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the object",
        autocomplete = "objects_from_class"
    )]
    pub name: String,
    #[option(
        short = "c",
        long = "class",
        help = "Name of the class the object belongs to",
        autocomplete = "classes"
    )]
    pub class: String,
    #[option(short = "r", long = "rename", help = "Rename object")]
    pub rename: Option<String>,
    #[option(
        short = "R",
        long = "reclass",
        help = "Reclass object",
        autocomplete = "classes"
    )]
    pub reclass: Option<String>,
    #[option(
        short = "N",
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the object")]
    pub description: Option<String>,
    #[option(short = "D", long = "data", help = "JSON data for the object")]
    pub data: Option<String>,
}

impl CliCommand for ObjectModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let object = services.gateway().object_details(&new.class, &new.name)?;

        let data = if let Some(data) = &new.data {
            let jqesque = data.parse::<Jqesque>()?;
            let mut json_data = serde_json::Value::Null;
            if let Some(current_data) = object.data.clone() {
                json_data = current_data;
            }
            jqesque.apply_to(&mut json_data)?;
            Some(json_data)
        } else {
            None
        };
        let object = services.gateway().update_object(ObjectUpdateInput {
            name: new.name,
            class_name: new.class,
            rename: new.rename,
            namespace: new.namespace,
            description: new.description,
            data,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => object.format_json_noreturn()?,
            OutputFormat::Text => object.format_noreturn()?,
        }

        Ok(())
    }
}
