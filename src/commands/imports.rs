use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{parse_task_submit_options, run_task_backed};
use super::{build_list_query, desired_format, render_list_page, CliCommand};
use crate::autocomplete::{file_paths, import_result_sort};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
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
                        "Submit an import request from a local JSON file or HTTP(S) URL.",
                    ),
                    examples: Some("--file import.json\n--http https://example.com/import.json"),
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
        let request_json = import_request_json(&query)?;
        let task = services.gateway().submit_import(SubmitImportInput {
            request_json,
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

fn import_request_json(query: &ImportSubmit) -> Result<String, AppError> {
    match (&query.file, &query.http) {
        (Some(_), Some(_)) => Err(AppError::ParseError(
            "Use either --file or --http, not both".to_string(),
        )),
        (Some(file), None) => std::fs::read_to_string(file).map_err(AppError::IoError),
        (None, Some(http_body)) => Ok(http_body.clone()),
        (None, None) => Err(AppError::MissingOptions(vec![
            "file".to_string(),
            "http".to_string(),
        ])),
    }
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
    use super::{import_request_json, ImportSubmit};
    use crate::errors::AppError;

    #[test]
    fn import_request_json_reads_file_source() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let path = dir.path().join("import.json");
        std::fs::write(&path, "{\"items\":[]}").expect("file should be written");

        let query = ImportSubmit {
            file: Some(path.to_string_lossy().to_string()),
            ..ImportSubmit::default()
        };

        assert_eq!(
            import_request_json(&query).expect("file should load"),
            "{\"items\":[]}"
        );
    }

    #[test]
    fn import_request_json_accepts_http_body_source() {
        let query = ImportSubmit {
            http: Some("{\"items\":[]}".to_string()),
            ..ImportSubmit::default()
        };

        assert_eq!(
            import_request_json(&query).expect("http body should be used"),
            "{\"items\":[]}"
        );
    }

    #[test]
    fn import_request_json_rejects_missing_or_multiple_sources() {
        assert!(matches!(
            import_request_json(&ImportSubmit::default()),
            Err(AppError::MissingOptions(options)) if options == vec!["file", "http"]
        ));

        let query = ImportSubmit {
            file: Some("import.json".to_string()),
            http: Some("{\"items\":[]}".to_string()),
            ..ImportSubmit::default()
        };
        assert!(matches!(
            import_request_json(&query),
            Err(AppError::ParseError(message)) if message.contains("either --file or --http")
        ));
    }
}
