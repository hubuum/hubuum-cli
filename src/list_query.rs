mod fetch;
mod render;
pub(crate) use fetch::{fetch_cursor_results, fetch_query_results};
pub(crate) use render::{
    append_paging_footer, render_paged_result, set_paged_json_output, warn_on_partial_pipeline,
};
#[cfg(test)]
use render::{next_cursor_command, should_wrap_paged_json};

pub use hubuum_query::{
    completion_operators, completion_sort_directions, filter_clause, parse_sort_clause,
    parse_where_clause, resolve_filter_field_spec, resolve_sort_field_spec,
    validate_filter_clauses, validate_sort_clauses, FilterClause, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, FilterValueResolver, PageSelection, ServerPageSize,
    SortClause, SortDirectionArg, SortFieldSpec, ValidatedFilterClause, ValidatedSortClause,
    SERVER_MAX_PAGE_SIZE,
};

pub(crate) const PARTIAL_PIPELINE_WARNING: &str =
    "Pipeline applied to the current page only; use --all for all matching results.";

use crate::diagnostics::client_page;

use hubuum_client::{client::Page, ApiError};
use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListQuery {
    pub filters: Vec<FilterClause>,
    pub sorts: Vec<SortClause>,
    pub limit: Option<ServerPageSize>,
    pub cursor: Option<String>,
    pub include_total: bool,
    #[serde(default)]
    pub page_selection: PageSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub returned_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u64>,
}

impl<T> PagedResult<T> {
    pub(crate) fn empty(next_cursor: Option<String>, total_count: Option<u64>) -> Self {
        Self {
            items: Vec::new(),
            next_cursor,
            returned_count: 0,
            total_count,
        }
    }

    pub(crate) fn from_page<U, F>(page: Page<U>, map: F) -> Self
    where
        F: Fn(U) -> T,
    {
        let total_count = page.total_count;
        let items = page.items.into_iter().map(map).collect::<Vec<_>>();
        let returned_count = items.len();
        Self {
            items,
            next_cursor: page.next_cursor,
            returned_count,
            total_count,
        }
    }

    pub(crate) fn from_pages<I>(pages: I) -> Result<Self, AppError>
    where
        I: IntoIterator<Item = Result<Page<T>, ApiError>>,
    {
        let mut pages = pages.into_iter();
        let Some(first_page) = client_page(&mut pages) else {
            return Ok(Self::empty(None, None));
        };
        let first_page = first_page?;
        let total_count = first_page.total_count;
        let mut items = first_page.items;
        while let Some(page) = client_page(&mut pages) {
            items.extend(page?.items);
        }
        let returned_count = items.len();
        Ok(Self {
            items,
            next_cursor: None,
            returned_count,
            total_count,
        })
    }

    pub(crate) fn try_map<U, E>(
        self,
        map: impl FnMut(T) -> Result<U, E>,
    ) -> Result<PagedResult<U>, E> {
        Ok(PagedResult {
            items: self.items.into_iter().map(map).collect::<Result<_, _>>()?,
            next_cursor: self.next_cursor,
            returned_count: self.returned_count,
            total_count: self.total_count,
        })
    }

    pub(crate) fn map<U, F>(self, map: F) -> PagedResult<U>
    where
        F: Fn(T) -> U,
    {
        PagedResult {
            items: self.items.into_iter().map(map).collect(),
            next_cursor: self.next_cursor,
            returned_count: self.returned_count,
            total_count: self.total_count,
        }
    }
}

impl ListQuery {
    pub(crate) fn page_selection(mut self, page_selection: PageSelection) -> Self {
        self.page_selection = page_selection;
        self
    }

    pub(crate) const fn fetches_all(&self) -> bool {
        matches!(self.page_selection, PageSelection::All)
    }
}

pub fn list_query_from_raw(
    where_clauses: &[String],
    sort_clauses: &[String],
    limit: Option<usize>,
    cursor: Option<String>,
) -> Result<ListQuery, AppError> {
    let filters = where_clauses
        .iter()
        .map(|clause| parse_where_clause(clause))
        .collect::<Result<Vec<_>, _>>()?;
    let sorts = sort_clauses
        .iter()
        .map(|clause| parse_sort_clause(clause))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ListQuery {
        filters,
        sorts,
        limit: limit.map(ServerPageSize::from_requested).transpose()?,
        cursor,
        include_total: false,
        page_selection: PageSelection::Single,
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn page_sizes_are_validated_during_construction_and_deserialization() {
        use super::ServerPageSize;
        assert!(ServerPageSize::from_requested(0).is_err());
        assert!(serde_json::from_str::<ServerPageSize>("0").is_err());
        let size = ServerPageSize::from_requested(1000).unwrap();
        assert_eq!(size.requested(), 1000);
        assert_eq!(size.effective(), 250);
        assert!(size.was_truncated());
        assert_eq!(serde_json::to_string(&size).unwrap(), "250");
    }

    use std::sync::Once;

    use serde::Serialize;
    use serde_json::{from_str, Value};
    use serial_test::serial;

    use super::{
        list_query_from_raw, next_cursor_command, render_paged_result, should_wrap_paged_json,
        PagedResult, SortDirectionArg, SERVER_MAX_PAGE_SIZE,
    };
    use crate::commands::render_format;
    use crate::config::{get_config, init_config, AppConfig};
    use crate::formatting::TableRenderable;
    use crate::models::OutputFormat;
    use crate::output::{
        reset_output, set_pipeline, set_pipeline_suffix, set_render_format, take_output,
        RenderFormat,
    };
    use crate::tokenizer::CommandTokenizer;

    use hubuum_filter::{
        AggregateFunction, AggregateRequest, AggregateSpec, GroupKey, PipeStage, ProjectTerm,
    };

    static CONFIG_INIT: Once = Once::new();

    #[test]
    fn parses_where_clauses_with_symbols_and_spaces() {
        let query = list_query_from_raw(
            &["name icontains foo bar".to_string(), "id >= 10".to_string()],
            &["name asc".to_string()],
            Some(10),
            None,
        )
        .expect("query should parse");

        assert_eq!(query.filters.len(), 2);
        assert_eq!(query.sorts.len(), 1);
        assert_eq!(query.limit.map(super::ServerPageSize::effective), Some(10));
    }

    #[test]
    fn truncates_page_size_requests_above_the_server_maximum() {
        let query = list_query_from_raw(&[], &[], Some(251), None)
            .expect("oversized page size should parse");

        assert_eq!(
            query.limit.map(super::ServerPageSize::effective),
            Some(SERVER_MAX_PAGE_SIZE)
        );
    }

    #[test]
    fn parses_sort_clauses_in_order() {
        let query = list_query_from_raw(
            &[],
            &["name asc".to_string(), "created_at desc".to_string()],
            None,
            None,
        )
        .expect("sort query should parse");

        assert_eq!(
            query
                .sorts
                .iter()
                .map(|clause| (clause.field.as_str(), clause.direction))
                .collect::<Vec<_>>(),
            vec![
                ("name", SortDirectionArg::Asc),
                ("created_at", SortDirectionArg::Desc),
            ]
        );
    }

    #[test]
    fn paged_json_wraps_when_explicit_page_size_has_next_cursor() {
        let tokens = CommandTokenizer::new("class list --limit 10", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![1, 2],
            next_cursor: Some("abc123".to_string()),
            returned_count: 2,
            total_count: None,
        };

        assert!(should_wrap_paged_json(&tokens, &paged));
    }

    #[test]
    fn paged_json_wraps_when_cursor_exists() {
        let tokens = CommandTokenizer::new("class list --cursor abc123 --limit 10", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![1, 2],
            next_cursor: None,
            returned_count: 2,
            total_count: None,
        };

        assert!(should_wrap_paged_json(&tokens, &paged));
    }

    #[test]
    fn paged_json_wraps_when_page_has_next_cursor() {
        let tokens =
            CommandTokenizer::new("class list", "list", &[]).expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![1, 2],
            next_cursor: Some("abc123".to_string()),
            returned_count: 2,
            total_count: None,
        };

        assert!(should_wrap_paged_json(&tokens, &paged));
    }

    #[test]
    fn paged_json_wraps_when_total_count_is_available() {
        let tokens =
            CommandTokenizer::new("class list", "list", &[]).expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![1, 2],
            next_cursor: None,
            returned_count: 2,
            total_count: Some(42),
        };

        assert!(should_wrap_paged_json(&tokens, &paged));
    }

    #[derive(Clone, Serialize)]
    struct DummyRow {
        id: i32,
    }

    impl TableRenderable for DummyRow {
        fn headers() -> Vec<&'static str> {
            vec!["id"]
        }

        fn row(&self) -> Vec<String> {
            vec![self.id.to_string()]
        }
    }

    #[test]
    #[serial]
    fn text_output_appends_pagination_footer() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        init_config(AppConfig::default()).expect("config should update");
        reset_output().expect("output should reset");
        let tokens =
            CommandTokenizer::new("class list", "list", &[]).expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 1,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Paginated results available.")));
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Type 'next' for the next page")));
        assert_eq!(
            snapshot.next_page_command.as_deref(),
            Some("class list --cursor abc123")
        );
    }

    #[test]
    #[serial]
    fn machine_output_omits_pagination_prose() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });

        for format in ["jsonl", "csv", "tsv"] {
            reset_output().expect("output should reset");
            let tokens = CommandTokenizer::new(
                &format!("class list --output {format} --limit 1"),
                "list",
                &[],
            )
            .expect("tokenization should succeed");
            set_render_format(render_format(&tokens).expect("render format"))
                .expect("render format should set");
            let paged = PagedResult {
                items: vec![DummyRow { id: 1 }],
                next_cursor: Some("abc123".to_string()),
                returned_count: 1,
                total_count: None,
            };

            render_paged_result(&tokens, &paged, OutputFormat::Text)
                .expect("machine output should render semantically");

            let snapshot = take_output().expect("snapshot should be captured");
            assert!(!snapshot.lines.iter().any(|line| line.contains("Returned")));
            assert!(!snapshot
                .lines
                .iter()
                .any(|line| line.contains("Paginated results available")));
            assert!(snapshot.next_page_command.is_none());
            if format == "jsonl" {
                assert!(snapshot
                    .lines
                    .iter()
                    .all(|line| from_str::<Value>(line).is_ok()));
            }
        }
    }

    #[test]
    #[serial]
    fn json_paged_results_remain_semantic_until_after_pipeline() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        set_render_format(RenderFormat::Json).expect("render format should set");
        set_pipeline(vec![PipeStage::Columns(vec![
            ProjectTerm::keep("id").expect("valid selector")
        ])])
        .expect("pipeline should set");
        let tokens = CommandTokenizer::new("class list --json", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: None,
            returned_count: 1,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Json)
            .expect("JSON output should remain semantic");

        let snapshot = take_output().expect("snapshot should be captured");
        assert_eq!(snapshot.semantic.len(), 1);
        assert!(snapshot.render().contains("\"id\""));
    }

    #[test]
    #[serial]
    fn next_cursor_command_preserves_pipeline_suffix() {
        reset_output().expect("output should reset");
        set_pipeline_suffix(Some("| P id".to_string())).expect("pipeline suffix should set");
        let tokens = CommandTokenizer::new("class list --cursor old", "list", &[])
            .expect("tokenization should succeed");

        assert_eq!(
            next_cursor_command(&tokens, "next").expect("next command"),
            "class list --cursor next | P id"
        );
    }

    #[test]
    #[serial]
    fn text_output_uses_effective_page_size_for_next_command() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        let tokens = CommandTokenizer::new("class list --limit 500", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 1,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line == "Returned 1 item(s)"));
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Paginated results available.")));
        assert_eq!(
            snapshot.next_page_command.as_deref(),
            Some("class list --limit 250 --cursor abc123")
        );
    }

    #[test]
    #[serial]
    fn text_output_reports_exact_total_count() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        let tokens = CommandTokenizer::new("class list --include-total", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: None,
            returned_count: 1,
            total_count: Some(42),
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Returned 1 item(s) (total: 42)")));
    }

    #[test]
    #[serial]
    fn text_output_keeps_pagination_for_explicit_cursor_with_limit() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        let tokens = CommandTokenizer::new("class list --cursor old --limit 1", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 1,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Paginated results available.")));
        assert_eq!(
            snapshot.next_page_command.as_deref(),
            Some("class list --limit 1 --cursor abc123")
        );
    }

    #[test]
    #[serial]
    fn terminal_pipeline_stage_suppresses_pagination_footer() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        set_pipeline(vec![PipeStage::Head {
            count: 10,
            offset: 0,
        }])
        .expect("pipeline should set");
        let tokens = CommandTokenizer::new("class list --limit 100", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }, DummyRow { id: 2 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 100,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot.next_page_command.is_none());
        assert!(snapshot.warnings.iter().any(|warning| warning.contains(
            "Pipeline applied to the current page only; use --all for all matching results."
        )));
        assert!(!snapshot
            .lines
            .iter()
            .any(|line| line.contains("Paginated results available.")));
        assert!(!snapshot
            .lines
            .iter()
            .any(|line| line.contains("Returned 100 item")));
    }

    #[test]
    #[serial]
    fn grouped_pipeline_suppresses_pagination_footer() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        set_pipeline(vec![
            PipeStage::Group(vec![
                GroupKey::new("os_version", "OS Version").expect("valid group key")
            ]),
            PipeStage::Aggregate(AggregateRequest::grouped(
                AggregateSpec::new(AggregateFunction::Count, "Hosts").expect("valid aggregate"),
            )),
        ])
        .expect("pipeline should set");
        let tokens = CommandTokenizer::new("object list --class Hosts", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }, DummyRow { id: 2 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 100,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot.next_page_command.is_none());
        assert!(!snapshot
            .lines
            .iter()
            .any(|line| line.contains("Paginated results available.")));
        assert!(!snapshot
            .lines
            .iter()
            .any(|line| line.contains("Returned 100 item")));
    }

    #[test]
    #[serial]
    fn global_aggregate_pipeline_warns_when_only_one_page_is_available() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        reset_output().expect("output should reset");
        set_pipeline(vec![PipeStage::Aggregate(
            AggregateRequest::global(vec![
                AggregateSpec::new(AggregateFunction::Count, "Hosts").expect("valid aggregate")
            ])
            .expect("valid global aggregate"),
        )])
        .expect("pipeline should set");
        let tokens = CommandTokenizer::new("object list --class Hosts", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }, DummyRow { id: 2 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 100,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot.next_page_command.is_none());
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("use --all for all matching results")));
    }

    #[test]
    #[serial]
    fn text_output_omits_pagination_footer_without_limit_or_next_cursor() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        init_config(AppConfig::default()).expect("config should update");
        reset_output().expect("output should reset");
        let tokens =
            CommandTokenizer::new("class list", "list", &[]).expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: None,
            returned_count: 1,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot
            .lines
            .iter()
            .all(|line| !line.contains("Returned ")));
        assert!(snapshot.next_page_command.is_none());
    }

    #[test]
    #[serial]
    fn text_output_uses_enter_hint_when_config_enables_it() {
        CONFIG_INIT.call_once(|| {
            let _ = init_config(AppConfig::default());
        });
        let previous = get_config();
        let mut config = AppConfig::default();
        config.repl.enter_fetches_next_page = true;
        init_config(config).expect("config should update");

        reset_output().expect("output should reset");
        let tokens = CommandTokenizer::new("class list --limit 1", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: Some("abc123".to_string()),
            returned_count: 1,
            total_count: None,
        };

        render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        init_config((*previous).clone()).expect("config should be restored");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Press Enter for the next page")));
    }

    #[test]
    #[serial]
    fn next_cursor_command_replaces_existing_cursor() {
        reset_output().expect("output should reset");
        for (raw, next, expected) in [
            (
                "class list --limit 1 --cursor old",
                "next token",
                "class list --limit 1 --cursor 'next token'",
            ),
            (
                "class list --cursor=old --limit 1",
                "abc123",
                "class list --limit 1 --cursor abc123",
            ),
        ] {
            let tokens = CommandTokenizer::new(raw, "list", &[]).unwrap();
            assert_eq!(next_cursor_command(&tokens, next).unwrap(), expected);
        }
    }
}
