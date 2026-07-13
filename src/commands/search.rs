use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, option_or_pos, CliCommand};
use crate::autocomplete::search_kinds;
use crate::catalog::CommandCatalogBuilder;
use crate::command_line::rebuild_with_replaced_options;
use crate::config::get_config;
use crate::domain::{
    SearchBatchRecord, SearchCursorSet, SearchResponseRecord, SearchResultsRecord,
    SearchStreamEvent,
};
use crate::errors::AppError;
use crate::formatting::{append_json, OutputFormatter, TableRenderable};
use crate::models::OutputFormat;
use crate::output::{add_error, append_line, set_next_page_command};
use crate::services::{AppServices, SearchInput, SearchKind};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &[],
        catalog_command(
            "search",
            SearchCommand::default(),
            CommandDocs {
                about: Some("Run a unified search"),
                long_about: Some(
                    "Search across collections, classes, and objects. Pass the query as the first positional argument or with --query. Use --stream to consume the server-sent event variant of the endpoint.",
                ),
                examples: Some(
                    r#"server
--query server --kind class --kind object --limit-per-kind 5
streamneedle --stream --kind class --kind object --search-object-data"#,
                ),
            },
        ),
    );
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, CommandArgs)]
pub struct SearchCommand {
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
}

impl CliCommand for SearchCommand {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.query = option_or_pos(query.query, tokens, 0, "query")?;

        let query_string = query
            .query
            .clone()
            .ok_or_else(|| AppError::MissingOptions(vec!["query".to_string()]))?;

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

        if query.stream.unwrap_or(false) {
            let events = services.gateway().search_stream(&input)?;
            render_search_stream(tokens, &events)
        } else {
            let response = services.gateway().search(&input)?;
            render_search_response(tokens, &response)
        }
    }
}

fn render_search_response(
    tokens: &CommandTokenizer,
    response: &SearchResponseRecord,
) -> Result<(), AppError> {
    if matches!(desired_format(tokens), OutputFormat::Json) {
        append_json(response)?;
        return apply_next_page_state(tokens, &response.next, false);
    }

    append_line(format!("Query: {}", response.query))?;
    render_search_results(&response.results)?;
    append_line(format!(
        "Returned {} collection(s), {} class(es), {} object(s)",
        response.results.collections.len(),
        response.results.classes.len(),
        response.results.objects.len()
    ))?;

    apply_next_page_state(tokens, &response.next, true)
}

fn render_search_stream(
    tokens: &CommandTokenizer,
    events: &[SearchStreamEvent],
) -> Result<(), AppError> {
    if matches!(desired_format(tokens), OutputFormat::Json) {
        append_json(events)?;
        let next = next_from_stream(events);
        return apply_next_page_state(tokens, &next, false);
    }

    let mut started_query: Option<String> = None;
    for event in events {
        match event {
            SearchStreamEvent::Started(payload) => {
                started_query = Some(payload.query.clone());
                append_line(format!("Streaming query: {}", payload.query))?;
            }
            SearchStreamEvent::Batch(batch) => {
                append_line("")?;
                append_line(format!("Batch: {}", batch.kind))?;
                render_search_batch(batch)?;
                if let Some(next) = &batch.next {
                    append_line(format!("Next cursor for {}: {}", batch.kind, next))?;
                }
            }
            SearchStreamEvent::Done(payload) => {
                append_line("")?;
                append_line(format!("Search complete: {}", payload.query))?;
            }
            SearchStreamEvent::Error(payload) => {
                add_error(&payload.message)?;
            }
        }
    }

    if started_query.is_none() && events.is_empty() {
        append_line("No events returned.")?;
    }

    let next = next_from_stream(events);
    apply_next_page_state(tokens, &next, true)
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

fn next_from_stream(events: &[SearchStreamEvent]) -> SearchCursorSet {
    let mut next = SearchCursorSet::default();

    for event in events {
        let SearchStreamEvent::Batch(batch) = event else {
            continue;
        };

        match batch.kind.as_str() {
            "collections" => next.collections = batch.next.clone(),
            "classes" => next.classes = batch.next.clone(),
            "objects" => next.objects = batch.next.clone(),
            _ => {}
        }
    }

    next
}

fn apply_next_page_state(
    tokens: &CommandTokenizer,
    next: &SearchCursorSet,
    notify: bool,
) -> Result<(), AppError> {
    if next.is_empty() {
        return Ok(());
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
    use super::{next_cursor_command, SearchCommand};
    use crate::commands::{command_options, option_or_pos};
    use crate::domain::SearchCursorSet;
    use crate::services::SearchKind;
    use crate::tokenizer::CommandTokenizer;

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
}
