use cli_command_derive::CommandArgs;
use hubuum_client::{ClassFieldInventory, FieldDiscoveryOptions, FieldSampleLimit, FieldSource};
use hubuum_filter::OutputEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::builder::{
    catalog_command, deprecated_catalog_command, CommandDeprecation, CommandDocs,
};
use super::{normalize_server_page_size, required_option_or_pos, CliCommand};
use crate::autocomplete::classes;
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::diagnostics::stage_sync;
use crate::domain::{ResourceName, DEFAULT_OBJECT_FIELD_DEPTH, DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::output::{add_warning, set_semantic_output};
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
        help = "Maximum data path depth to inspect (0-32; default: 6)"
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
        help = "Maximum data path depth to inspect (0-32; default: 6)"
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
    let options = FieldDiscoveryOptions::default()
        .with_sample_limit(FieldSampleLimit::new(sample_limit)?)
        .with_depth(depth.unwrap_or(DEFAULT_OBJECT_FIELD_DEPTH))?
        .with_containers(include_containers.unwrap_or(false));
    let inventory = services
        .gateway()
        .discover_class_fields(&ResourceName::new(class_name)?, &options)?;
    render_class_fields(inventory)
}

fn render_class_fields(inventory: ClassFieldInventory) -> Result<(), AppError> {
    if !inventory.truncation.is_empty() {
        add_warning(format!("Field discovery is a bounded authorized sample ({} objects, limit {}, depth {}); limited by {}. Counts are not class totals.", inventory.sampled_objects, inventory.effective_sample_limit, inventory.max_depth, inventory.truncation.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")))?;
    }
    let rows = stage_sync("prepare_field_output", || {
        inventory.fields.iter().map(|field| {
        let source = match field.source {
            FieldSource::Data => "data", FieldSource::SharedComputed => "shared computed",
            FieldSource::PersonalComputed => "personal computed", _ => "unknown",
        };
        json!({
            "Field": field.field, "Source": source, "Count": field.observation_count,
            "Types": field.types.keys().map(ToString::to_string).collect::<Vec<_>>().join(","),
            "Example": field.example.as_deref().unwrap_or_default(), "Sample": inventory.sampled_objects,
            "Limit": inventory.effective_sample_limit, "Path": field.path, "Objects": field.object_count,
            "TypeCounts": field.types, "Errors": field.error_count, "ExampleTruncated": field.example_truncated,
            "Sampled": inventory.sampled, "Depth": inventory.max_depth, "Containers": inventory.include_containers,
            "Truncation": inventory.truncation,
        })
    }).collect::<Vec<_>>()
    });
    set_semantic_output(OutputEnvelope::rows(
        rows,
        [
            "Field", "Source", "Count", "Types", "Example", "Sample", "Limit", "Objects", "Errors",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    ))?;
    Ok(())
}
