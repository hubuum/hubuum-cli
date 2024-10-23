use std::collections::HashMap;

use cli_command_derive::CliCommand;

use hubuum_client::{
    Authenticated, IntoResourceFilter, Object, ObjectPatch, ObjectPost, QueryFilter, SyncClient,
};
use jqesque::Jqesque;
use jsonpath_rust::{JsonPath, JsonPathValue};

use serde::{Deserialize, Serialize};

use super::shared::{find_object_by_name, prettify_slice_path};
use super::{CliCommand, CliCommandInfo, CliOption};

use crate::commands::shared::{find_class_by_name, find_entities_by_ids, find_namespace_by_name};
use crate::errors::AppError;
use crate::formatting::{FormattedObject, OutputFormatter, OutputFormatterWithPadding};
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
        autocomplete = "crate::commandlist::classes"
    )]
    pub class: String,
    #[option(
        short = "N",
        long = "namespace",
        help = "Namespace name",
        autocomplete = "crate::commandlist::namespaces"
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
        let namespace = find_namespace_by_name(client, &new.namespace)?;
        let class = find_class_by_name(client, &new.class)?;

        let result = client.objects(class.id).create(ObjectPost {
            name: new.name.clone(),
            hubuum_class_id: class.id,
            namespace_id: namespace.id,
            description: new.description.clone(),
            data: new.data.clone(),
        })?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id, class.clone());

        let mut nsmap = HashMap::new();
        nsmap.insert(namespace.id, namespace.clone());

        let object = FormattedObject::new(&result, &classmap, &nsmap);

        object.format(15)?;

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
            });
        }
        if let Some(id) = &self.id {
            filters.push(QueryFilter {
                key: "id".to_string(),
                value: id.to_string(),
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
    #[option(short = "i", long = "id", help = "ID of the object")]
    pub id: Option<i32>,
    #[option(short = "n", long = "name", help = "Name of the object")]
    pub name: Option<String>,
    #[option(
        short = "c",
        long = "class",
        help = "Class of the object",
        autocomplete = "crate::commandlist::classes"
    )]
    pub class: String,
    #[option(
        short = "d",
        long = "data",
        help = "Show full JSON data",
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

        let class = find_class_by_name(client, &query.class)?;
        let object = find_object_by_name(client, class.id, &query.name.unwrap())?;

        let namespace = client
            .namespaces()
            .find()
            .add_filter_id(object.namespace_id)
            .execute_expecting_single_result()?;

        let mut nsmap = HashMap::new();
        nsmap.insert(namespace.id, namespace.clone());

        let mut classmap = HashMap::new();
        classmap.insert(class.id, class.clone());

        let object = FormattedObject::new(&object, &classmap, &nsmap);
        object.format(15)?;

        if query.jsonpath.is_none() && query.data.is_none() {
            return Ok(());
        }

        if object.data.is_none() {
            add_warning("JSON data requested, but object has no data")?;
            return Ok(());
        }

        let json_data = object.data.unwrap().clone();

        if query.jsonpath.is_some() {
            let jsonpath = query.jsonpath.clone().unwrap();
            let path = jsonpath
                .parse::<JsonPath>()
                .map_err(|e| AppError::JsonPathError(e.to_string()))?;

            let slice_of_data = path.find_slice(&json_data);
            if slice_of_data.is_empty() {
                add_warning("JSONPath did not match any data")?;
                return Ok(());
            }

            // Hashmap to store the key value pairs, allowing for sorting and padding lookups
            let mut key_values = HashMap::new();

            // Iterate over the slices and handle each JsonPathValue
            for slice in slice_of_data {
                match slice {
                    JsonPathValue::Slice(value, path) => {
                        let pretty_path = prettify_slice_path(&path);
                        key_values.insert(pretty_path, value.clone());
                    }
                    JsonPathValue::NewValue(value) => {
                        key_values.insert("Generated".to_string(), value.clone());
                    }
                    JsonPathValue::NoValue => {
                        add_warning(format!("{} produced no results", jsonpath))?;
                    }
                }
            }

            let padding = key_values
                .keys()
                .map(|k| k.len())
                .max()
                .map_or(15, |len| len.max(15));

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
    #[option(short = "n", long = "name", help = "Name of the object")]
    pub name: Option<String>,
    #[option(short = "c", long = "class", help = "Class of the object")]
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
            find_class_by_name(client, &query.class.unwrap())?
        } else {
            return Err(AppError::MissingOptions(vec!["class".to_string()]));
        };

        let object = find_object_by_name(client, class.id, &query.name.unwrap())?;

        client.objects(class.id).delete(object.id)?;
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
    #[option(short = "c", long = "class", help = "Name of the class")]
    pub class: String,
    #[option(short = "n", long = "name", help = "Name of the object")]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: Option<String>,
}

impl IntoResourceFilter<Object> for &ObjectList {
    fn into_resource_filter(self) -> Vec<QueryFilter> {
        let mut filters = vec![];
        if let Some(name) = &self.name {
            filters.push(QueryFilter {
                key: "name__contains".to_string(),
                value: name.clone(),
            });
        }
        if let Some(description) = &self.description {
            filters.push(QueryFilter {
                key: "description__contains".to_string(),
                value: description.clone(),
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

        let class = find_class_by_name(client, &new.class)?;

        let objects = client.objects(class.id).filter(&new)?;

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

        objects.format()?;
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
    #[option(short = "n", long = "name", help = "Name of the object")]
    pub name: String,
    #[option(
        short = "c",
        long = "class",
        help = "Name of the class the object belongs to"
    )]
    pub class: String,
    #[option(short = "r", long = "rename", help = "Rename object")]
    pub rename: Option<String>,
    #[option(short = "R", long = "reclass", help = "Reclass object")]
    pub reclass: Option<String>,
    #[option(short = "N", long = "namespace", help = "Namespace name")]
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
        let class = find_class_by_name(client, &new.class)?;
        let object = find_object_by_name(client, class.id, &new.name)?;

        let mut patch = ObjectPatch::default();

        if let Some(data) = &new.data.clone() {
            let jqesque = data.parse::<Jqesque>()?;
            let mut json_data = serde_json::Value::Null;
            if object.data.is_some() {
                json_data = object.data.unwrap().clone();
            }
            jqesque.merge_into(&mut json_data);
            patch.data = Some(json_data);
        }

        if let Some(namespace) = &new.namespace {
            let namespace = find_namespace_by_name(client, namespace)?;
            patch.namespace_id = Some(namespace.id);
        };

        if let Some(rename) = &new.rename {
            patch.name = Some(rename.clone());
        }

        if let Some(description) = &new.description {
            patch.description = Some(description.clone());
        }

        let result = client.objects(class.id).update(object.id, patch)?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id, class.clone());

        let namespace = client
            .namespaces()
            .find()
            .add_filter_id(result.namespace_id)
            .execute_expecting_single_result()?;

        let mut nsmap = HashMap::new();
        nsmap.insert(namespace.id, namespace.clone());

        let object = FormattedObject::new(&result, &classmap, &nsmap);
        object.format(15)?;

        Ok(())
    }
}
