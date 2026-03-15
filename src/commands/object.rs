use std::collections::HashMap;

use cli_command_derive::CommandArgs;
use jqesque::Jqesque;
use jsonpath_rust::JsonPath;

use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, equals_clause, render_list_page, want_json,
    CliCommand,
};
use crate::autocomplete::{classes, namespaces, object_sort, object_where, objects_from_class};
use crate::catalog::CommandCatalogBuilder;
use crate::config::get_config;
use crate::domain::ObjectShowRecord;
use crate::errors::AppError;
use crate::formatting::{
    append_json_message, render_related_object_tree_with_key, OutputFormatter,
};
use crate::models::OutputFormat;
use crate::output::{add_warning, append_key_value, append_line};
use crate::services::{
    AppServices, CreateObjectInput, ObjectUpdateInput, RelationTraversalOptions,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["object"],
            catalog_command(
                "create",
                ObjectNew::default(),
                CommandDocs {
                    about: Some("Create an object"),
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
            catalog_command(
                "list",
                ObjectList::default(),
                CommandDocs {
                    about: Some("List objects"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["object"],
            catalog_command(
                "delete",
                ObjectDelete::default(),
                CommandDocs {
                    about: Some("Delete an object"),
                    ..CommandDocs::default()
                },
            ),
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
            catalog_command(
                "show",
                ObjectInfo::default(),
                CommandDocs {
                    about: Some("Show object details"),
                    ..CommandDocs::default()
                },
            ),
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
    #[option(
        long = "include-self-class",
        help = "Include returned relations in the same class as the root object",
        flag = "true"
    )]
    pub include_self_class: Option<bool>,
    #[option(
        long = "max-depth",
        help = "Maximum traversal depth to include in related object output"
    )]
    pub max_depth: Option<i32>,
}

impl CliCommand for ObjectInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = objectname_or_pos(&query, tokens, 0)?;

        let object_name = query
            .name
            .as_ref()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        let config = get_config();
        let object = services.gateway().object_show_details(
            &query.class,
            object_name,
            &RelationTraversalOptions {
                include_self_class: query
                    .include_self_class
                    .unwrap_or(!config.relations.ignore_same_class),
                max_depth: query.max_depth.unwrap_or(config.relations.max_depth),
            },
        )?;

        if want_json(tokens) {
            append_line(serde_json::to_string_pretty(&object)?)?;
            return Ok(());
        }

        render_object_show_text(&object)?;

        let show_data = should_render_object_data(
            query.data,
            query.jsonpath.as_deref(),
            config.output.object_show_data,
        );
        if !show_data {
            return Ok(());
        }

        append_line("")?;
        render_object_data(object.object.data.as_ref(), query.jsonpath.as_deref())
    }
}

fn render_object_show_text(object: &ObjectShowRecord) -> Result<(), AppError> {
    object.object.format()?;
    let relation_padding = get_config().output.padding.saturating_sub(1);
    render_related_object_tree_with_key("Relations", &object.related_objects, relation_padding)
}

fn render_object_data(
    json_data: Option<&serde_json::Value>,
    jsonpath: Option<&str>,
) -> Result<(), AppError> {
    let Some(json_data) = json_data else {
        return Ok(());
    };

    if let Some(jsonpath_expr) = jsonpath {
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
            let value = display_json_value(result.val());
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
        return Ok(());
    }

    let flattener = smooth_json::Flattener {
        ..Default::default()
    };

    let v = flattener.flatten(json_data);

    if let serde_json::Value::Object(map) = v {
        let sorted_map: std::collections::BTreeMap<_, _> = map.into_iter().collect();
        let padding = sorted_map
            .keys()
            .map(|k| k.len())
            .max()
            .map_or(15, |len| len.max(15));

        for (key, value) in sorted_map {
            append_key_value(key, display_json_value(&value), padding)?;
        }
    } else {
        add_warning("JSON is not an object")?;
    }

    Ok(())
}

fn should_render_object_data(
    show_data_flag: Option<bool>,
    jsonpath: Option<&str>,
    config_default: bool,
) -> bool {
    jsonpath.is_some() || show_data_flag.unwrap_or(config_default)
}

fn display_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use serial_test::serial;

    use super::display_json_value;
    use super::{render_object_data, render_object_show_text, should_render_object_data};
    use crate::domain::{ObjectShowRecord, RelatedObjectTreeNode, ResolvedObjectRecord};
    use crate::output::{append_line, reset_output, take_output};

    #[test]
    fn display_json_value_unquotes_strings() {
        assert_eq!(display_json_value(&json!("Entry")), "Entry");
    }

    #[test]
    fn display_json_value_keeps_non_scalars_as_json() {
        assert_eq!(display_json_value(&json!(["a", "b"])), "[\"a\",\"b\"]");
        assert_eq!(display_json_value(&json!({"k": "v"})), "{\"k\":\"v\"}");
    }

    #[test]
    fn should_render_object_data_uses_config_unless_flag_or_path_overrides() {
        assert!(!should_render_object_data(None, None, false));
        assert!(should_render_object_data(None, None, true));
        assert!(should_render_object_data(Some(true), None, false));
        assert!(should_render_object_data(None, Some("$.hello"), false));
    }

    #[test]
    #[serial]
    fn object_show_renders_relations_before_data() {
        reset_output().expect("output should reset");
        let object = ObjectShowRecord {
            object: ResolvedObjectRecord {
                id: 1,
                name: "Entry".to_string(),
                description: String::new(),
                namespace: "default".to_string(),
                class: "Contacts".to_string(),
                data: Some(json!({"email": "a@example.com"})),
                created_at: "2024-01-01 00:00:00".to_string(),
                updated_at: "2024-01-01 00:00:00".to_string(),
            },
            related_objects: vec![RelatedObjectTreeNode {
                id: 2,
                class: "Jacks".to_string(),
                name: "BL14=521.A7-UD7056".to_string(),
                namespace: "default".to_string(),
                depth: 1,
                children: vec![RelatedObjectTreeNode {
                    id: 3,
                    class: "Rooms".to_string(),
                    name: "B701".to_string(),
                    namespace: "default".to_string(),
                    depth: 2,
                    children: vec![],
                }],
            }],
        };

        render_object_show_text(&object).expect("show text should render");
        append_line("").expect("separator should render");
        render_object_data(object.object.data.as_ref(), None).expect("data should render");

        let snapshot = take_output().expect("snapshot should exist");
        let relations_index = snapshot
            .lines
            .iter()
            .position(|line| line.starts_with("Relations"))
            .expect("relations line should exist");
        let data_index = snapshot
            .lines
            .iter()
            .position(|line| line.starts_with("email"))
            .expect("data line should exist");

        assert!(relations_index < data_index);
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Jacks/BL14=521.A7-UD7056 → Rooms/B701")));
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
    pub class: Option<String>,
    #[option(
        short = "n",
        long = "name",
        help = "Name of the object",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: Option<String>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "object_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "object_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for ObjectList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query: ObjectList = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [
                query.class.map(|value| equals_clause("class", value)),
                query.name.map(|value| contains_clause("name", value)),
                query
                    .description
                    .map(|value| contains_clause("description", value)),
            ]
            .into_iter()
            .flatten(),
        )?;
        let objects = services.gateway().list_objects(&list_query)?;
        render_list_page(tokens, &objects)
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
            reclass: new.reclass,
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
