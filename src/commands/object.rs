use std::collections::HashMap;

use cli_command_derive::CliCommand;

use hubuum_client::{
    Authenticated, FilterOperator, IntoResourceFilter, Object, ObjectPatch, ObjectPost,
    QueryFilter, SyncClient,
};
use jqesque::Jqesque;
use jsonpath_rust::JsonPath;

use serde::{Deserialize, Serialize};

use super::shared::prettify_slice_path;
use super::{CliCommand, CliCommandInfo, CliOption};

use crate::autocomplete::{classes, namespaces, objects_from_class};
use crate::commands::shared::find_entities_by_ids;
use crate::errors::AppError;
use crate::formatting::{append_json_message, FormattedObject, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{add_warning, append_key_value, append_line};
use crate::tokenizer::CommandTokenizer;

trait GetObjectname {
    fn objectname(&self) -> Option<String>;
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Create a object class",
    long_about = "Create a new object in a specific class with the specified properties.",
    examples = r#"-n MyObject -c MyClaass -N namespace_1 -d "My object description"
--name MyObject --class MyClass --namespace namespace_1 --description 'My object' --data '{"key": "val"}'"#
)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
        let namespace = client.namespaces().select_by_name(&new.namespace)?;
        let class = client.classes().select_by_name(&new.class)?;

        let result = client.objects(class.id()).create(ObjectPost {
            name: new.name.clone(),
            hubuum_class_id: class.id(),
            namespace_id: namespace.id(),
            description: new.description.clone(),
            data: new.data.clone(),
        })?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id(), class.resource().clone());

        let mut nsmap = HashMap::new();
        nsmap.insert(namespace.id(), namespace.resource().clone());

        let object = FormattedObject::new(&result, &classmap, &nsmap);

        match self.desired_format(tokens) {
            OutputFormat::Json => object.format_json_noreturn()?,
            OutputFormat::Text => object.format_noreturn()?,
        }

        Ok(())
    }
}

impl IntoResourceFilter<Object> for &ObjectInfo {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];
        if let Some(name) = &self.name {
            filters.push(QueryFilter {
                key: "name".to_string(),
                value: name.clone(),
                operator: FilterOperator::IContains { is_negated: false },
            });
        }
        filters
    }
}

impl GetObjectname for &ObjectInfo {
    fn objectname(&self) -> Option<String> {
        self.name.clone()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut query = self.new_from_tokens(tokens)?;
        query.name = objectname_or_pos(&query, tokens, 0)?;

        let class = client.classes().select_by_name(&query.class)?;
        let object = class.object_by_name(&query.name.unwrap())?;

        let namespace = client
            .namespaces()
            .find()
            .add_filter_id(object.resource().namespace_id)
            .execute_expecting_single_result()?;

        let mut nsmap = HashMap::new();
        nsmap.insert(namespace.id, namespace.clone());

        let mut classmap = HashMap::new();
        classmap.insert(class.id(), class.resource().clone());

        let object = FormattedObject::new(object.resource(), &classmap, &nsmap);

        if self.want_json(tokens) {
            append_line(serde_json::to_string_pretty(&object).unwrap())?;
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

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let mut query = self.new_from_tokens(tokens)?;
        query.name = objectname_or_pos(&query, tokens, 1)?;

        let class = if query.class.is_some() {
            client.classes().select_by_name(&query.class.unwrap())?
        } else {
            return Err(AppError::MissingOptions(vec!["class".to_string()]));
        };

        let object = class.object_by_name(&query.name.unwrap())?;

        client.objects(class.id()).delete(object.id())?;

        let message = format!(
            "Object '{}' in class '{}' deleted successfully",
            object.resource().name,
            class.resource().name
        );

        match self.desired_format(tokens) {
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

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
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

impl IntoResourceFilter<Object> for &ObjectList {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];
        if let Some(name) = &self.name {
            filters.push(QueryFilter {
                key: "name".to_string(),
                value: name.clone(),
                operator: FilterOperator::IContains { is_negated: false },
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

impl CliCommand for ObjectList {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new: ObjectList = self.new_from_tokens(tokens)?;

        let class = client.classes().select_by_name(&new.class)?;

        let objects = client.objects(class.id()).filter(&new)?;

        if objects.is_empty() {
            append_line("No objects found")?;
            return Ok(());
        }

        let classmap = find_entities_by_ids(&client.classes(), &objects, |o| o.hubuum_class_id)?;
        let nsmap = find_entities_by_ids(&client.namespaces(), &objects, |o| o.namespace_id)?;

        let objects = objects
            .iter()
            .map(|o| FormattedObject::new(o, &classmap, &nsmap))
            .collect::<Vec<_>>();

        match self.desired_format(tokens) {
            OutputFormat::Json => objects.format_json_noreturn()?,
            OutputFormat::Text => objects.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Modify an object",
    long_about = "Modify an object in a specific class with the specified properties.",
    examples = r#"-n MyObject -c MyClaass -N namespace_1 -d "My object description"
--name MyObject --class MyClass --namespace namespace_1 --description 'My object' --data foo.bar=4"#
)]
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
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
        let class = client.classes().select_by_name(&new.class)?;
        let object = class.object_by_name(&new.name)?;

        let mut patch = ObjectPatch::default();

        if let Some(data) = &new.data.clone() {
            let jqesque = data.parse::<Jqesque>()?;
            let mut json_data = serde_json::Value::Null;
            if object.resource().data.is_some() {
                json_data = object.resource().data.as_ref().unwrap().clone();
            }
            jqesque.apply_to(&mut json_data)?;
            patch.data = Some(json_data);
        }

        if let Some(namespace) = &new.namespace {
            let namespace = client.namespaces().select_by_name(namespace)?;
            patch.namespace_id = Some(namespace.id());
        };

        if let Some(rename) = &new.rename {
            patch.name = Some(rename.clone());
        }

        if let Some(description) = &new.description {
            patch.description = Some(description.clone());
        }

        let result = client.objects(class.id()).update(object.id(), patch)?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id(), class.resource().clone());

        let namespace = client.namespaces().select(result.namespace_id)?;

        let mut nsmap = HashMap::new();
        nsmap.insert(namespace.id(), namespace.resource().clone());

        let object = FormattedObject::new(&result, &classmap, &nsmap);

        match self.desired_format(tokens) {
            OutputFormat::Json => object.format_json_noreturn()?,
            OutputFormat::Text => object.format_noreturn()?,
        }

        Ok(())
    }
}
