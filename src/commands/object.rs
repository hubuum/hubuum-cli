use std::collections::{HashMap, HashSet};

use cli_command_derive::CommandArgs;
use jqesque::Jqesque;
use jsonpath_rust::JsonPath;

use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, equals_clause, want_json, CliCommand,
};
use crate::autocomplete::{
    classes, namespaces, object_data_columns, object_sort, object_where, objects_from_class,
};
use crate::catalog::CommandCatalogBuilder;
use crate::config::get_config;
use crate::domain::{ObjectShowRecord, ResolvedObjectRecord};
use crate::errors::AppError;
use crate::formatting::{
    append_json_message, data_preview, render_related_object_tree_with_key, OutputFormatter,
};
use crate::list_query::{append_paging_footer, PagedResult};
use crate::models::{ObjectListDataColumns, OutputFormat};
use crate::output::{add_warning, append_key_value, append_line, set_semantic_output};
use crate::services::{
    AppServices, CreateObjectInput, ObjectUpdateInput, RelationTraversalOptions,
};

const AUTO_OBJECT_DATA_COLUMN_LIMIT: usize = 4;
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
        help = "JSON data for the object the class",
        value_source = true
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

    use super::{
        bounded_auto_data_columns, display_json_value, explicit_data_columns, first_seen_data_keys,
        object_data_column_label, object_list_row, ObjectListColumns,
    };
    use super::{render_object_data, render_object_show_text, should_render_object_data};
    use crate::domain::{ObjectShowRecord, RelatedObjectTreeNode, ResolvedObjectRecord};
    use crate::list_query::PagedResult;
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
    fn explicit_data_columns_preserve_requested_order() {
        assert_eq!(
            explicit_data_columns("contact, ip,cpu_cpuinfo"),
            vec![
                "contact".to_string(),
                "ip".to_string(),
                "cpu_cpuinfo".to_string()
            ]
        );
    }

    #[test]
    fn first_seen_data_keys_preserve_page_order() {
        let page = PagedResult {
            items: vec![
                test_object(1, json!({"contact": "Entry", "ip": "127.0.0.1"})),
                test_object(
                    2,
                    json!({"cpu_cpuinfo": "8 x Apple M4", "contact": "Bitpro"}),
                ),
            ],
            next_cursor: None,
            limit: None,
            returned_count: 2,
        };

        assert_eq!(
            first_seen_data_keys(&page),
            vec![
                "contact".to_string(),
                "ip".to_string(),
                "cpu_cpuinfo".to_string()
            ]
        );
    }

    #[test]
    fn object_list_row_expands_requested_data_columns() {
        let object = test_object(
            1,
            json!({"contact": "Entry", "ip": "127.0.0.1", "name": "data-name"}),
        );
        let columns = ObjectListColumns {
            data_keys: vec!["contact".to_string(), "name".to_string()],
            compact_base: false,
        };

        let row = object_list_row(&object, &columns)
            .expect("row should render")
            .as_object()
            .cloned()
            .expect("row should be object");

        assert_eq!(row.get("contact"), Some(&json!("Entry")));
        assert_eq!(row.get("data.name"), Some(&json!("data-name")));
        assert!(!row.contains_key("Data"));
    }

    #[test]
    fn auto_data_columns_are_bounded_for_wide_schemas() {
        let keys = vec![
            "contact".to_string(),
            "cpu_cpuinfo".to_string(),
            "date".to_string(),
            "ip".to_string(),
            "ipv4".to_string(),
        ];

        assert_eq!(
            bounded_auto_data_columns(keys),
            vec![
                "contact".to_string(),
                "cpu_cpuinfo".to_string(),
                "date".to_string(),
                "ip".to_string(),
            ]
        );
    }

    #[test]
    fn object_data_column_label_avoids_display_and_serialized_collisions() {
        assert_eq!(object_data_column_label("id"), "data.id");
        assert_eq!(object_data_column_label("name"), "data.name");
        assert_eq!(object_data_column_label("Name"), "data.Name");
        assert_eq!(object_data_column_label("contact"), "contact");
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

    fn test_object(id: i32, data: serde_json::Value) -> ResolvedObjectRecord {
        ResolvedObjectRecord {
            id,
            name: format!("host-{id}"),
            description: String::new(),
            namespace: "Math".to_string(),
            class: "Hosts".to_string(),
            data: Some(data),
            created_at: "2026-07-05 03:44:41".to_string(),
            updated_at: "2026-07-05 03:44:41".to_string(),
        }
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
    #[option(
        long = "data-columns",
        help = "Object data columns: auto, preview, all, or comma-separated keys",
        autocomplete = "object_data_columns"
    )]
    pub data_columns: Option<String>,
}

impl CliCommand for ObjectList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query: ObjectList = Self::parse_tokens(tokens)?;
        let class_filter = query.class.clone();
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
        render_object_list_page(
            services,
            tokens,
            &objects,
            class_filter.as_deref(),
            query.data_columns.as_deref(),
        )
    }
}

fn render_object_list_page(
    services: &AppServices,
    tokens: &CommandTokenizer,
    objects: &PagedResult<ResolvedObjectRecord>,
    class_filter: Option<&str>,
    data_columns: Option<&str>,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => {
            crate::list_query::render_paged_result(tokens, objects, OutputFormat::Json)
        }
        OutputFormat::Text => {
            let columns = object_list_columns(services, objects, class_filter, data_columns)?;
            let rows = objects
                .items
                .iter()
                .map(|object| object_list_row(object, &columns))
                .collect::<Result<Vec<_>, AppError>>()?;
            set_semantic_output(hubuum_filter::OutputEnvelope::rows(
                rows,
                columns.display_columns(),
            ))?;
            append_paging_footer(tokens, objects)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectListColumns {
    data_keys: Vec<String>,
    compact_base: bool,
}

impl ObjectListColumns {
    fn display_columns(&self) -> Vec<String> {
        let mut columns = vec![
            "id".to_string(),
            "Name".to_string(),
            "Description".to_string(),
            "Namespace".to_string(),
        ];
        if !self.compact_base {
            columns.push("Class".to_string());
        }
        if self.data_keys.is_empty() {
            columns.push("Data".to_string());
        } else {
            columns.extend(
                self.data_keys
                    .iter()
                    .map(|key| object_data_column_label(key)),
            );
        }
        if !self.compact_base {
            columns.extend(["Created".to_string(), "Updated".to_string()]);
        }
        columns
    }
}

fn object_list_columns(
    services: &AppServices,
    objects: &PagedResult<ResolvedObjectRecord>,
    class_filter: Option<&str>,
    requested: Option<&str>,
) -> Result<ObjectListColumns, AppError> {
    let (data_keys, compact_base) = match requested {
        Some(value) => match value.parse::<ObjectListDataColumns>() {
            Ok(ObjectListDataColumns::Preview) => (Vec::new(), false),
            Ok(ObjectListDataColumns::Auto) => {
                let keys = auto_object_data_columns(services, objects, class_filter)?;
                let compact_base = !keys.is_empty();
                (keys, compact_base)
            }
            Ok(ObjectListDataColumns::All) => (first_seen_data_keys(objects), false),
            Err(_) => (explicit_data_columns(value), false),
        },
        None => match get_config().output.object_list_data_columns {
            ObjectListDataColumns::Preview => (Vec::new(), false),
            ObjectListDataColumns::Auto => {
                let keys = auto_object_data_columns(services, objects, class_filter)?;
                let compact_base = !keys.is_empty();
                (keys, compact_base)
            }
            ObjectListDataColumns::All => (first_seen_data_keys(objects), false),
        },
    };
    Ok(ObjectListColumns {
        data_keys,
        compact_base,
    })
}

fn auto_object_data_columns(
    services: &AppServices,
    objects: &PagedResult<ResolvedObjectRecord>,
    class_filter: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let Some(class_name) = class_filter else {
        return Ok(Vec::new());
    };

    let schema_keys = services
        .gateway()
        .class_schema(class_name)?
        .as_ref()
        .map(schema_property_keys)
        .unwrap_or_default();
    let keys = if schema_keys.is_empty() {
        first_seen_data_keys(objects)
    } else {
        schema_keys
    };
    Ok(bounded_auto_data_columns(keys))
}

fn bounded_auto_data_columns(keys: Vec<String>) -> Vec<String> {
    keys.into_iter()
        .take(AUTO_OBJECT_DATA_COLUMN_LIMIT)
        .collect()
}

fn explicit_data_columns(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn schema_property_keys(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn first_seen_data_keys(objects: &PagedResult<ResolvedObjectRecord>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for object in &objects.items {
        if let Some(data) = object.data.as_ref().and_then(serde_json::Value::as_object) {
            for key in data.keys() {
                if seen.insert(key.clone()) {
                    keys.push(key.clone());
                }
            }
        }
    }
    keys
}

fn object_list_row(
    object: &ResolvedObjectRecord,
    columns: &ObjectListColumns,
) -> Result<serde_json::Value, AppError> {
    let mut row = match serde_json::to_value(object)? {
        serde_json::Value::Object(object) => object,
        value => {
            let mut object = serde_json::Map::new();
            object.insert("value".to_string(), value);
            object
        }
    };

    row.insert(
        "id".to_string(),
        serde_json::Value::String(object.id.to_string()),
    );
    row.insert(
        "Name".to_string(),
        serde_json::Value::String(object.name.clone()),
    );
    row.insert(
        "Description".to_string(),
        serde_json::Value::String(object.description.clone()),
    );
    row.insert(
        "Namespace".to_string(),
        serde_json::Value::String(object.namespace.clone()),
    );
    row.insert(
        "Class".to_string(),
        serde_json::Value::String(object.class.clone()),
    );
    if columns.data_keys.is_empty() {
        row.insert(
            "Data".to_string(),
            serde_json::Value::String(data_preview(object.data.as_ref())),
        );
    } else if let Some(data) = object.data.as_ref().and_then(serde_json::Value::as_object) {
        for key in &columns.data_keys {
            if let Some(value) = data.get(key) {
                row.insert(
                    object_data_column_label(key),
                    serde_json::Value::String(data_preview(Some(value))),
                );
            }
        }
    }
    row.insert(
        "Created".to_string(),
        serde_json::Value::String(object.created_at.clone()),
    );
    row.insert(
        "Updated".to_string(),
        serde_json::Value::String(object.updated_at.clone()),
    );

    Ok(serde_json::Value::Object(row))
}

fn object_data_column_label(key: &str) -> String {
    match key {
        "id" | "name" | "description" | "namespace" | "class" | "created_at" | "updated_at"
        | "Name" | "Description" | "Namespace" | "Class" | "Data" | "Created" | "Updated" => {
            format!("data.{key}")
        }
        _ => key.to_string(),
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
    #[option(
        short = "D",
        long = "data",
        help = "JSON data for the object",
        value_source = true
    )]
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
