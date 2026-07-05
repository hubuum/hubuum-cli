use cli_command_derive::CommandArgs;
use hubuum_client::{
    ClassKey, ImportAtomicity, ImportCollisionPolicy, ImportMode, ImportPermissionPolicy,
    ImportRequest, NamespaceKey,
};
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{parse_task_submit_options, run_task_backed};
use super::{build_list_query, desired_format, render_list_page, CliCommand};
use crate::autocomplete::{file_paths, import_result_sort, namespaces};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
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
                        "Submit an import request from a local JSON file or HTTP(S) URL. CLI policy flags override the request mode. --namespace reuses an existing namespace for classes and class keys that do not already specify one.",
                    ),
                    examples: Some("--file import.json --namespace Math --collision-policy overwrite\n--http https://example.com/import.json --atomicity best_effort"),
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

trait GetTaskId {
    fn task_id(&self) -> Option<i32>;
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
        long = "namespace",
        help = "Existing namespace to reuse when import entries do not specify a namespace",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
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
        if let Some(namespace) = &query.namespace {
            services.gateway().get_namespace(namespace)?;
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
        (Some(file), None) => std::fs::read_to_string(file).map_err(AppError::IoError),
        (None, Some(http_body)) => Ok(http_body.clone()),
        (None, None) => Err(AppError::MissingOptions(vec![
            "file".to_string(),
            "http".to_string(),
        ])),
    }?;
    let mut request = serde_json::from_str::<ImportRequest>(&body)?;
    apply_mode_overrides(&mut request, query);
    if let Some(namespace) = &query.namespace {
        apply_default_namespace(&mut request, namespace);
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

fn apply_default_namespace(request: &mut ImportRequest, namespace: &str) {
    let namespace_key = NamespaceKey {
        name: namespace.to_string(),
    };

    for class in &mut request.graph.classes {
        apply_namespace_to_ref_or_key(
            &mut class.namespace_ref,
            &mut class.namespace_key,
            namespace_key.clone(),
        );
    }
    for object in &mut request.graph.objects {
        if let Some(class_key) = &mut object.class_key {
            apply_namespace_to_class_key(class_key, namespace_key.clone());
        }
    }
    for relation in &mut request.graph.class_relations {
        for class_key in [&mut relation.from_class_key, &mut relation.to_class_key]
            .into_iter()
            .flatten()
        {
            apply_namespace_to_class_key(class_key, namespace_key.clone());
        }
    }
    for relation in &mut request.graph.object_relations {
        for object_key in [&mut relation.from_object_key, &mut relation.to_object_key]
            .into_iter()
            .flatten()
        {
            if let Some(class_key) = &mut object_key.class_key {
                apply_namespace_to_class_key(class_key, namespace_key.clone());
            }
        }
    }
    for permission in &mut request.graph.namespace_permissions {
        apply_namespace_to_ref_or_key(
            &mut permission.namespace_ref,
            &mut permission.namespace_key,
            namespace_key.clone(),
        );
    }
}

fn apply_namespace_to_class_key(class_key: &mut ClassKey, namespace_key: NamespaceKey) {
    apply_namespace_to_ref_or_key(
        &mut class_key.namespace_ref,
        &mut class_key.namespace_key,
        namespace_key,
    );
}

fn apply_namespace_to_ref_or_key(
    namespace_ref: &mut Option<String>,
    target: &mut Option<NamespaceKey>,
    namespace_key: NamespaceKey,
) {
    if namespace_ref.is_none() && target.is_none() {
        *target = Some(namespace_key);
    }
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

impl GetTaskId for &ImportShow {
    fn task_id(&self) -> Option<i32> {
        self.id
    }
}

impl CliCommand for ImportShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = task_id_or_pos(&query, tokens, 0)?;
        let task = services.gateway().import_task(
            query
                .id
                .ok_or_else(|| AppError::MissingOptions(vec!["id".to_string()]))?,
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&task)?)?,
            OutputFormat::Text => task.format_noreturn()?,
        }

        Ok(())
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

impl GetTaskId for &ImportResults {
    fn task_id(&self) -> Option<i32> {
        self.id
    }
}

impl CliCommand for ImportResults {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = task_id_or_pos(&query, tokens, 0)?;
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

fn task_id_or_pos<U>(
    query: U,
    tokens: &CommandTokenizer,
    pos: usize,
) -> Result<Option<i32>, AppError>
where
    U: GetTaskId,
{
    let pos0 = tokens.get_positionals().get(pos);
    if query.task_id().is_none() {
        if let Some(value) = pos0 {
            return Ok(Some(value.parse()?));
        }
        return Err(AppError::MissingOptions(vec!["id".to_string()]));
    }
    Ok(query.task_id())
}

#[cfg(test)]
mod tests {
    use super::{import_request, ImportSubmit};
    use crate::commands::command_options;
    use crate::errors::AppError;
    use crate::tokenizer::CommandTokenizer;
    use hubuum_client::{ImportAtomicity, ImportCollisionPolicy, ImportPermissionPolicy};

    const EMPTY_IMPORT: &str = r#"{"version":1,"dry_run":null,"mode":null,"graph":{}}"#;

    #[test]
    fn import_request_reads_file_source() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let path = dir.path().join("import.json");
        std::fs::write(&path, EMPTY_IMPORT).expect("file should be written");

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
    fn import_submit_parses_policy_and_namespace_options() {
        let tokens = CommandTokenizer::new(
            "import submit --file payload.json --namespace Math --atomicity best_effort --collision-policy overwrite --permission-policy continue",
            "submit",
            &command_options::<ImportSubmit>(),
        )
        .expect("tokens should parse");

        let query = ImportSubmit::parse_tokens(&tokens).expect("query should parse");
        assert_eq!(query.namespace.as_deref(), Some("Math"));
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
    fn import_request_applies_default_namespace_to_unscoped_class_references() {
        let body = r#"{
            "version": 1,
            "dry_run": null,
            "mode": null,
            "graph": {
                "classes": [
                    {
                        "ref": "host-class",
                        "name": "Hosts",
                        "description": "Hosts",
                        "json_schema": null,
                        "validate_schema": null,
                        "namespace_ref": null,
                        "namespace_key": null
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
                            "namespace_ref": null,
                            "namespace_key": null
                        }
                    }
                ]
            }
        }"#;
        let query = ImportSubmit {
            http: Some(body.to_string()),
            namespace: Some("Math".to_string()),
            ..ImportSubmit::default()
        };

        let request = import_request(&query).expect("request should parse");
        assert_eq!(
            request.graph.classes[0]
                .namespace_key
                .as_ref()
                .map(|key| key.name.as_str()),
            Some("Math")
        );
        assert_eq!(
            request.graph.objects[0]
                .class_key
                .as_ref()
                .and_then(|key| key.namespace_key.as_ref())
                .map(|key| key.name.as_str()),
            Some("Math")
        );
    }
}
