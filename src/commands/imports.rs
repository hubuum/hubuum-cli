use std::fs::read_to_string;

use cli_command_derive::CommandArgs;
use hubuum_client::{
    ClassKey, CollectionKey, ImportAtomicity, ImportCollisionPolicy, ImportMode,
    ImportPermissionPolicy, ImportRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::from_str;

use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{parse_task_submit_options, run_task_backed};
use super::{build_list_query, option_or_pos, render_list_page, render_task_record, CliCommand};
use crate::autocomplete::{collections, file_paths, import_result_sort};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::services::CompletionContext;
use crate::services::{AppServices, SubmitImportInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["import"],
            catalog_command(
                "submit",
                ImportSubmit::default(),
                CommandDocs {
                    about: Some("Submit an import request"),
                    long_about: Some(
                        "Submit an import request from a local JSON file or HTTP(S) URL. CLI policy flags override the request mode. --collection rewrites the import to reuse an existing collection and removes collection creation/permission entries.",
                    ),
                    examples: Some("--file import.json --collection Math --collision-policy overwrite\n--http https://example.com/import.json --atomicity best_effort"),
                },
            ),
        )
        .add_command(
            &["import"],
            catalog_command(
                "show",
                ImportShow::default(),
                CommandDocs {
                    about: Some("Show import task details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["import"],
            catalog_command(
                "results",
                ImportResults::default(),
                CommandDocs {
                    about: Some("List import results"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ImportSubmit {
    #[option(
        short = "f",
        long = "file",
        help = "Path to import JSON file",
        autocomplete = "file_paths"
    )]
    pub file: Option<String>,
    #[option(
        long = "http",
        help = "HTTP(S) URL to import JSON",
        value_source = true
    )]
    pub http: Option<String>,
    #[option(
        short = "N",
        long = "collection",
        help = "Existing collection to reuse for all import collection references",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(
        long = "atomicity",
        help = "Import atomicity: strict or best_effort",
        autocomplete = "import_atomicity"
    )]
    pub atomicity: Option<ImportAtomicity>,
    #[option(
        long = "collision-policy",
        help = "Import collision policy: abort or overwrite",
        autocomplete = "import_collision_policy"
    )]
    pub collision_policy: Option<ImportCollisionPolicy>,
    #[option(
        long = "permission-policy",
        help = "Import permission policy: abort or continue",
        autocomplete = "import_permission_policy"
    )]
    pub permission_policy: Option<ImportPermissionPolicy>,
    #[option(
        short = "k",
        long = "idempotency-key",
        help = "Optional idempotency key"
    )]
    pub idempotency_key: Option<String>,
    #[option(long = "wait", flag, help = "Wait for task completion")]
    pub wait: bool,
    #[option(long = "timeout", help = "Timeout in seconds when waiting")]
    pub timeout: Option<u64>,
    #[option(long = "poll-interval", help = "Poll interval in seconds when waiting")]
    pub poll_interval: Option<u64>,
}

impl CliCommand for ImportSubmit {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let opts = parse_task_submit_options(tokens)?;
        if let Some(collection) = &query.collection {
            services.gateway().get_collection(collection)?;
        }
        let request = import_request(&query)?;
        let task = services.gateway().submit_import(SubmitImportInput {
            request,
            idempotency_key: query.idempotency_key,
        })?;
        run_task_backed(
            services,
            tokens,
            format!("import {}", task.0.id),
            opts,
            task,
        )
    }
}

fn import_request(query: &ImportSubmit) -> Result<ImportRequest, AppError> {
    let body = match (&query.file, &query.http) {
        (Some(_), Some(_)) => Err(AppError::ParseError(
            "Use either --file or --http, not both".to_string(),
        )),
        (Some(file), None) => read_to_string(file).map_err(AppError::IoError),
        (None, Some(http_body)) => Ok(http_body.clone()),
        (None, None) => Err(AppError::MissingOptions(vec![
            "file".to_string(),
            "http".to_string(),
        ])),
    }?;
    let mut request = from_str::<ImportRequest>(&body)?;
    apply_mode_overrides(&mut request, query);
    if let Some(collection) = &query.collection {
        apply_existing_collection_override(&mut request, collection);
    }
    Ok(request)
}

fn apply_mode_overrides(request: &mut ImportRequest, query: &ImportSubmit) {
    if query.atomicity.is_none()
        && query.collision_policy.is_none()
        && query.permission_policy.is_none()
    {
        return;
    }

    let mode = request.mode.get_or_insert(ImportMode {
        atomicity: None,
        collision_policy: None,
        permission_policy: None,
    });

    if query.atomicity.is_some() {
        mode.atomicity = query.atomicity;
    }
    if query.collision_policy.is_some() {
        mode.collision_policy = query.collision_policy;
    }
    if query.permission_policy.is_some() {
        mode.permission_policy = query.permission_policy;
    }
}

fn apply_existing_collection_override(request: &mut ImportRequest, collection: &str) {
    let collection_key = CollectionKey {
        name: collection.to_string(),
        path: None,
    };

    for class in &mut request.graph.classes {
        class.collection_ref = None;
        class.collection_key = Some(collection_key.clone());
    }
    for object in &mut request.graph.objects {
        if let Some(class_key) = &mut object.class_key {
            rewrite_class_key_collection(class_key, collection_key.clone());
        }
    }
    for relation in &mut request.graph.class_relations {
        for class_key in [&mut relation.from_class_key, &mut relation.to_class_key]
            .into_iter()
            .flatten()
        {
            rewrite_class_key_collection(class_key, collection_key.clone());
        }
    }
    for relation in &mut request.graph.object_relations {
        for object_key in [&mut relation.from_object_key, &mut relation.to_object_key]
            .into_iter()
            .flatten()
        {
            if let Some(class_key) = &mut object_key.class_key {
                rewrite_class_key_collection(class_key, collection_key.clone());
            }
        }
    }
    request.graph.collections.clear();
    request.graph.collection_permissions.clear();
}

fn rewrite_class_key_collection(class_key: &mut ClassKey, collection_key: CollectionKey) {
    class_key.collection_ref = None;
    class_key.collection_key = Some(collection_key);
}

fn import_atomicity(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_import_policy(prefix, &["strict", "best_effort"])
}

fn import_collision_policy(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_import_policy(prefix, &["abort", "overwrite"])
}

fn import_permission_policy(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_import_policy(prefix, &["abort", "continue"])
}

fn complete_import_policy(prefix: &str, values: &[&str]) -> Vec<String> {
    values
        .iter()
        .copied()
        .filter(|value| value.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ImportShow {
    #[option(short = "i", long = "id", help = "Import task ID")]
    pub id: Option<i32>,
}

impl CliCommand for ImportShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let task = services.gateway().import_task(
            query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
        )?;

        render_task_record(tokens, &task)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ImportResults {
    #[option(short = "i", long = "id", help = "Import task ID")]
    pub id: Option<i32>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "import_result_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for ImportResults {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let list_query = build_list_query(&[], &query.sort_clauses, query.limit, query.cursor, [])?;
        let results = services.gateway().import_results(
            query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
            &list_query,
        )?;
        render_list_page(tokens, &results)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::write;

    use super::{import_request, ImportSubmit};
    use crate::commands::command_options;
    use crate::errors::AppError;
    use crate::tokenizer::CommandTokenizer;
    use hubuum_client::{ImportAtomicity, ImportCollisionPolicy, ImportPermissionPolicy};
    use tempfile::tempdir;

    const EMPTY_IMPORT: &str = r#"{"version":1,"dry_run":null,"mode":null,"graph":{}}"#;

    #[test]
    fn import_request_reads_file_source() {
        let dir = tempdir().expect("temp dir should be created");
        let path = dir.path().join("import.json");
        write(&path, EMPTY_IMPORT).expect("file should be written");

        let query = ImportSubmit {
            file: Some(path.to_string_lossy().to_string()),
            ..ImportSubmit::default()
        };

        assert_eq!(import_request(&query).expect("file should load").version, 1);
    }

    #[test]
    fn import_request_accepts_http_body_source() {
        let query = ImportSubmit {
            http: Some(EMPTY_IMPORT.to_string()),
            ..ImportSubmit::default()
        };

        assert_eq!(
            import_request(&query)
                .expect("http body should be used")
                .version,
            1
        );
    }

    #[test]
    fn import_request_rejects_missing_or_multiple_sources() {
        assert!(matches!(
            import_request(&ImportSubmit::default()),
            Err(AppError::MissingOptions(options)) if options == vec!["file", "http"]
        ));

        let query = ImportSubmit {
            file: Some("import.json".to_string()),
            http: Some(EMPTY_IMPORT.to_string()),
            ..ImportSubmit::default()
        };
        assert!(matches!(
            import_request(&query),
            Err(AppError::ParseError(message)) if message.contains("either --file or --http")
        ));
    }

    #[test]
    fn import_submit_parses_policy_and_collection_options() {
        let tokens = CommandTokenizer::new(
            "import submit --file payload.json --collection Math --atomicity best_effort --collision-policy overwrite --permission-policy continue",
            "submit",
            &command_options::<ImportSubmit>(),
        )
        .expect("tokens should parse");

        let query = ImportSubmit::parse_tokens(&tokens).expect("query should parse");
        assert_eq!(query.collection.as_deref(), Some("Math"));
        assert_eq!(query.atomicity, Some(ImportAtomicity::BestEffort));
        assert_eq!(
            query.collision_policy,
            Some(ImportCollisionPolicy::Overwrite)
        );
        assert_eq!(
            query.permission_policy,
            Some(ImportPermissionPolicy::Continue)
        );
    }

    #[test]
    fn import_request_applies_policy_overrides() {
        let query = ImportSubmit {
            http: Some(EMPTY_IMPORT.to_string()),
            atomicity: Some(ImportAtomicity::BestEffort),
            collision_policy: Some(ImportCollisionPolicy::Overwrite),
            permission_policy: Some(ImportPermissionPolicy::Continue),
            ..ImportSubmit::default()
        };

        let request = import_request(&query).expect("request should parse");
        let mode = request.mode.expect("mode should be set");
        assert_eq!(mode.atomicity, Some(ImportAtomicity::BestEffort));
        assert_eq!(
            mode.collision_policy,
            Some(ImportCollisionPolicy::Overwrite)
        );
        assert_eq!(
            mode.permission_policy,
            Some(ImportPermissionPolicy::Continue)
        );
    }

    #[test]
    fn import_request_rewrites_to_existing_collection_override() {
        let body = r#"{
            "version": 1,
            "dry_run": null,
            "mode": null,
            "graph": {
                "collections": [
                    {
                        "ref": "ns:math",
                        "name": "Math",
                        "description": "Should not be submitted"
                    }
                ],
                "classes": [
                    {
                        "ref": "host-class",
                        "name": "Hosts",
                        "description": "Hosts",
                        "json_schema": null,
                        "validate_schema": null,
                        "collection_ref": "ns:math",
                        "collection_key": null
                    }
                ],
                "objects": [
                    {
                        "ref": null,
                        "name": "host-1",
                        "description": "host-1",
                        "data": {},
                        "class_ref": null,
                        "class_key": {
                            "name": "Hosts",
                            "collection_ref": "ns:math",
                            "collection_key": null
                        }
                    }
                ],
                "collection_permissions": [
                    {
                        "ref": null,
                        "collection_ref": "ns:math",
                        "collection_key": null,
                        "group_key": { "groupname": "admins" },
                        "permissions": [],
                        "replace_existing": false
                    }
                ]
            }
        }"#;
        let query = ImportSubmit {
            http: Some(body.to_string()),
            collection: Some("Math".to_string()),
            ..ImportSubmit::default()
        };

        let request = import_request(&query).expect("request should parse");
        assert!(request.graph.collections.is_empty());
        assert!(request.graph.collection_permissions.is_empty());
        assert_eq!(request.graph.classes[0].collection_ref, None);
        assert_eq!(
            request.graph.classes[0]
                .collection_key
                .as_ref()
                .map(|key| key.name.as_str()),
            Some("Math")
        );
        assert_eq!(
            request.graph.objects[0]
                .class_key
                .as_ref()
                .and_then(|key| key.collection_ref.as_ref()),
            None
        );
        assert_eq!(
            request.graph.objects[0]
                .class_key
                .as_ref()
                .and_then(|key| key.collection_key.as_ref())
                .map(|key| key.name.as_str()),
            Some("Math")
        );
    }
}
