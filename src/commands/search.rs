use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use hubuum_search::{
    PageOptions, ResourceKind, SearchError, SearchRequest, SortDirection, MAX_REQUEST_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use std::fs::File;
use std::io::Read;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, option_or_pos, render_format, CliCommand};
use crate::autocomplete::{classes, file_paths, search_kinds, search_targets};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::command_line::rebuild_with_replaced_options;
use crate::config::get_config;
use crate::domain::{
    SearchBatchRecord, SearchCursorSet, SearchResponseRecord, SearchResultsRecord,
    SearchStreamEvent,
};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::{append_json, OutputFormatter, TableRenderable};
use crate::list_query::{parse_sort_clause, SortDirectionArg, PARTIAL_PIPELINE_WARNING};
use crate::models::OutputFormat;
use crate::output::{
    add_warning, append_line, flush_stream_output, has_pipeline, set_next_page_command,
    set_semantic_output, RenderFormat,
};
use crate::services::{AppServices, SearchInput, SearchKind};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &[],
        catalog_command(
            "search",
            SearchCommand::default(),
            CommandDocs {
                about: Some("Search resources by text or a structured predicate"),
                long_about: Some(
                    "Search across collections, classes, and objects with a plain-text query, or choose --target and a quoted --where predicate for structured server-side search. The REPL completes fields, operators, and boolean expressions inside --where quotes. Use --query-file for the full version 1 JSON search language, including relations. Structured search supports --sort, --limit, --cursor, --include-total, and --all. Plain-text --stream displays text batches or JSONL events as they arrive; JSON output, pipelines, and file redirects are buffered.",
                ),
                examples: Some(
                    r#"server
--query server --kind class --kind object --limit-per-kind 5
streamneedle --stream --kind class --kind object --search-object-data
--target object --class Hosts --where 'data.cpu.cores >= 8 AND name ~ "^srv-"' --sort name asc --limit 25
--query-file search.json --include-total --all --output json"#,
                ),
            },
        ),
    );
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, CommandArgs)]
pub struct SearchCommand {
    #[option(
        long = "target",
        help = "Structured search target: object, class, collection, audit_event, user, group, service_account",
        autocomplete = "search_targets"
    )]
    pub target: Option<String>,
    #[option(
        long = "class",
        help = "Exact class name for a structured object search",
        autocomplete = "classes"
    )]
    pub class: Option<String>,
    #[option(
        long = "where",
        help = "Quoted search predicate with AND, OR, NOT, comparisons, IN, or IS NULL",
        nargs = 1
    )]
    pub predicate: Option<String>,
    #[option(
        long = "sort",
        help = "Structured search sort: field asc|desc",
        nargs = 2
    )]
    pub sorts: Vec<String>,
    #[option(
        long = "query-file",
        help = "JSON file containing a version 1 structured resource search",
        autocomplete = "file_paths"
    )]
    pub query_file: Option<String>,
    #[option(long = "limit", help = "Page size for structured search")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Next-page cursor for structured search")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact authorized structured-search count",
        flag = true
    )]
    pub include_total: Option<bool>,
    #[option(short = "q", long = "query", help = "Plain-text search query")]
    pub query: Option<String>,
    #[option(
        short = "k",
        long = "kind",
        help = "Restrict to collection, class, or object (repeatable)",
        autocomplete = "search_kinds"
    )]
    pub kinds: Vec<SearchKind>,
    #[option(
        long = "limit-per-kind",
        help = "Maximum results to return for each kind"
    )]
    pub limit_per_kind: Option<usize>,
    #[option(
        long = "cursor-collections",
        help = "Cursor for the next collection result page"
    )]
    pub cursor_collections: Option<String>,
    #[option(
        long = "cursor-classes",
        help = "Cursor for the next class result page"
    )]
    pub cursor_classes: Option<String>,
    #[option(
        long = "cursor-objects",
        help = "Cursor for the next object result page"
    )]
    pub cursor_objects: Option<String>,
    #[option(
        long = "search-class-schema",
        help = "Include class schema text in matching",
        flag = "true"
    )]
    pub search_class_schema: Option<bool>,
    #[option(
        long = "search-object-data",
        help = "Include object JSON string values in matching",
        flag = "true"
    )]
    pub search_object_data: Option<bool>,
    #[option(
        long = "stream",
        help = "Use the streaming SSE endpoint",
        flag = "true"
    )]
    pub stream: Option<bool>,
    #[option(
        long = "all",
        help = "Fetch and buffer all result pages before applying pipelines",
        flag = "true"
    )]
    pub all: Option<bool>,
}

impl CliCommand for SearchCommand {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.query = option_or_pos(query.query, tokens, 0, "query")?;

        if query.query_file.is_some() || query.target.is_some() {
            return query.execute_structured(services, tokens);
        }
        if query.limit.is_some()
            || query.cursor.is_some()
            || query.include_total.is_some()
            || query.class.is_some()
            || query.predicate.is_some()
            || !query.sorts.is_empty()
        {
            return Err(AppError::InvalidOption("Structured search options require --target or --query-file; plain-text search uses --limit-per-kind and per-kind cursors".into()));
        }

        let query_string = query
            .query
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["query".to_string()]))?;
        let fetch_all = query.all.unwrap_or(false);
        let stream = query.stream.unwrap_or(false);
        validate_search_mode(fetch_all, stream)?;

        let input = SearchInput {
            query: query_string,
            kinds: query.kinds,
            limit_per_kind: query.limit_per_kind,
            cursor_collections: query.cursor_collections,
            cursor_classes: query.cursor_classes,
            cursor_objects: query.cursor_objects,
            search_class_schema: query.search_class_schema.unwrap_or(false),
            search_object_data: query.search_object_data.unwrap_or(false),
        };

        if stream {
            let format = render_format(tokens)?;
            let mut events = Vec::new();
            let next = services.gateway().search_stream(&input, |event| {
                if matches!(desired_format(tokens)?, OutputFormat::Json) {
                    events.push(event);
                } else if format == RenderFormat::Jsonl && !has_pipeline()? {
                    append_json(&event)?;
                    flush_stream_output()?;
                } else {
                    render_search_event(&event, format == RenderFormat::Text)?;
                    flush_stream_output()?;
                }
                Ok(())
            })?;
            if matches!(desired_format(tokens)?, OutputFormat::Json) {
                append_json(&events)?;
            }
            apply_next_page_state(tokens, &next, format == RenderFormat::Text, false)
        } else {
            let response = if fetch_all {
                services.gateway().search_all(&input)?
            } else {
                services.gateway().search(&input)?
            };
            render_search_response(tokens, &response)
        }
    }
}

impl SearchCommand {
    fn execute_structured(
        &self,
        services: &AppServices,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        if self.query.is_some()
            || !self.kinds.is_empty()
            || self.limit_per_kind.is_some()
            || self.cursor_collections.is_some()
            || self.cursor_classes.is_some()
            || self.cursor_objects.is_some()
            || self.search_class_schema.is_some()
            || self.search_object_data.is_some()
            || self.stream.unwrap_or(false)
        {
            return Err(AppError::InvalidOption(
                "Structured search cannot be combined with plain-text search options or --stream"
                    .into(),
            ));
        }
        let request = self.structured_request()?;
        let mut page = PageOptions::new();
        if let Some(limit) = self.limit {
            page = page.limit(limit);
        }
        if let Some(cursor) = &self.cursor {
            page = page.cursor(cursor);
        }
        if let Some(total) = self.include_total {
            page = page.include_total(total);
        }
        let request = request
            .with_page(page)
            .map_err(|error| AppError::InvalidOption(error.to_string()))?;
        let response = services
            .gateway()
            .structured_search(request, self.all.unwrap_or(false))?;
        if let Some(next) = response.next() {
            set_next_page_command(rebuild_with_replaced_options(
                tokens,
                &["--cursor"],
                [("--cursor", Some(next))],
            ))?;
            if has_pipeline()? {
                add_warning(PARTIAL_PIPELINE_WARNING)?;
            }
        }
        if matches!(desired_format(tokens)?, OutputFormat::Json) {
            return append_json(&response);
        }
        let columns: &[&str] = match response.kind() {
            ResourceKind::AuditEvent => &["id", "occurred_at", "action", "summary"],
            ResourceKind::User => &["id", "name", "identity_scope", "email"],
            ResourceKind::Group => &["id", "groupname", "description"],
            ResourceKind::Object => &[
                "id",
                "name",
                "description",
                "hubuum_class_id",
                "collection_id",
            ],
            _ => &["id", "name", "description"],
        };
        set_semantic_output(OutputEnvelope::rows(
            response.rows(),
            columns.iter().map(|c| c.to_string()).collect(),
        ))?;
        if matches!(render_format(tokens)?, RenderFormat::Text) && !has_pipeline()? {
            if let Some(total) = response.total() {
                append_line(format!(
                    "Total matching {} results: {total}",
                    response.kind().as_str()
                ))?;
            }
            if response.next().is_some() {
                append_line("More results available; use next or --all.")?;
            }
        }
        Ok(())
    }

    fn structured_request(&self) -> Result<SearchRequest, AppError> {
        let invalid = |error: SearchError| AppError::InvalidOption(error.to_string());
        if let Some(path) = &self.query_file {
            if self.target.is_some()
                || self.class.is_some()
                || self.predicate.is_some()
                || !self.sorts.is_empty()
            {
                return Err(AppError::InvalidOption(
                    "--query-file cannot be combined with --target, --class, --where, or --sort"
                        .into(),
                ));
            }
            let mut json = String::new();
            File::open(path)?
                .take(MAX_REQUEST_BYTES as u64 + 1)
                .read_to_string(&mut json)?;
            return SearchRequest::parse(&json).map_err(invalid);
        }
        let target = self
            .target
            .as_deref()
            .ok_or_else(|| AppError::MissingOptions(vec!["target".into()]))?;
        let kind: ResourceKind = from_value(Value::String(target.into()))
            .map_err(|_| AppError::InvalidOption(format!("Unknown search target '{target}'; use object, class, collection, audit_event, user, group, or service_account")))?;
        let mut request = SearchRequest::new(kind);
        if let Some(class) = &self.class {
            request = request.in_class(class).map_err(invalid)?;
        }
        if let Some(predicate) = &self.predicate {
            request = request.with_predicate(predicate).map_err(invalid)?;
        }
        for clause in &self.sorts {
            let sort = parse_sort_clause(clause)?;
            let direction = match sort.direction {
                SortDirectionArg::Asc => SortDirection::Asc,
                SortDirectionArg::Desc => SortDirection::Desc,
            };
            request = request.sort_by(sort.field, direction).map_err(invalid)?;
        }
        Ok(request)
    }
}

fn validate_search_mode(fetch_all: bool, stream: bool) -> Result<(), AppError> {
    if fetch_all && stream {
        return Err(AppError::InvalidOption(
            "--all cannot be combined with --stream".to_string(),
        ));
    }
    Ok(())
}

fn render_search_response(
    tokens: &CommandTokenizer,
    response: &SearchResponseRecord,
) -> Result<(), AppError> {
    if matches!(desired_format(tokens)?, OutputFormat::Json) {
        append_json(response)?;
        return apply_next_page_state(tokens, &response.next, false, true);
    }

    append_line(format!("Query: {}", response.query))?;
    render_search_results(&response.results)?;
    append_line(format!(
        "Returned {} collection(s), {} class(es), {} object(s)",
        response.results.collections.len(),
        response.results.classes.len(),
        response.results.objects.len()
    ))?;

    apply_next_page_state(tokens, &response.next, true, true)
}

fn render_search_event(event: &SearchStreamEvent, text: bool) -> Result<(), AppError> {
    match event {
        SearchStreamEvent::Started(payload) if text => {
            append_line(format!("Streaming query: {}", payload.query))
        }
        SearchStreamEvent::Batch(batch) => {
            if text {
                append_line(format!("Batch: {}", batch.kind))?;
            }
            if text {
                render_search_batch(batch)
            } else {
                if !batch.collections.is_empty() {
                    batch.collections.format_noreturn()?;
                }
                if !batch.classes.is_empty() {
                    batch.classes.format_noreturn()?;
                }
                if !batch.objects.is_empty() {
                    batch.objects.format_noreturn()?;
                }
                Ok(())
            }
        }
        SearchStreamEvent::Done(payload) if text => {
            append_line(format!("Search complete: {}", payload.query))
        }
        _ => Ok(()),
    }
}

fn render_search_results(results: &SearchResultsRecord) -> Result<(), AppError> {
    let mut rendered_any = false;

    rendered_any |= render_group("Collections", &results.collections)?;
    rendered_any |= render_group("Classes", &results.classes)?;
    rendered_any |= render_group("Objects", &results.objects)?;

    if !rendered_any {
        append_line("No results.")?;
    }

    Ok(())
}

fn render_search_batch(batch: &SearchBatchRecord) -> Result<(), AppError> {
    let rendered_any = render_group("Collections", &batch.collections)?
        | render_group("Classes", &batch.classes)?
        | render_group("Objects", &batch.objects)?;

    if !rendered_any {
        append_line("No results in this batch.")?;
    }

    if let Some(cursor) = &batch.next {
        append_line(format!("Next {} cursor: {cursor}", batch.kind))?;
    }

    Ok(())
}

fn render_group<T>(title: &str, items: &[T]) -> Result<bool, AppError>
where
    T: Serialize + Clone + TableRenderable,
{
    if items.is_empty() {
        return Ok(false);
    }

    append_line(title)?;
    items.to_vec().format_noreturn()?;
    Ok(true)
}

fn apply_next_page_state(
    tokens: &CommandTokenizer,
    next: &SearchCursorSet,
    notify: bool,
    supports_all: bool,
) -> Result<(), AppError> {
    if next.is_empty() {
        return Ok(());
    }

    if has_pipeline()? {
        let warning = if supports_all {
            PARTIAL_PIPELINE_WARNING
        } else {
            "Pipeline applied to the current streaming page only; --all cannot be combined with --stream."
        };
        add_warning(warning)?;
    }

    let next_command = next_cursor_command(tokens, next);
    set_next_page_command(next_command)?;

    if !notify {
        return Ok(());
    }

    if get_config().repl.enter_fetches_next_page {
        append_line(
            "Paginated results available. Press Enter for the next page, or Esc/Ctrl-C to stop.",
        )?;
    } else {
        append_line(
            "Paginated results available. Type 'next' for the next page, or Esc/Ctrl-C to stop.",
        )?;
    }

    Ok(())
}

fn next_cursor_command(tokens: &CommandTokenizer, next: &SearchCursorSet) -> String {
    rebuild_with_replaced_options(
        tokens,
        &[
            "--cursor-collections",
            "--cursor-classes",
            "--cursor-objects",
        ],
        [
            ("--cursor-collections", next.collections.as_deref()),
            ("--cursor-classes", next.classes.as_deref()),
            ("--cursor-objects", next.objects.as_deref()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{next_cursor_command, validate_search_mode, SearchCommand};
    use crate::commands::{command_options, option_or_pos};
    use crate::domain::SearchCursorSet;
    use crate::services::SearchKind;
    use crate::tokenizer::CommandTokenizer;

    #[test]
    #[serial_test::serial]
    fn text_stream_batches_report_each_kind_cursor_even_without_rows() {
        use super::render_search_event;
        use crate::domain::{SearchBatchRecord, SearchStreamEvent};
        use crate::output::{reset_output, take_output};

        for kind in ["collections", "classes", "objects"] {
            for next in [None, Some("next-page")] {
                reset_output().unwrap();
                let batch = SearchBatchRecord {
                    kind: kind.into(),
                    collections: Vec::new(),
                    classes: Vec::new(),
                    objects: Vec::new(),
                    next: next.map(str::to_string),
                };
                render_search_event(&SearchStreamEvent::Batch(batch), true).unwrap();
                let output = take_output().unwrap().render();
                assert_eq!(
                    output.contains(&format!("Next {kind} cursor: next-page")),
                    next.is_some(),
                    "{output}",
                );
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn structured_group_output_displays_groupname_and_preserves_resource_fields() {
        use std::sync::Arc;
        use std::time::Duration;

        use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
        use reqwest::StatusCode;
        use serde_json::{from_str, json, Value};
        use tokio::runtime::Runtime;

        use crate::commands::CliCommand;
        use crate::output::{reset_output, set_render_format, take_output, RenderFormat};
        use crate::services::AppServices;

        let transport = MockTransport::default();
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("test"));
        let runtime = Runtime::new().unwrap();
        let services = AppServices::new(
            Arc::new(client),
            runtime.handle().clone(),
            Duration::from_secs(60),
        );
        let group = json!({"id": 7, "groupname": "staff", "description": "operators", "identity_scope": "local"});
        for (name, format) in [
            ("text", RenderFormat::Text),
            ("csv", RenderFormat::Csv),
            ("tsv", RenderFormat::Tsv),
            ("jsonl", RenderFormat::Jsonl),
            ("json", RenderFormat::Json),
        ] {
            transport.push_response(TransportResponse::json(StatusCode::OK, &json!({
                "version": 1, "kind": "group", "results": [{"kind": "group", "resource": group}],
                "next": null, "total": null,
            })).unwrap());
            let tokens = CommandTokenizer::new(
                &format!("search --target group --output {name}"),
                "search",
                &command_options::<SearchCommand>(),
            )
            .unwrap();
            reset_output().unwrap();
            set_render_format(format).unwrap();
            SearchCommand::default()
                .execute(&services, &tokens)
                .unwrap();
            let output = take_output().unwrap();
            let rendered = output.render();
            assert!(rendered.contains("staff"), "{name}: {rendered}");
            if format == RenderFormat::Json {
                let value: Value = from_str(&rendered).unwrap();
                assert_eq!(value["results"][0]["resource"], group);
            } else {
                assert_eq!(output.semantic[0].value(), &json!([group]));
            }
        }
    }

    #[test]
    fn query_or_pos_uses_first_positional_when_missing_flag() {
        let tokens = CommandTokenizer::new(
            "search server --kind class",
            "search",
            &command_options::<SearchCommand>(),
        )
        .expect("tokenization should succeed");

        let query = option_or_pos(SearchCommand::default().query, &tokens, 0, "query")
            .expect("query resolution should succeed");
        assert_eq!(query.as_deref(), Some("server"));
    }

    #[test]
    fn next_cursor_command_replaces_existing_cursor_flags() {
        let tokens = CommandTokenizer::new(
            "search server --kind class --cursor-classes old",
            "search",
            &command_options::<SearchCommand>(),
        )
        .expect("tokenization should succeed");

        let command = next_cursor_command(
            &tokens,
            &SearchCursorSet {
                classes: Some("next cursor".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(
            command,
            "search server --kind class --cursor-classes 'next cursor'"
        );
    }

    #[test]
    fn parse_tokens_accepts_repeatable_kind_values() {
        let tokens = CommandTokenizer::new(
            "search --query server --kind collection --kind object",
            "search",
            &command_options::<SearchCommand>(),
        )
        .expect("tokenization should succeed");

        let parsed = SearchCommand::parse_tokens(&tokens).expect("parse should succeed");
        assert_eq!(
            parsed.kinds,
            vec![SearchKind::Collection, SearchKind::Object]
        );
    }

    #[test]
    fn parse_tokens_accepts_all_pages_flag() {
        let tokens = CommandTokenizer::new(
            "search server --all",
            "search",
            &command_options::<SearchCommand>(),
        )
        .expect("tokenization should succeed");

        let parsed = SearchCommand::parse_tokens(&tokens).expect("parse should succeed");
        assert_eq!(parsed.all, Some(true));
    }

    #[test]
    fn terminal_predicate_preserves_quotes_and_repeatable_sort_fields() {
        let tokens = CommandTokenizer::new(
            r#"search --target object --class Hosts --where 'name == "a room" AND data.cores >= 8' --sort name asc --sort id desc"#,
            "search",
            &command_options::<SearchCommand>(),
        ).unwrap();
        let parsed = SearchCommand::parse_tokens(&tokens).unwrap();
        let wire = serde_json::to_value(parsed.structured_request().unwrap()).unwrap();
        assert_eq!(wire["filter"]["args"][0]["predicate"]["value"], "a room");
        assert_eq!(wire["filter"]["args"][1]["predicate"]["value"], 8);
        assert_eq!(wire["sort"][0]["field"], "name");
        assert_eq!(wire["sort"][1]["direction"], "desc");
    }

    #[test]
    fn all_pages_rejects_streaming_search() {
        let error = validate_search_mode(true, true).expect_err("modes should conflict");
        assert!(error.to_string().contains("--all"));
        assert!(error.to_string().contains("--stream"));
    }
}
