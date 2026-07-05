use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
use crate::terminal::terminal_width;

const AUTO_OBJECT_DATA_COLUMN_LIMIT: usize = 4;
const AUTO_OBJECT_DATA_TARGET_WIDTH: usize = 100;
const AUTO_OBJECT_DATA_MAX_COLUMN_WIDTH: usize = 24;
const OBJECT_FIELD_SAMPLE_LIMIT: usize = 100;
const OBJECT_FIELD_DEPTH: usize = 6;
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
                "fields",
                ObjectFields::default(),
                CommandDocs {
                    about: Some("Inspect observed object data fields"),
                    long_about: Some(
                        "Sample objects in a class and list observed data paths, value types, counts, and examples. This is useful for classes without schemas.",
                    ),
                    examples: Some("--class Hosts --limit 100"),
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
    use std::collections::HashMap;

    use serde_json::json;
    use serial_test::serial;

    use super::{
        bounded_auto_data_columns, data_column_display_value, data_column_value,
        display_json_value, explicit_data_columns, first_seen_data_keys, object_data_column_label,
        object_field_summaries, object_list_row, ObjectListColumns, OBJECT_FIELD_DEPTH,
    };
    use super::{render_object_data, render_object_show_text, should_render_object_data};
    use crate::config::{init_config, AppConfig};
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
    fn object_list_row_accepts_data_prefixed_column_selectors() {
        let object = test_object(1, json!({"name": "data-name", "hardware": {"cpu": "M2"}}));
        let columns = ObjectListColumns {
            data_keys: vec!["data.name".to_string(), "data.hardware.cpu".to_string()],
            compact_base: true,
        };

        let row = object_list_row(&object, &columns)
            .expect("row should render")
            .as_object()
            .cloned()
            .expect("row should be object");

        assert_eq!(row.get("data.name"), Some(&json!("data-name")));
        assert_eq!(row.get("data.hardware.cpu"), Some(&json!("M2")));
    }

    #[test]
    fn data_column_value_accepts_raw_and_data_prefixed_paths() {
        let data = json!({"name": "host", "hardware": {"cpu": "M2"}});
        let data = data.as_object().expect("data object");

        assert_eq!(data_column_value(data, "name"), Some(&json!("host")));
        assert_eq!(data_column_value(data, "data.name"), Some(&json!("host")));
        assert_eq!(
            data_column_value(data, "data.hardware.cpu"),
            Some(&json!("M2"))
        );
    }

    #[test]
    fn data_column_display_value_accepts_array_wildcards_and_indexes() {
        let data = json!({
            "network": {
                "interfaces": [
                    {"ipv4": "127.0.0.1"},
                    {"ipv4": "127.0.0.2"}
                ]
            }
        });
        let data = data.as_object().expect("data object");

        assert_eq!(
            data_column_display_value(data, "data.network.interfaces[*].ipv4"),
            Some("127.0.0.1,127.0.0.2".to_string())
        );
        assert_eq!(
            data_column_display_value(data, "data.network.interfaces[1].ipv4"),
            Some("127.0.0.2".to_string())
        );
    }

    #[test]
    #[serial]
    fn object_list_row_inserts_configured_meta_columns() {
        let mut config = AppConfig::default();
        config.output.object_list_class_meta.insert(
            "Hosts".to_string(),
            HashMap::from([(
                "os_version".to_string(),
                vec![
                    "data.os.macos.version".to_string(),
                    "data.os.redhat.version".to_string(),
                ],
            )]),
        );
        init_config(config).expect("config should initialize");
        let object = test_object(1, json!({"os": {"redhat": {"version": "9.8"}}}));
        let columns = ObjectListColumns {
            data_keys: vec!["os_version".to_string()],
            compact_base: true,
        };

        let row = object_list_row(&object, &columns)
            .expect("row should render")
            .as_object()
            .cloned()
            .expect("row should be object");

        assert_eq!(row.get("os_version"), Some(&json!("9.8")));
    }

    #[test]
    fn object_field_summaries_collect_nested_data_paths() {
        let page = PagedResult {
            items: vec![
                test_object(1, json!({"contact": "Entry", "hardware": {"cpu": "M2"}})),
                test_object(2, json!({"contact": "Dell", "hardware": {"cpu": "i3"}})),
            ],
            next_cursor: None,
            limit: None,
            returned_count: 2,
        };

        let summaries = object_field_summaries(&page, OBJECT_FIELD_DEPTH, false);

        assert_eq!(
            summaries.get("data.contact").map(|summary| summary.count),
            Some(2)
        );
        assert_eq!(
            summaries.get("data.hardware.cpu").map(|summary| summary
                .types
                .iter()
                .copied()
                .collect::<Vec<_>>()),
            Some(vec!["string"])
        );
    }

    #[test]
    fn object_field_summaries_expand_array_item_paths() {
        let page = PagedResult {
            items: vec![test_object(
                1,
                json!({"network": {"interfaces": [{"ipv4": "127.0.0.1"}]}}),
            )],
            next_cursor: None,
            limit: None,
            returned_count: 1,
        };

        let summaries = object_field_summaries(&page, OBJECT_FIELD_DEPTH, false);

        assert!(summaries.contains_key("data.network.interfaces[*].ipv4"));
        assert!(!summaries.contains_key("data.network"));
        assert!(!summaries.contains_key("data.network.interfaces"));
    }

    #[test]
    fn object_field_summaries_can_include_containers() {
        let page = PagedResult {
            items: vec![test_object(1, json!({"hardware": {"cpu": "M2"}}))],
            next_cursor: None,
            limit: None,
            returned_count: 1,
        };

        let summaries = object_field_summaries(&page, OBJECT_FIELD_DEPTH, true);

        assert!(summaries.contains_key("data.hardware"));
        assert!(summaries.contains_key("data.hardware.cpu"));
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
            bounded_auto_data_columns(
                keys,
                &PagedResult {
                    items: vec![
                        test_object(
                            1,
                            json!({
                                "contact": "Entry",
                                "cpu_cpuinfo": "8 x Intel(R) Core(TM) i3-10100 CPU @ 3.60GHz",
                                "date": 1643704976,
                                "ip": "129.240.222.98",
                                "ipv4": "129.240.222.98",
                            }),
                        ),
                        test_object(
                            2,
                            json!({
                                "contact": "Entry",
                                "cpu_cpuinfo": "8 x Apple M2",
                                "date": 1694599127,
                                "ip": "129.240.222.51",
                                "ipv4": "129.240.222.51",
                            }),
                        ),
                    ],
                    next_cursor: None,
                    limit: None,
                    returned_count: 2,
                },
            ),
            vec![
                "contact".to_string(),
                "date".to_string(),
                "ip".to_string(),
                "ipv4".to_string(),
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

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectFields {
    #[option(
        short = "c",
        long = "class",
        help = "Name of the class to sample",
        autocomplete = "classes"
    )]
    pub class: String,
    #[option(
        long = "limit",
        help = "Maximum number of objects to sample (default: 100)"
    )]
    pub limit: Option<usize>,
    #[option(
        long = "depth",
        help = "Maximum data path depth to inspect (default: 6)"
    )]
    pub depth: Option<usize>,
    #[option(
        long = "containers",
        help = "Include object and array container paths",
        flag = true
    )]
    pub containers: Option<bool>,
}

impl CliCommand for ObjectFields {
    fn execute(&self, services: &AppServices, _tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(_tokens)?;
        let sample_limit = query.limit.unwrap_or(OBJECT_FIELD_SAMPLE_LIMIT);
        let list_query = build_list_query(
            &[],
            &[],
            Some(sample_limit),
            None,
            [equals_clause("class", query.class.clone())],
        )?;
        let objects = services.gateway().list_objects(&list_query)?;
        render_object_fields(
            &objects,
            sample_limit,
            query.depth.unwrap_or(OBJECT_FIELD_DEPTH),
            query.containers.unwrap_or(false),
        )
    }
}

#[derive(Debug, Default)]
struct FieldSummary {
    count: usize,
    types: BTreeSet<&'static str>,
    example: Option<String>,
}

fn render_object_fields(
    objects: &PagedResult<ResolvedObjectRecord>,
    sample_limit: usize,
    depth: usize,
    include_containers: bool,
) -> Result<(), AppError> {
    let summaries = object_field_summaries(objects, depth, include_containers);
    let rows = summaries
        .into_iter()
        .map(|(field, summary)| {
            serde_json::json!({
                "Field": field,
                "Count": summary.count,
                "Types": summary.types.into_iter().collect::<Vec<_>>().join(","),
                "Example": summary.example.unwrap_or_default(),
                "Sample": objects.returned_count,
                "Limit": sample_limit,
            })
        })
        .collect::<Vec<_>>();
    set_semantic_output(hubuum_filter::OutputEnvelope::rows(
        rows,
        vec![
            "Field".to_string(),
            "Count".to_string(),
            "Types".to_string(),
            "Example".to_string(),
            "Sample".to_string(),
            "Limit".to_string(),
        ],
    ))?;
    Ok(())
}

fn object_field_summaries(
    objects: &PagedResult<ResolvedObjectRecord>,
    depth: usize,
    include_containers: bool,
) -> BTreeMap<String, FieldSummary> {
    let mut summaries = BTreeMap::new();
    for object in &objects.items {
        if let Some(data) = object.data.as_ref() {
            collect_object_field_summaries(
                data,
                "data",
                0,
                depth,
                include_containers,
                &mut summaries,
            );
        }
    }
    summaries
}

fn collect_object_field_summaries(
    value: &serde_json::Value,
    path: &str,
    depth: usize,
    max_depth: usize,
    include_containers: bool,
    summaries: &mut BTreeMap<String, FieldSummary>,
) {
    let is_container = matches!(
        value,
        serde_json::Value::Object(_) | serde_json::Value::Array(_)
    );
    if path != "data" && (!is_container || include_containers) {
        let summary = summaries.entry(path.to_string()).or_default();
        summary.count += 1;
        summary.types.insert(json_type_name(value));
        summary
            .example
            .get_or_insert_with(|| data_preview(Some(value)));
    }

    if depth >= max_depth {
        return;
    }

    if let Some(object) = value.as_object() {
        for (key, value) in object {
            collect_object_field_summaries(
                value,
                &format!("{path}.{key}"),
                depth + 1,
                max_depth,
                include_containers,
                summaries,
            );
        }
    } else if let Some(array) = value.as_array() {
        for item in array {
            collect_object_field_summaries(
                item,
                &format!("{path}[*]"),
                depth + 1,
                max_depth,
                include_containers,
                summaries,
            );
        }
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
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
        let mut columns = vec!["id".to_string(), "Name".to_string()];
        if !self.compact_base {
            columns.extend([
                "Description".to_string(),
                "Namespace".to_string(),
                "Class".to_string(),
            ]);
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
                let keys = match configured_object_data_columns(class_filter) {
                    Some(columns) => columns,
                    None => auto_object_data_columns(services, objects, class_filter)?,
                };
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

fn configured_object_data_columns(class_filter: Option<&str>) -> Option<Vec<String>> {
    let class_name = class_filter?;
    let columns = get_config()
        .output
        .object_list_class_columns
        .get(class_name)?
        .clone();
    (!columns.is_empty()).then_some(columns)
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
    Ok(bounded_auto_data_columns(keys, objects))
}

fn bounded_auto_data_columns(
    keys: Vec<String>,
    objects: &PagedResult<ResolvedObjectRecord>,
) -> Vec<String> {
    let target_width = auto_object_data_target_width();
    let mut selected = Vec::new();
    let mut estimated_width = auto_base_width(objects);

    for key in keys {
        if selected.len() >= AUTO_OBJECT_DATA_COLUMN_LIMIT {
            break;
        }

        let width = data_column_width(&key, objects);
        if width > AUTO_OBJECT_DATA_MAX_COLUMN_WIDTH {
            continue;
        }

        let next_width = estimated_width + 3 + width;
        if next_width > target_width && !selected.is_empty() {
            continue;
        }

        selected.push(key);
        estimated_width = next_width;
    }

    selected
}

fn auto_object_data_target_width() -> usize {
    terminal_width()
        .filter(|width| *width >= 60)
        .unwrap_or(AUTO_OBJECT_DATA_TARGET_WIDTH)
}

fn auto_base_width(objects: &PagedResult<ResolvedObjectRecord>) -> usize {
    let id_width = objects
        .items
        .iter()
        .map(|object| object.id.to_string().len())
        .chain(std::iter::once("id".len()))
        .max()
        .unwrap_or("id".len());
    let name_width = objects
        .items
        .iter()
        .map(|object| object.name.len())
        .chain(std::iter::once("Name".len()))
        .max()
        .unwrap_or("Name".len());
    id_width + 3 + name_width
}

fn data_column_width(key: &str, objects: &PagedResult<ResolvedObjectRecord>) -> usize {
    objects
        .items
        .iter()
        .filter_map(|object| {
            object
                .data
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .and_then(|data| data.get(key))
        })
        .map(|value| data_preview(Some(value)).len())
        .chain(std::iter::once(object_data_column_header(key).len()))
        .max()
        .unwrap_or_else(|| object_data_column_header(key).len())
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
    let data_object = object.data.as_ref().and_then(serde_json::Value::as_object);
    if let Some(data) = data_object {
        insert_meta_columns(&mut row, &object.class, data);
    }

    if columns.data_keys.is_empty() {
        row.insert(
            "Data".to_string(),
            serde_json::Value::String(data_preview(object.data.as_ref())),
        );
    } else if let Some(data) = data_object {
        for key in &columns.data_keys {
            if let Some(value) = data_or_meta_column_display_value(&object.class, data, key) {
                row.insert(
                    object_data_column_label(key),
                    serde_json::Value::String(value),
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

fn insert_meta_columns(
    row: &mut serde_json::Map<String, serde_json::Value>,
    class_name: &str,
    data: &serde_json::Map<String, serde_json::Value>,
) {
    if let Some(meta) = get_config().output.object_list_class_meta.get(class_name) {
        for (alias, selectors) in meta {
            if let Some(value) = meta_column_display_value(data, selectors) {
                row.insert(alias.clone(), serde_json::Value::String(value));
            }
        }
    }
}

fn object_data_column_label(key: &str) -> String {
    if key.starts_with("data.") {
        return key.to_string();
    }

    match key {
        "id" | "name" | "description" | "namespace" | "class" | "created_at" | "updated_at"
        | "Name" | "Description" | "Namespace" | "Class" | "Data" | "Created" | "Updated" => {
            format!("data.{key}")
        }
        _ => key.to_string(),
    }
}

fn object_data_column_header(key: &str) -> String {
    let label = object_data_column_label(key);
    label.strip_prefix("data.").unwrap_or(&label).to_string()
}

#[cfg(test)]
fn data_column_value<'a>(
    data: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a serde_json::Value> {
    data_column_values(data, key).into_iter().next()
}

fn data_column_display_value(
    data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    let values = data_column_values(data, key);
    match values.as_slice() {
        [] => None,
        [value] => Some(data_preview(Some(value))),
        many => Some(
            many.iter()
                .map(|value| data_preview(Some(value)))
                .collect::<Vec<_>>()
                .join(","),
        ),
    }
}

fn data_or_meta_column_display_value(
    class_name: &str,
    data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    data_column_display_value(data, key).or_else(|| {
        get_config()
            .output
            .object_list_class_meta
            .get(class_name)
            .and_then(|meta| meta.get(key))
            .and_then(|selectors| meta_column_display_value(data, selectors))
    })
}

fn meta_column_display_value(
    data: &serde_json::Map<String, serde_json::Value>,
    selectors: &[String],
) -> Option<String> {
    selectors
        .iter()
        .find_map(|selector| data_column_display_value(data, selector))
}

fn data_column_values<'a>(
    data: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Vec<&'a serde_json::Value> {
    let key = key.strip_prefix("data.").unwrap_or(key);
    let mut current = Vec::new();
    for (index, part) in key.split('.').enumerate() {
        if index == 0 {
            current = select_data_part_from_object(data, part);
        } else {
            current = select_data_part(current, part);
        }
        if current.is_empty() {
            break;
        }
    }
    current
}

fn select_data_part_from_object<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    part: &str,
) -> Vec<&'a serde_json::Value> {
    let (field, selectors) = split_data_selector_part(part);
    let Some(value) = object.get(field) else {
        return Vec::new();
    };
    apply_data_selectors(vec![value], selectors)
}

fn select_data_part<'a>(
    values: Vec<&'a serde_json::Value>,
    part: &str,
) -> Vec<&'a serde_json::Value> {
    let (field, selectors) = split_data_selector_part(part);
    let mut selected = Vec::new();
    for value in values {
        if let Some(object) = value.as_object() {
            if let Some(value) = object.get(field) {
                selected.push(value);
            }
        }
    }
    apply_data_selectors(selected, selectors)
}

fn split_data_selector_part(part: &str) -> (&str, &str) {
    part.find('[')
        .map(|index| (&part[..index], &part[index..]))
        .unwrap_or((part, ""))
}

fn apply_data_selectors<'a>(
    mut values: Vec<&'a serde_json::Value>,
    mut selectors: &str,
) -> Vec<&'a serde_json::Value> {
    while let Some(inner) = selectors.strip_prefix('[') {
        let Some(end) = inner.find(']') else {
            break;
        };
        let selector = &inner[..end];
        let mut next = Vec::new();
        for value in values {
            if let Some(array) = value.as_array() {
                if selector == "*" {
                    next.extend(array);
                } else if let Ok(index) = selector.parse::<usize>() {
                    if let Some(value) = array.get(index) {
                        next.push(value);
                    }
                }
            }
        }
        values = next;
        selectors = &inner[end + 1..];
    }
    values
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
