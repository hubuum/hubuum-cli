use cli_command_derive::CommandArgs;
use hubuum_client::{ClassId, ObjectId, StructuredClassSelector};

use super::builder::{catalog_command, CommandDocs};
use super::{render_json_record, CliCommand};
use crate::autocomplete::{classes, objects_from_class};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::append_json;
use crate::output::{add_warning, has_pipeline};
use crate::services::{AppServices, HydrationOptions, ResourceScope};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(&["object"], catalog_command("hydrate", ObjectHydrate::default(), CommandDocs {
        about: Some("Expand visible paths from one object toward a target class"),
        long_about: Some("Returns unique objects, direct relations, and paths. Depth is 1–10 and the limit bounds 1–50 paths, not objects. A truncated response has no continuation cursor; narrow the query or raise --path-limit. Stored relation direction can differ from traversal direction."),
        examples: Some("--class Hosts --name web-01 --target-class Rooms --max-depth 3 --path-limit 20\n--class-id 1 --object-id 2 --target-class-id 3 --output json"),
    }));
}

#[derive(Clone, Default, CommandArgs)]
struct ObjectHydrate {
    #[option(long = "class", help = "Root class name", autocomplete = "classes")]
    class: Option<String>,
    #[option(long = "class-id", help = "Positive root class ID")]
    class_id: Option<ClassId>,
    #[option(
        long = "name",
        help = "Root object name",
        autocomplete = "objects_from_class"
    )]
    name: Option<String>,
    #[option(long = "object-id", help = "Positive root object ID")]
    object_id: Option<ObjectId>,
    #[option(
        long = "target-class",
        help = "Target class name",
        autocomplete = "classes"
    )]
    target_class: Option<String>,
    #[option(long = "target-class-id", help = "Positive target class ID")]
    target_class_id: Option<ClassId>,
    #[option(long = "max-depth", help = "Maximum traversal depth, 1–10 (default 1)")]
    max_depth: Option<u8>,
    #[option(long = "path-limit", help = "Maximum visible paths, 1–50 (default 20)")]
    path_limit: Option<u8>,
}

impl CliCommand for ObjectHydrate {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let root = ResourceScope::class(query.class, query.class_id)?
            .with_object(query.name, query.object_id)?
            .require_object()?;
        let target = match (query.target_class, query.target_class_id) {
            (Some(name), None) if !name.trim().is_empty() => StructuredClassSelector::Name { name },
            (None, Some(id)) if id.get() > 0 => StructuredClassSelector::Id { id },
            _ => {
                return Err(AppError::InvalidOption(
                    "Select exactly one nonblank --target-class or positive --target-class-id"
                        .into(),
                ))
            }
        };
        let options = HydrationOptions::new(target)
            .depth(query.max_depth.unwrap_or(1))?
            .path_limit(query.path_limit.unwrap_or(20))?;
        let record = services.gateway().hydrate_object(root, options)?;
        if record.value["truncated"] == true {
            add_warning("Hydration was truncated; narrow the scope or raise --path-limit (maximum 50). There is no next page.")?;
        }
        if has_pipeline()? {
            append_json(&record)
        } else {
            render_json_record(tokens, &record)
        }
    }
}
