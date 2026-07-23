use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::read_to_string;
use std::iter::once;

use cli_command_derive::CommandArgs;
use hubuum_client::ObjectDataPatchDocument;
use jqesque::Jqesque;
use jsonpath_rust::JsonPath;
use smooth_json::Flattener;

use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, to_string_pretty, to_value, Map, Value};

use hubuum_filter::{scalar_text, select_values, OutputEnvelope};

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, equals_clause, normalize_server_page_size,
    option_or_pos, want_json, CliCommand,
};
use crate::autocomplete::{
    classes, collections, computed_fields, object_data_columns, object_sort, object_where,
    objects_from_class,
};
use crate::catalog::CommandCatalogBuilder;
use crate::config::get_config;
use crate::domain::{
    visit_observed_data_fields, ComputedFieldSelector, ComputedFieldSet, ObjectShowRecord,
    ResolvedObjectRecord, DEFAULT_OBJECT_FIELD_DEPTH, DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT,
};
use crate::errors::AppError;
use crate::formatting::{
    append_json_message, data_preview, render_related_object_tree_with_key, OutputFormatter,
};
use crate::list_query::{append_paging_footer, render_paged_result, PagedResult};
use crate::models::{ObjectListDataColumns, OutputFormat};
use crate::output::{
    add_warning, append_key_value, append_line, has_pipeline, set_semantic_output,
};
use crate::services::{
    AppServices, CreateObjectInput, ObjectDataPatchInput, ObjectUpdateInput,
    RelationTraversalOptions,
};
use crate::terminal::terminal_width;

const AUTO_OBJECT_DATA_COLUMN_LIMIT: usize = 4;
const AUTO_OBJECT_DATA_TARGET_WIDTH: usize = 100;
const AUTO_OBJECT_DATA_MAX_COLUMN_WIDTH: usize = 24;
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
                        r#"-n MyObject -c MyClaass -N collection_1 -d "My object description"
--name MyObject --class MyClass --collection collection_1 --description 'My object' --data '{"key": "val"}'"#,
                    ),
                },
            ),
        )
        .add_command(
            &["object", "data"],
            catalog_command(
                "patch",
                ObjectDataPatch::default(),
                CommandDocs {
                    about: Some("Atomically patch an object's raw data"),
                    long_about: Some(
                        "Apply an RFC 6902 JSON Patch document through the exact class/object-name endpoint. With --create, a missing object is initialized by applying the patch to an empty JSON object. If a concurrent creator wins the race, the patch is retried once.",
                    ),
                    examples: Some(
                        r#"--class Hosts --name srv-01 --patch '[{"op":"add","path":"/facts","value":{"os":"Fedora"}}]'
--class Hosts --name srv-01 --patch @facts-patch.json --create --description "Managed by Ansible""#,
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
                        r#"-n MyObject -c MyClaass -N collection_1 -d "My object description"
--name MyObject --class MyClass --collection collection_1 --description 'My object' --data foo.bar=4"#,
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
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: String,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: String,
    #[option(
        short = "D",
        long = "data",
        help = "JSON data for the object the class",
        value_source = true
    )]
    pub data: Option<Value>,
}

impl CliCommand for ObjectNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let object = services.gateway().create_object(CreateObjectInput {
            name: new.name,
            class_name: new.class,
            collection: new.collection,
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

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectDataPatch {
    #[option(
        short = "n",
        long = "name",
        help = "Exact name of the object",
        autocomplete = "objects_from_class"
    )]
    pub name: String,
    #[option(
        short = "c",
        long = "class",
        help = "Exact name of the object's class",
        autocomplete = "classes"
    )]
    pub class: String,
    #[option(
        short = "p",
        long = "patch",
        help = "RFC 6902 JSON Patch document; use @FILE, file://FILE, or inline JSON",
        value_source = true
    )]
    pub patch: String,
    #[option(
        long = "create",
        help = "Create the object when it does not exist",
        flag = "true"
    )]
    pub create: bool,
    #[option(
        short = "d",
        long = "description",
        help = "Description to use when creating a missing object"
    )]
    pub description: Option<String>,
}

impl CliCommand for ObjectDataPatch {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        if query.description.is_some() && !query.create {
            return Err(AppError::ParseError(
                "--description requires --create".to_string(),
            ));
        }

        let patch = parse_object_data_patch(&query.patch)?;
        let mut input = ObjectDataPatchInput::new(query.class, query.name, patch)?;
        if query.create {
            input = input.create_if_missing(query.description.unwrap_or_default());
        }
        let result = services.gateway().patch_object_data(input)?;

        match desired_format(tokens) {
            OutputFormat::Json => result.format_json_noreturn()?,
            OutputFormat::Text => result.format_noreturn()?,
        }

        Ok(())
    }
}

fn parse_object_data_patch(source: &str) -> Result<ObjectDataPatchDocument, AppError> {
    let payload = if let Some(path) = source.strip_prefix('@') {
        if path.is_empty() {
            return Err(AppError::ParseError(
                "--patch @FILE requires a file path".to_string(),
            ));
        }
        read_to_string(path)?
    } else {
        source.to_string()
    };
    Ok(from_str(&payload)?)
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
    #[option(
        long = "computed",
        help = "Computed field to show: S:key, P:key, all, or none (repeatable)",
        nargs = 1,
        autocomplete = "computed_fields"
    )]
    pub computed: Vec<String>,
}

impl CliCommand for ObjectInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = option_or_pos(query.name, tokens, 0, "name")?;

        let object_name = query
            .name
            .as_ref()
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        let computed_selection =
            ComputedFieldSelection::resolve(&query.computed, Some(&query.class))?;
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
            computed_selection.requests_values(),
        )?;

        if has_pipeline()? {
            let (value, columns) = object_show_pipeline_value(&object, &computed_selection)?;
            set_semantic_output(OutputEnvelope::detail(value, columns))?;
            return Ok(());
        }

        if want_json(tokens) {
            append_line(to_string_pretty(
                &computed_selection.project_object_show(&object),
            )?)?;
            return Ok(());
        }

        render_object_show_text(&object)?;

        let computed_columns = computed_selection.columns(std::slice::from_ref(&object.object));
        if !computed_columns.is_empty() {
            append_line("")?;
            append_line("Computed fields")?;
            render_computed_fields(object.object.computed.as_ref(), &computed_columns)?;
        }

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

fn render_object_data(json_data: Option<&Value>, jsonpath: Option<&str>) -> Result<(), AppError> {
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
            let pretty_path = prettify_slice_path(&result.path);
            let value = display_json_value(result.val);
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

    let flattener = Flattener {
        ..Default::default()
    };

    let v = flattener.flatten(json_data);

    if let Value::Object(map) = v {
        let sorted_map: BTreeMap<_, _> = map.into_iter().collect();
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

fn display_json_value(value: &Value) -> String {
    scalar_text(value).unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs::write;

    use hubuum_client::ObjectDataPatchOperation;
    use hubuum_filter::{apply_pipeline, OutputEnvelope, PipeStage, ProjectTerm, SortCast};
    use serde_json::{json, Value};
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        all_computed_value_columns, bounded_auto_data_columns, data_column_display_value,
        data_column_value, display_json_value, explicit_data_columns, first_seen_data_keys,
        object_data_column_label, object_field_summaries, object_list_row,
        object_show_pipeline_value, parse_object_data_patch, ComputedFieldSelection,
        ComputedValueColumn, ComputedValueScope, ObjectList, ObjectListColumns,
        DEFAULT_OBJECT_FIELD_DEPTH,
    };
    use super::{render_object_data, render_object_show_text, should_render_object_data};
    use crate::commands::command_options;
    use crate::config::{init_config, AppConfig};
    use crate::domain::{
        ComputedFieldSet, ObjectShowRecord, RelatedObjectTreeNode, ResolvedObjectRecord,
    };
    use crate::list_query::PagedResult;
    use crate::output::{append_line, reset_output, take_output};
    use crate::tokenizer::CommandTokenizer;

    #[test]
    fn display_json_value_unquotes_strings() {
        assert_eq!(display_json_value(&json!("Entry")), "Entry");
    }

    #[test]
    fn object_data_patch_parses_inline_json() {
        let patch =
            parse_object_data_patch(r#"[{"op":"add","path":"/facts","value":{"os":"Fedora"}}]"#)
                .expect("patch should parse");

        assert!(matches!(
            &patch[0],
            ObjectDataPatchOperation::Add { path, value }
                if path == "/facts" && value == &json!({"os": "Fedora"})
        ));
    }

    #[test]
    fn object_data_patch_reads_at_file_source() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("patch.json");
        write(&path, r#"[{"op":"remove","path":"/stale"}]"#).expect("patch file should be written");

        let patch = parse_object_data_patch(&format!("@{}", path.display()))
            .expect("patch file should parse");

        assert!(matches!(
            &patch[0],
            ObjectDataPatchOperation::Remove { path } if path == "/stale"
        ));
    }

    #[test]
    fn computed_object_reads_default_off_and_accept_scoped_fields_or_all() {
        assert_eq!(
            ComputedFieldSelection::parse(&[]).expect("default should resolve"),
            ComputedFieldSelection::None
        );
        assert_eq!(
            ComputedFieldSelection::parse(&["all".to_string()]).expect("all should resolve"),
            ComputedFieldSelection::All
        );
        assert_eq!(
            ComputedFieldSelection::parse(&["none".to_string()]).expect("none should resolve"),
            ComputedFieldSelection::None
        );
        assert_eq!(
            ComputedFieldSelection::parse(&["S:load,P:label".to_string(), "S:load".to_string(),])
                .expect("scoped fields should resolve"),
            ComputedFieldSelection::Fields(vec![
                ComputedValueColumn::new(ComputedValueScope::Shared, "load"),
                ComputedValueColumn::new(ComputedValueScope::Personal, "label"),
            ])
        );
        assert!(ComputedFieldSelection::parse(&["all".to_string(), "S:load".to_string()]).is_err());
        assert!(
            ComputedFieldSelection::parse(&["none".to_string(), "S:load".to_string()]).is_err()
        );
        assert!(ComputedFieldSelection::parse(&["load".to_string()]).is_err());
    }

    #[test]
    #[serial]
    fn configured_computed_fields_apply_per_class_and_explicit_values_override_them() {
        let mut config = AppConfig::default();
        config.output.object_class_computed_fields.insert(
            "Hosts".to_string(),
            ComputedFieldSet::from_values(&["S:load,P:note".to_string()])
                .expect("configured fields should parse"),
        );
        init_config(config).expect("config should initialize");

        assert_eq!(
            ComputedFieldSelection::resolve(&[], Some("Hosts"))
                .expect("configured fields should resolve"),
            ComputedFieldSelection::Fields(vec![
                ComputedValueColumn::new(ComputedValueScope::Shared, "load"),
                ComputedValueColumn::new(ComputedValueScope::Personal, "note"),
            ])
        );
        assert_eq!(
            ComputedFieldSelection::resolve(&["none".to_string()], Some("Hosts"))
                .expect("none should override configured fields"),
            ComputedFieldSelection::None
        );
        assert_eq!(
            ComputedFieldSelection::resolve(&["S:other".to_string()], Some("Hosts"))
                .expect("explicit fields should override configured fields"),
            ComputedFieldSelection::Fields(vec![ComputedValueColumn::new(
                ComputedValueScope::Shared,
                "other",
            )])
        );
        assert_eq!(
            ComputedFieldSelection::resolve(&[], Some("Other"))
                .expect("unconfigured class should resolve"),
            ComputedFieldSelection::None
        );

        init_config(AppConfig::default()).expect("default config should be restored");
    }

    #[test]
    fn object_list_parses_repeatable_computed_selections() {
        let tokens = CommandTokenizer::new(
            "object list --class Hosts --computed S:load --computed P:note",
            "list",
            &command_options::<ObjectList>(),
        )
        .expect("command should tokenize");

        let query = ObjectList::parse_tokens(&tokens).expect("command should parse");

        assert_eq!(query.computed, vec!["S:load", "P:note"]);
    }

    #[test]
    fn computed_field_selection_projects_only_requested_scope_values() {
        let selection =
            ComputedFieldSelection::parse(&["S:load".to_string(), "P:label".to_string()])
                .expect("selection should parse");
        let projected = selection
            .project_computed(Some(&json!({
                "shared": {
                    "revision": 4,
                    "values": {"load": 1.5, "hidden": 99},
                    "errors": {"broken": {"message": "nope"}}
                },
                "personal": {
                    "values": {"label": "mine", "hidden": true},
                    "errors": {}
                }
            })))
            .expect("computed envelope should remain");

        assert_eq!(projected["shared"]["revision"], json!(4));
        assert_eq!(projected["shared"]["values"], json!({"load": 1.5}));
        assert_eq!(projected["shared"]["errors"], json!({}));
        assert_eq!(projected["personal"]["values"], json!({"label": "mine"}));
    }

    #[test]
    fn unrequested_computed_values_are_removed_from_projected_records() {
        let mut object = test_object(1, json!({}));
        object.computed = Some(json!({
            "shared": {"values": {"load": 1.5}, "errors": {}}
        }));

        let projected = ComputedFieldSelection::None.project_record(&object);

        assert!(projected.computed.is_none());
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
            returned_count: 2,
            total_count: None,
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
            computed_columns: Vec::new(),
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
            computed_columns: Vec::new(),
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
    fn object_list_row_expands_computed_values_into_scoped_columns() {
        let mut object = test_object(1, json!({"load": 1}));
        object.computed = Some(json!({
            "shared": {
                "values": {"average_load": 1.5},
                "errors": {"broken": {"message": "missing input"}}
            },
            "personal": {"values": {"note": "mine"}, "errors": {}}
        }));
        let columns = ObjectListColumns {
            data_keys: Vec::new(),
            compact_base: false,
            computed_columns: all_computed_value_columns(std::slice::from_ref(&object)),
        };
        let displayed = columns.display_columns();
        assert!(displayed.ends_with(&[
            "S:average_load".to_string(),
            "S:broken".to_string(),
            "P:note".to_string(),
        ]));

        let row = object_list_row(&object, &columns)
            .expect("row should render")
            .as_object()
            .cloned()
            .expect("row should be object");

        assert_eq!(row.get("S:average_load"), Some(&json!(1.5)));
        assert_eq!(row.get("S:broken"), Some(&json!("ERROR: missing input")));
        assert_eq!(row.get("P:note"), Some(&json!("mine")));
        assert!(!row.contains_key("Computed"));
    }

    #[test]
    fn scoped_computed_columns_support_semantic_pipe_stages() {
        let mut first = test_object(1, json!({}));
        first.computed = Some(json!({
            "shared": {"values": {"load": 1.5}, "errors": {}},
            "personal": {"values": {"label": "one"}, "errors": {}}
        }));
        let mut second = test_object(2, json!({}));
        second.computed = Some(json!({
            "shared": {"values": {"load": 3.0}, "errors": {}},
            "personal": {"values": {"label": "two"}, "errors": {}}
        }));
        let objects = vec![first, second];
        let columns = ObjectListColumns {
            data_keys: Vec::new(),
            compact_base: false,
            computed_columns: all_computed_value_columns(&objects),
        };
        let rows = objects
            .iter()
            .map(|object| object_list_row(object, &columns).expect("row should render"))
            .collect();

        let output = apply_pipeline(
            OutputEnvelope::rows(rows, columns.display_columns()),
            &[
                PipeStage::Grep("S:load>=2".to_string()),
                PipeStage::Columns(vec![
                    ProjectTerm::keep("Name"),
                    ProjectTerm::keep("S:load"),
                    ProjectTerm::keep("P:label"),
                ]),
                PipeStage::SortColumn {
                    column: "S:load".to_string(),
                    descending: true,
                    cast: SortCast::Number,
                },
            ],
        )
        .expect("scoped fields should work in semantic pipelines");

        assert_eq!(
            output.value,
            json!([{
                "Name": "host-2",
                "S:load": 3.0,
                "P:label": "two"
            }])
        );
    }

    #[test]
    fn object_show_pipeline_exposes_scoped_computed_fields() {
        let mut object = test_object(1, json!({"owner": "ops"}));
        object.computed = Some(json!({
            "shared": {"values": {"load": 1.5}, "errors": {}},
            "personal": {"values": {"label": "mine"}, "errors": {}}
        }));
        let selection = ComputedFieldSelection::Fields(vec![
            ComputedValueColumn::new(ComputedValueScope::Shared, "load"),
            ComputedValueColumn::new(ComputedValueScope::Personal, "label"),
        ]);
        let (value, columns) = object_show_pipeline_value(
            &ObjectShowRecord {
                object,
                related_objects: Vec::new(),
            },
            &selection,
        )
        .expect("show pipeline value should render");

        assert_eq!(value.get("S:load"), Some(&json!(1.5)));
        assert_eq!(value.get("P:label"), Some(&json!("mine")));
        assert_eq!(value.get("related_objects"), Some(&json!([])));
        assert!(columns.contains(&"S:load".to_string()));
        assert!(columns.contains(&"P:label".to_string()));
    }

    #[test]
    fn data_column_value_accepts_raw_and_data_prefixed_paths() {
        let data = json!({"name": "host", "hardware": {"cpu": "M2"}});
        let data = data.as_object().expect("data object");

        assert_eq!(data_column_value(data, "name"), Some(json!("host")));
        assert_eq!(data_column_value(data, "data.name"), Some(json!("host")));
        assert_eq!(
            data_column_value(data, "data.hardware.cpu"),
            Some(json!("M2"))
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
    fn object_list_row_inserts_configured_display_aliases() {
        let mut config = AppConfig::default();
        config.output.object_list_class_aliases.insert(
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
            computed_columns: Vec::new(),
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
            returned_count: 2,
            total_count: None,
        };

        let summaries = object_field_summaries(&page, DEFAULT_OBJECT_FIELD_DEPTH, false);

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
            returned_count: 1,
            total_count: None,
        };

        let summaries = object_field_summaries(&page, DEFAULT_OBJECT_FIELD_DEPTH, false);

        assert!(summaries.contains_key("data.network.interfaces[*].ipv4"));
        assert!(!summaries.contains_key("data.network"));
        assert!(!summaries.contains_key("data.network.interfaces"));
    }

    #[test]
    fn object_field_summaries_can_include_containers() {
        let page = PagedResult {
            items: vec![test_object(1, json!({"hardware": {"cpu": "M2"}}))],
            next_cursor: None,
            returned_count: 1,
            total_count: None,
        };

        let summaries = object_field_summaries(&page, DEFAULT_OBJECT_FIELD_DEPTH, true);

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
                    returned_count: 2,
                    total_count: None,
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
                collection: "default".to_string(),
                class: "Contacts".to_string(),
                data: Some(json!({"email": "a@example.com"})),
                computed: None,
                created_at: "2024-01-01 00:00:00".to_string(),
                updated_at: "2024-01-01 00:00:00".to_string(),
            },
            related_objects: vec![RelatedObjectTreeNode {
                id: 2,
                class: "Jacks".to_string(),
                name: "BL14=521.A7-UD7056".to_string(),
                collection: "default".to_string(),
                depth: 1,
                children: vec![RelatedObjectTreeNode {
                    id: 3,
                    class: "Rooms".to_string(),
                    name: "B701".to_string(),
                    collection: "default".to_string(),
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

    fn test_object(id: i32, data: Value) -> ResolvedObjectRecord {
        ResolvedObjectRecord {
            id,
            name: format!("host-{id}"),
            description: String::new(),
            collection: "Math".to_string(),
            class: "Hosts".to_string(),
            data: Some(data),
            computed: None,
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
        query.name = option_or_pos(query.name, tokens, 1, "name")?;

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
        help = "Sort clause: 'field asc|desc', including S:key or P:key",
        nargs = 2,
        autocomplete = "object_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
    #[option(
        long = "data-columns",
        help = "Object data columns: auto, preview, all, or comma-separated keys",
        autocomplete = "object_data_columns"
    )]
    pub data_columns: Option<String>,
    #[option(
        long = "computed",
        help = "Computed field to show: S:key, P:key, all, or none (repeatable)",
        nargs = 1,
        autocomplete = "computed_fields"
    )]
    pub computed: Vec<String>,
}

impl CliCommand for ObjectList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query: ObjectList = Self::parse_tokens(tokens)?;
        let computed_selection =
            ComputedFieldSelection::resolve(&query.computed, query.class.as_deref())?;
        let class_filter = query.class.clone();
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
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
        let include_computed = computed_selection.requests_values()
            || list_query
                .sorts
                .iter()
                .any(|sort| sort.field.starts_with("S:") || sort.field.starts_with("P:"));
        let objects = services
            .gateway()
            .list_objects(&list_query, include_computed)?;
        render_object_list_page(
            services,
            tokens,
            &objects,
            class_filter.as_deref(),
            query.data_columns.as_deref(),
            &computed_selection,
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
        help = "Maximum objects to sample (default: 100; server maximum: 250)"
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
        let sample_limit =
            normalize_server_page_size(query.limit)?.unwrap_or(DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT);
        let list_query = build_list_query(
            &[],
            &[],
            Some(sample_limit),
            None,
            false,
            [equals_clause("class", query.class.clone())],
        )?;
        let objects = services.gateway().list_objects(&list_query, false)?;
        render_object_fields(
            &objects,
            sample_limit,
            query.depth.unwrap_or(DEFAULT_OBJECT_FIELD_DEPTH),
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
            json!({
                "Field": field,
                "Count": summary.count,
                "Types": summary.types.into_iter().collect::<Vec<_>>().join(","),
                "Example": summary.example.unwrap_or_default(),
                "Sample": objects.returned_count,
                "Limit": sample_limit,
            })
        })
        .collect::<Vec<_>>();
    set_semantic_output(OutputEnvelope::rows(
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
    let mut summaries: BTreeMap<String, FieldSummary> = BTreeMap::new();
    visit_observed_data_fields(
        objects
            .items
            .iter()
            .filter_map(|object| object.data.as_ref()),
        depth,
        |path, value| {
            let is_container = matches!(value, Value::Object(_) | Value::Array(_));
            if is_container && !include_containers {
                return;
            }

            let summary = summaries.entry(path.display().to_string()).or_default();
            summary.count += 1;
            summary.types.insert(json_type_name(value));
            summary
                .example
                .get_or_insert_with(|| data_preview(Some(value)));
        },
    );
    summaries
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn render_object_list_page(
    services: &AppServices,
    tokens: &CommandTokenizer,
    objects: &PagedResult<ResolvedObjectRecord>,
    class_filter: Option<&str>,
    data_columns: Option<&str>,
    computed_selection: &ComputedFieldSelection,
) -> Result<(), AppError> {
    match (desired_format(tokens), has_pipeline()?) {
        (OutputFormat::Json, false) => render_paged_result(
            tokens,
            &computed_selection.project_page(objects),
            OutputFormat::Json,
        ),
        (OutputFormat::Json, true) | (OutputFormat::Text, _) => {
            let columns = object_list_columns(
                services,
                objects,
                class_filter,
                data_columns,
                computed_selection,
            )?;
            let rows = objects
                .items
                .iter()
                .map(|object| object_list_row(object, &columns))
                .collect::<Result<Vec<_>, AppError>>()?;
            set_semantic_output(OutputEnvelope::rows(rows, columns.display_columns()))?;
            append_paging_footer(tokens, objects)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectListColumns {
    data_keys: Vec<String>,
    compact_base: bool,
    computed_columns: Vec<ComputedValueColumn>,
}

impl ObjectListColumns {
    fn display_columns(&self) -> Vec<String> {
        let mut columns = vec!["id".to_string(), "Name".to_string()];
        if !self.compact_base {
            columns.extend([
                "Description".to_string(),
                "Collection".to_string(),
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
        columns.extend(self.computed_columns.iter().map(ComputedValueColumn::label));
        columns
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ComputedValueScope {
    Shared,
    Personal,
}

impl ComputedValueScope {
    fn name(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Personal => "personal",
        }
    }

    fn column_prefix(self) -> &'static str {
        match self {
            Self::Shared => "S",
            Self::Personal => "P",
        }
    }

    fn parse(prefix: &str) -> Option<Self> {
        match prefix {
            "S" => Some(Self::Shared),
            "P" => Some(Self::Personal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ComputedValueColumn {
    scope: ComputedValueScope,
    key: String,
}

impl ComputedValueColumn {
    fn new(scope: ComputedValueScope, key: impl Into<String>) -> Self {
        Self {
            scope,
            key: key.into(),
        }
    }

    fn label(&self) -> String {
        format!("{}:{}", self.scope.column_prefix(), self.key)
    }

    fn from_selector(selector: &ComputedFieldSelector) -> Option<Self> {
        let (prefix, key) = selector.scoped_parts()?;
        Some(Self::new(ComputedValueScope::parse(prefix)?, key))
    }

    fn semantic_value(&self, computed: &Value) -> Option<Value> {
        let scope = computed.get(self.scope.name())?;
        if let Some(value) = scope.get("values").and_then(|values| values.get(&self.key)) {
            return Some(value.clone());
        }

        scope
            .get("errors")
            .and_then(|errors| errors.get(&self.key))
            .map(computed_error_display)
            .map(Value::String)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ComputedFieldSelection {
    None,
    All,
    Fields(Vec<ComputedValueColumn>),
}

impl ComputedFieldSelection {
    fn parse(values: &[String]) -> Result<Self, AppError> {
        let fields = ComputedFieldSet::from_values(values).map_err(AppError::InvalidOption)?;
        Ok(Self::from_field_set(&fields))
    }

    fn resolve(values: &[String], class_name: Option<&str>) -> Result<Self, AppError> {
        if !values.is_empty() {
            return Self::parse(values);
        }
        let config = get_config();
        let fields = class_name
            .and_then(|class_name| config.output.object_class_computed_fields.get(class_name));
        Ok(fields.map_or(Self::None, Self::from_field_set))
    }

    fn from_field_set(fields: &ComputedFieldSet) -> Self {
        if fields.is_empty() {
            return Self::None;
        }
        if fields.is_all() {
            return Self::All;
        }
        Self::Fields(
            fields
                .selectors()
                .iter()
                .filter_map(ComputedValueColumn::from_selector)
                .collect(),
        )
    }

    fn requests_values(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn columns(&self, objects: &[ResolvedObjectRecord]) -> Vec<ComputedValueColumn> {
        match self {
            Self::None => Vec::new(),
            Self::All => all_computed_value_columns(objects),
            Self::Fields(fields) => fields.clone(),
        }
    }

    fn project_computed(&self, computed: Option<&Value>) -> Option<Value> {
        let computed = computed?;
        match self {
            Self::None => None,
            Self::All => Some(computed.clone()),
            Self::Fields(fields) => {
                let mut projected = computed.clone();
                let Some(envelope) = projected.as_object_mut() else {
                    return Some(projected);
                };
                for scope in [ComputedValueScope::Shared, ComputedValueScope::Personal] {
                    let selected = fields
                        .iter()
                        .filter(|field| field.scope == scope)
                        .map(|field| field.key.as_str())
                        .collect::<HashSet<_>>();
                    if selected.is_empty() {
                        envelope.remove(scope.name());
                        continue;
                    }
                    let Some(scope_value) = envelope.get_mut(scope.name()) else {
                        continue;
                    };
                    for section in ["values", "errors"] {
                        if let Some(values) =
                            scope_value.get_mut(section).and_then(Value::as_object_mut)
                        {
                            values.retain(|key, _| selected.contains(key.as_str()));
                        }
                    }
                }
                Some(projected)
            }
        }
    }

    fn project_record(&self, object: &ResolvedObjectRecord) -> ResolvedObjectRecord {
        let mut projected = object.clone();
        projected.computed = self.project_computed(object.computed.as_ref());
        projected
    }

    fn project_object_show(&self, object: &ObjectShowRecord) -> ObjectShowRecord {
        let mut projected = object.clone();
        projected.object = self.project_record(&object.object);
        projected
    }

    fn project_page(
        &self,
        objects: &PagedResult<ResolvedObjectRecord>,
    ) -> PagedResult<ResolvedObjectRecord> {
        let mut projected = objects.clone();
        projected.items = objects
            .items
            .iter()
            .map(|object| self.project_record(object))
            .collect();
        projected
    }
}

fn all_computed_value_columns(objects: &[ResolvedObjectRecord]) -> Vec<ComputedValueColumn> {
    let mut columns = BTreeSet::new();
    for object in objects {
        let Some(computed) = object.computed.as_ref() else {
            continue;
        };
        for scope in [ComputedValueScope::Shared, ComputedValueScope::Personal] {
            let Some(scope_value) = computed.get(scope.name()) else {
                continue;
            };
            for section in ["values", "errors"] {
                if let Some(fields) = scope_value.get(section).and_then(Value::as_object) {
                    columns.extend(
                        fields
                            .keys()
                            .map(|key| ComputedValueColumn::new(scope, key)),
                    );
                }
            }
        }
    }
    columns.into_iter().collect()
}

fn computed_error_display(error: &Value) -> String {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| display_json_value(error));
    format!("ERROR: {message}")
}

fn object_list_columns(
    services: &AppServices,
    objects: &PagedResult<ResolvedObjectRecord>,
    class_filter: Option<&str>,
    requested: Option<&str>,
    computed_selection: &ComputedFieldSelection,
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
        computed_columns: computed_selection.columns(&objects.items),
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
        .chain(once("id".len()))
        .max()
        .unwrap_or("id".len());
    let name_width = objects
        .items
        .iter()
        .map(|object| object.name.len())
        .chain(once("Name".len()))
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
                .and_then(Value::as_object)
                .and_then(|data| data.get(key))
        })
        .map(|value| data_preview(Some(value)).len())
        .chain(once(object_data_column_header(key).len()))
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

fn schema_property_keys(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn first_seen_data_keys(objects: &PagedResult<ResolvedObjectRecord>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for object in &objects.items {
        if let Some(data) = object.data.as_ref().and_then(Value::as_object) {
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
) -> Result<Value, AppError> {
    let mut row = match to_value(object)? {
        Value::Object(object) => object,
        value => {
            let mut object = Map::new();
            object.insert("value".to_string(), value);
            object
        }
    };

    row.insert("id".to_string(), Value::String(object.id.to_string()));
    row.insert("Name".to_string(), Value::String(object.name.clone()));
    row.insert(
        "Description".to_string(),
        Value::String(object.description.clone()),
    );
    row.insert(
        "Collection".to_string(),
        Value::String(object.collection.clone()),
    );
    row.insert("Class".to_string(), Value::String(object.class.clone()));
    let data_object = object.data.as_ref().and_then(Value::as_object);
    if let Some(data) = data_object {
        insert_display_aliases(&mut row, &object.class, data);
    }

    if columns.data_keys.is_empty() {
        row.insert(
            "Data".to_string(),
            Value::String(data_preview(object.data.as_ref())),
        );
    } else if let Some(data) = data_object {
        for key in &columns.data_keys {
            if let Some(value) = data_or_alias_display_value(&object.class, data, key) {
                row.insert(object_data_column_label(key), Value::String(value));
            }
        }
    }
    row.insert(
        "Created".to_string(),
        Value::String(object.created_at.clone()),
    );
    row.insert(
        "Updated".to_string(),
        Value::String(object.updated_at.clone()),
    );
    if let Some(computed) = object.computed.as_ref() {
        for column in &columns.computed_columns {
            if let Some(value) = column.semantic_value(computed) {
                row.insert(column.label(), value);
            }
        }
    }

    Ok(Value::Object(row))
}

fn object_show_pipeline_value(
    object: &ObjectShowRecord,
    computed_selection: &ComputedFieldSelection,
) -> Result<(Value, Vec<String>), AppError> {
    let columns = ObjectListColumns {
        data_keys: Vec::new(),
        compact_base: false,
        computed_columns: computed_selection.columns(std::slice::from_ref(&object.object)),
    };
    let mut value = object_list_row(&object.object, &columns)?;
    if let Some(row) = value.as_object_mut() {
        row.insert(
            "related_objects".to_string(),
            to_value(&object.related_objects)?,
        );
    }
    Ok((value, columns.display_columns()))
}

fn render_computed_fields(
    computed: Option<&Value>,
    columns: &[ComputedValueColumn],
) -> Result<(), AppError> {
    let padding = columns
        .iter()
        .map(|column| column.label().len())
        .max()
        .unwrap_or(15)
        .max(15);
    for column in columns {
        let value = computed
            .and_then(|computed| column.semantic_value(computed))
            .as_ref()
            .map(display_json_value)
            .unwrap_or_default();
        append_key_value(column.label(), value, padding)?;
    }
    Ok(())
}

fn insert_display_aliases(
    row: &mut Map<String, Value>,
    class_name: &str,
    data: &Map<String, Value>,
) {
    if let Some(aliases) = get_config()
        .output
        .object_list_class_aliases
        .get(class_name)
    {
        for (alias, selectors) in aliases {
            if let Some(value) = display_alias_value(data, selectors) {
                row.insert(alias.clone(), Value::String(value));
            }
        }
    }
}

fn object_data_column_label(key: &str) -> String {
    if key.starts_with("data.") {
        return key.to_string();
    }

    match key {
        "id" | "name" | "description" | "collection" | "class" | "created_at" | "updated_at"
        | "Name" | "Description" | "Collection" | "Class" | "Data" | "Created" | "Updated" => {
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
fn data_column_value(data: &Map<String, Value>, key: &str) -> Option<Value> {
    data_column_values(data, key).into_iter().next()
}

fn data_column_display_value(data: &Map<String, Value>, key: &str) -> Option<String> {
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

fn data_or_alias_display_value(
    class_name: &str,
    data: &Map<String, Value>,
    key: &str,
) -> Option<String> {
    data_column_display_value(data, key).or_else(|| {
        get_config()
            .output
            .object_list_class_aliases
            .get(class_name)
            .and_then(|aliases| aliases.get(key))
            .and_then(|selectors| display_alias_value(data, selectors))
    })
}

fn display_alias_value(data: &Map<String, Value>, selectors: &[String]) -> Option<String> {
    selectors
        .iter()
        .find_map(|selector| data_column_display_value(data, selector))
}

fn data_column_values(data: &Map<String, Value>, key: &str) -> Vec<Value> {
    let key = key.strip_prefix("data.").unwrap_or(key);
    let root = Value::Object(data.clone());
    select_values(&root, key).into_iter().cloned().collect()
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
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
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
            let mut json_data = Value::Null;
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
            collection: new.collection,
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
