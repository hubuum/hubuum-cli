use std::collections::{BTreeMap, BTreeSet};

use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::builder::{
    catalog_command, deprecated_catalog_command, CommandDeprecation, CommandDocs,
};
use super::{normalize_server_page_size, required_option_or_pos, CliCommand};
use crate::autocomplete::classes;
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::diagnostics::stage_sync;
use crate::domain::{
    visit_observed_data_fields, ObjectFieldValues, ResourceName, DEFAULT_OBJECT_FIELD_DEPTH,
    DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT,
};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::data_preview;
use crate::list_query::ServerPageSize;
use crate::output::set_semantic_output;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["class"],
            catalog_command(
                "fields",
                ClassFields::default(),
                CommandDocs {
                    about: Some("Inspect available fields for a class"),
                    long_about: Some(
                        "Sample objects in a class and list observed data paths plus enabled shared and personal computed selectors. Output includes each field's source, observed value types, counts, and examples.",
                    ),
                    examples: Some("--name Hosts --limit 100"),
                },
            ),
        )
        .add_command(
            &["object"],
            deprecated_catalog_command(
                "fields",
                ObjectFields::default(),
                CommandDocs {
                    about: Some("Inspect available fields for a class"),
                    long_about: Some(
                        "Sample objects in a class and list observed data paths plus enabled shared and personal computed selectors.",
                    ),
                    examples: Some("--class Hosts --limit 100"),
                },
                CommandDeprecation::renamed(
                    &["class", "fields"],
                    &[("--class", "--name"), ("-c", "--name")],
                ),
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
struct ClassFields {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class to inspect",
        autocomplete = "classes"
    )]
    name: Option<String>,
    #[option(
        long = "limit",
        help = "Maximum objects to sample (default: 100; server maximum: 250)"
    )]
    limit: Option<usize>,
    #[option(
        long = "depth",
        help = "Maximum data path depth to inspect (default: 6)"
    )]
    depth: Option<usize>,
    #[option(
        long = "containers",
        help = "Include object and array container paths",
        flag = true
    )]
    containers: Option<bool>,
}

impl CliCommand for ClassFields {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class_name = required_option_or_pos(query.name, tokens, 0, "name")?;
        inspect_class_fields(
            services,
            &class_name,
            query.limit,
            query.depth,
            query.containers,
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
struct ObjectFields {
    #[option(
        short = "c",
        long = "class",
        help = "Name of the class to sample",
        autocomplete = "classes"
    )]
    class: String,
    #[option(
        long = "limit",
        help = "Maximum objects to sample (default: 100; server maximum: 250)"
    )]
    limit: Option<usize>,
    #[option(
        long = "depth",
        help = "Maximum data path depth to inspect (default: 6)"
    )]
    depth: Option<usize>,
    #[option(
        long = "containers",
        help = "Include object and array container paths",
        flag = true
    )]
    containers: Option<bool>,
}

impl CliCommand for ObjectFields {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        inspect_class_fields(
            services,
            &query.class,
            query.limit,
            query.depth,
            query.containers,
        )
    }
}

fn inspect_class_fields(
    services: &AppServices,
    class_name: &str,
    sample_limit: Option<usize>,
    depth: Option<usize>,
    include_containers: Option<bool>,
) -> Result<(), AppError> {
    let sample_limit =
        normalize_server_page_size(sample_limit)?.unwrap_or(DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT);
    let sample = services.gateway().sample_class_fields(
        &ResourceName::new(class_name)?,
        ServerPageSize::from_requested(sample_limit)?,
    )?;
    let computed_fields = sample
        .shared_keys()
        .iter()
        .map(AvailableComputedField::shared)
        .chain(
            sample
                .personal_keys()
                .iter()
                .map(AvailableComputedField::personal),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    render_class_fields(
        sample.objects(),
        &computed_fields,
        sample_limit,
        depth.unwrap_or(DEFAULT_OBJECT_FIELD_DEPTH),
        include_containers.unwrap_or(false),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FieldSource {
    Data,
    SharedComputed,
    PersonalComputed,
}

impl FieldSource {
    const fn label(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::SharedComputed => "shared computed",
            Self::PersonalComputed => "personal computed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ComputedFieldScope {
    Shared,
    Personal,
}

impl ComputedFieldScope {
    const fn selector_prefix(self) -> &'static str {
        match self {
            Self::Shared => "S",
            Self::Personal => "P",
        }
    }

    const fn field_source(self) -> FieldSource {
        match self {
            Self::Shared => FieldSource::SharedComputed,
            Self::Personal => FieldSource::PersonalComputed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AvailableComputedField {
    scope: ComputedFieldScope,
    key: String,
}

impl AvailableComputedField {
    fn shared(key: impl Into<String>) -> Self {
        Self::new(ComputedFieldScope::Shared, key)
    }

    fn personal(key: impl Into<String>) -> Self {
        Self::new(ComputedFieldScope::Personal, key)
    }

    fn new(scope: ComputedFieldScope, key: impl Into<String>) -> Self {
        Self {
            scope,
            key: key.into(),
        }
    }

    fn selector(&self) -> String {
        format!("{}:{}", self.scope.selector_prefix(), self.key)
    }
}

#[derive(Debug)]
struct FieldSummary {
    source: FieldSource,
    count: usize,
    types: BTreeSet<&'static str>,
    example: Option<String>,
}

impl FieldSummary {
    fn new(source: FieldSource) -> Self {
        Self {
            source,
            count: 0,
            types: BTreeSet::new(),
            example: None,
        }
    }

    fn observe(&mut self, value: &Value) {
        self.count += 1;
        self.types.insert(json_type_name(value));
        self.example
            .get_or_insert_with(|| data_preview(Some(value)));
    }
}

fn render_class_fields(
    objects: &[ObjectFieldValues],
    computed_fields: &[AvailableComputedField],
    sample_limit: usize,
    depth: usize,
    include_containers: bool,
) -> Result<(), AppError> {
    let summaries = stage_sync("summarize_fields", || {
        class_field_summaries(objects, computed_fields, depth, include_containers)
    });
    let rows = stage_sync("prepare_field_output", || {
        let mut summaries = summaries.into_iter().collect::<Vec<_>>();
        summaries.sort_by(|(left_field, left), (right_field, right)| {
            left.source
                .cmp(&right.source)
                .then_with(|| left_field.cmp(right_field))
        });
        summaries
            .into_iter()
            .map(|(field, summary)| {
                json!({
                    "Field": field,
                    "Source": summary.source.label(),
                    "Count": summary.count,
                    "Types": summary.types.into_iter().collect::<Vec<_>>().join(","),
                    "Example": summary.example.unwrap_or_default(),
                    "Sample": objects.len(),
                    "Limit": sample_limit,
                })
            })
            .collect::<Vec<_>>()
    });
    set_semantic_output(OutputEnvelope::rows(
        rows,
        vec![
            "Field".to_string(),
            "Source".to_string(),
            "Count".to_string(),
            "Types".to_string(),
            "Example".to_string(),
            "Sample".to_string(),
            "Limit".to_string(),
        ],
    ))?;
    Ok(())
}

fn class_field_summaries(
    objects: &[ObjectFieldValues],
    computed_fields: &[AvailableComputedField],
    depth: usize,
    include_containers: bool,
) -> BTreeMap<String, FieldSummary> {
    let mut summaries: BTreeMap<String, FieldSummary> = BTreeMap::new();
    for field in computed_fields {
        summaries
            .entry(field.selector())
            .or_insert_with(|| FieldSummary::new(field.scope.field_source()));
    }
    visit_observed_data_fields(
        objects.iter().filter_map(ObjectFieldValues::data),
        depth,
        |path, value| {
            let is_container = matches!(value, Value::Object(_) | Value::Array(_));
            if is_container && !include_containers {
                return;
            }

            summaries
                .entry(path.display().to_string())
                .or_insert_with(|| FieldSummary::new(FieldSource::Data))
                .observe(value);
        },
    );
    for object in objects {
        for (scope, values) in [
            (ComputedFieldScope::Shared, object.shared()),
            (ComputedFieldScope::Personal, object.personal()),
        ] {
            for (key, value) in values {
                let field = AvailableComputedField::new(scope, key).selector();
                if let Some(summary) = summaries.get_mut(&field) {
                    summary.observe(value);
                }
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use hubuum_client::ComputedObject;
    use serde_json::{from_value, json, Value};

    use super::{
        class_field_summaries, AvailableComputedField, FieldSource, DEFAULT_OBJECT_FIELD_DEPTH,
    };
    use crate::domain::ObjectFieldValues;

    #[test]
    fn field_summaries_collect_nested_data_paths() {
        let page = page(vec![
            test_object(1, json!({"contact": "Entry", "hardware": {"cpu": "M2"}})),
            test_object(2, json!({"contact": "Dell", "hardware": {"cpu": "i3"}})),
        ]);

        let summaries = class_field_summaries(&page, &[], DEFAULT_OBJECT_FIELD_DEPTH, false);

        assert_eq!(
            summaries.get("data.contact").map(|summary| summary.count),
            Some(2)
        );
        assert_eq!(
            summaries.get("data.contact").map(|summary| summary.source),
            Some(FieldSource::Data)
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
    fn field_summaries_expand_array_item_paths() {
        let page = page(vec![test_object(
            1,
            json!({"network": {"interfaces": [{"ipv4": "127.0.0.1"}]}}),
        )]);

        let summaries = class_field_summaries(&page, &[], DEFAULT_OBJECT_FIELD_DEPTH, false);

        assert!(summaries.contains_key("data.network.interfaces[*].ipv4"));
        assert!(!summaries.contains_key("data.network"));
        assert!(!summaries.contains_key("data.network.interfaces"));
    }

    #[test]
    fn field_summaries_can_include_containers() {
        let page = page(vec![test_object(1, json!({"hardware": {"cpu": "M2"}}))]);

        let summaries = class_field_summaries(&page, &[], DEFAULT_OBJECT_FIELD_DEPTH, true);

        assert!(summaries.contains_key("data.hardware"));
        assert!(summaries.contains_key("data.hardware.cpu"));
    }

    #[test]
    fn field_summaries_include_computed_selectors_and_observed_values() {
        let mut first = test_object(1, json!({"contact": "Entry"}));
        first.computed = from_value(json!({
            "shared": {"revision": 1, "materialization_stale": false, "values": {"load": 1.5}, "errors": {}},
            "personal": {"values": {"note": "mine"}, "errors": {}}
        })).unwrap();
        let mut second = test_object(2, json!({"contact": "Dell"}));
        second.computed = from_value(json!({
            "shared": {"revision": 1, "materialization_stale": false, "values": {"load": 3}, "errors": {}},
            "personal": {
                "values": {},
                "errors": {"note": {"code": "invalid", "message": "not available"}}
            }
        })).unwrap();
        let page = page(vec![first, second]);
        let fields = vec![
            AvailableComputedField::shared("load"),
            AvailableComputedField::shared("empty"),
            AvailableComputedField::personal("note"),
        ];

        let summaries = class_field_summaries(&page, &fields, DEFAULT_OBJECT_FIELD_DEPTH, false);

        let load = summaries.get("S:load").expect("shared field should exist");
        assert_eq!(load.source, FieldSource::SharedComputed);
        assert_eq!(load.count, 2);
        assert_eq!(load.types.iter().copied().collect::<Vec<_>>(), ["number"]);
        assert_eq!(load.example.as_deref(), Some("1.5"));

        let note = summaries
            .get("P:note")
            .expect("personal field should exist");
        assert_eq!(note.source, FieldSource::PersonalComputed);
        assert_eq!(note.count, 1);
        assert_eq!(note.types.iter().copied().collect::<Vec<_>>(), ["string"]);
        assert_eq!(note.example.as_deref(), Some("mine"));

        let empty = summaries
            .get("S:empty")
            .expect("definition without an observed value should still exist");
        assert_eq!(empty.count, 0);
        assert!(empty.types.is_empty());
        assert!(empty.example.is_none());
    }

    fn page(items: Vec<ComputedObject>) -> Vec<ObjectFieldValues> {
        items.into_iter().map(ObjectFieldValues::from).collect()
    }

    fn test_object(id: i32, data: Value) -> ComputedObject {
        ComputedObject {
            object: from_value(json!({
                "id": id,
                "name": format!("host-{id}"),
                "description": "",
                "collection_id": 1,
                "hubuum_class_id": 1,
                "revision": 1,
                "data": data,
                "created_at": "2026-07-05T03:44:41Z",
                "updated_at": "2026-07-05T03:44:41Z",
            }))
            .unwrap(),
            computed: Default::default(),
        }
    }
}
