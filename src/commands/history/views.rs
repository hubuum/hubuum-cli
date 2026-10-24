use cli_command_derive::CommandArgs;
use hubuum_client::types::SortDirection;
use hubuum_client::{
    ClassId, HistoryDirection, HistoryId, HistoryPageOptions, HistorySortField,
    HistoryTimelineOptions, ObjectHistoryOptions, ObjectId, RelationHistoryOptions,
    ResourceRevision,
};

use crate::autocomplete::{
    classes, history_direction, history_sort, history_times, objects_from_class,
    relation_history_sort,
};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::commands::builder::{catalog_command, CommandDocs};
use crate::commands::{
    normalize_server_page_size, render_json_record, required_option, response_format, CliCommand,
    PageSelection,
};
use crate::domain::{HistoryComparison, HistoryPageRecord};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::{append_json, OutputFormatter};
use crate::list_query::{append_paging_footer, warn_on_partial_pipeline};
use crate::models::OutputFormat;
use crate::output::{append_line, has_pipeline};
use crate::services::{parse_timestamp, AppServices, HistoricalQuery, ResourceScope};
use crate::tokenizer::CommandTokenizer;

pub(super) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["history", "timeline"],
            catalog_command(
                "class",
                ClassTimeline::default(),
                docs(
                    "Navigate class history and field changes",
                    "--class-id 1 --direction older --limit 20\n--class-id 1 --against-latest",
                ),
            ),
        )
        .add_command(
            &["history", "timeline"],
            catalog_command(
                "object",
                ObjectTimeline::default(),
                docs(
                    "Navigate object history and field changes",
                    "--class-id 1 --object-id 2 --anchor 42 --direction newer\n--class Hosts --name escher.uio.no --against-latest",
                ),
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "object-name",
                ObjectNameHistory::default(),
                docs(
                    "Find exact-name snapshots across object identities",
                    "--class-id 1 --name old-host --all",
                ),
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "objects",
                HistoricalObjects::default(),
                docs(
                    "Read the retained object population and class schema at an instant",
                    "--class-id 1 --at 2026-10-23T00:00:00Z",
                ),
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "relations",
                HistoricalRelations::default(),
                docs(
                    "Read direct relationship history or an as-of view",
                    "--class-id 1\n--class-id 1 --object-id 2 --at 2026-10-23T00:00:00Z",
                ),
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "computed",
                HistoricalComputed::default(),
                docs(
                    "Evaluate retained shared computed definitions and object input",
                    "--class-id 1 --object-id 2 --at 2026-10-23T00:00:00Z",
                ),
            ),
        );
}

fn docs(about: &'static str, examples: &'static str) -> CommandDocs {
    CommandDocs { about: Some(about), examples: Some(examples), long_about: Some("Reads retained historical data under current authorization. IDs avoid live name lookup. Missing history is explicit and coverage does not establish complete past membership. JSON and pipelines retain the response envelope. Timeline --at seeks; use history show --at for a snapshot. Timeline --at and --anchor are exclusive; windows are [from, until). --all is bounded to 1,000 pages and 100,000 records.") }
}

// The paginated historical endpoints share resource and cursor arguments, but
// keep their selection options explicit in each command's generated help.
macro_rules! history_page_command {
    ($name:ident { $($fields:tt)* } |$query:ident, $services:ident, $tokens:ident| $body:block) => {
        #[derive(Clone, Default, CommandArgs)]
        struct $name {
            #[option(long = "class", help = "Class name", autocomplete = "classes")]
            class: Option<String>,
            #[option(long = "class-id", help = "Positive class ID; mutually exclusive with --class")]
            class_id: Option<ClassId>,
            #[option(long = "limit", help = "Page size (maximum 250)")]
            limit: Option<usize>,
            #[option(long = "cursor", help = "Opaque continuation cursor for this query")]
            cursor: Option<String>,
            #[option(long = "include-total", help = "Request the exact matching count", flag = true)]
            include_total: bool,
            #[option(long = "all", help = "Fetch all remaining pages within safety limits", flag = true)]
            all: bool,
            $($fields)*
        }
        impl $name {
            fn page(&self) -> Result<HistoryPageOptions, AppError> {
                let mut options = HistoryPageOptions::default().include_total(self.include_total);
                if self.limit == Some(0) { return Err(AppError::InvalidOption("--limit must be positive".into())); }
                if let Some(limit) = normalize_server_page_size(self.limit)? { options = options.limit(limit)?; }
                if let Some(cursor) = &self.cursor { options = options.cursor(cursor); }
                Ok(options)
            }
        }
        impl CliCommand for $name {
            const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
            const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
            fn execute(&self, $services: &AppServices, $tokens: &CommandTokenizer) -> Result<(), AppError> {
                let $query = Self::parse_tokens($tokens)?;
                $body
            }
        }
    }
}

macro_rules! timeline_command {
    ($name:ident { $($fields:tt)* } |$query:ident| $scope:expr) => {
        history_page_command!($name {
            $($fields)*
            #[option(long = "from", help = "Inclusive entry time: RFC3339, UTC date/time, or time ago")]
            from: Option<String>,
            #[option(long = "until", help = "Exclusive entry time: RFC3339, UTC date/time, or time ago")]
            until: Option<String>,
            #[option(long = "at", help = "Seek time: RFC3339, UTC date/time, or time ago; exclusive with --anchor", autocomplete = "history_times")]
            at: Option<String>,
            #[option(long = "anchor", help = "Seek past this history ID, excluding the entry")]
            anchor: Option<HistoryId>,
            #[option(long = "direction", help = "older (default) or newer", autocomplete = "history_direction")]
            direction: Option<String>,
            #[option(long = "against-latest", help = "Compare each entry to the newest retained snapshot, outside any selected window", flag = true)]
            against_latest: bool,
        } |$query, services, tokens| {
            let page = $query.page()?;
            let scope = $scope;
            let mut options = HistoryTimelineOptions::default().window(
                $query.from.as_deref().map(parse_timestamp).transpose()?,
                $query.until.as_deref().map(parse_timestamp).transpose()?,
            )?.direction(match $query.direction.as_deref().unwrap_or("older") {
                "older" => HistoryDirection::Older,
                "newer" => HistoryDirection::Newer,
                _ => return Err(AppError::InvalidOption("--direction must be older or newer".into())),
            });
            options = match ($query.at, $query.anchor) {
                (Some(_), Some(_)) => return Err(AppError::InvalidOption("--at and --anchor are mutually exclusive".into())),
                (Some(at), None) => options.at(parse_timestamp(&at)?),
                (None, Some(anchor)) => options.anchor(anchor)?,
                (None, None) => options,
            };
            let comparison = if $query.against_latest { HistoryComparison::Latest } else { HistoryComparison::Previous };
            let result = services.gateway().historical_view(scope, HistoricalQuery::TimelineWithChanges { options, comparison }, page, PageSelection::from_all($query.all))?;
            render_timeline_page(tokens, &result)
        });
    }
}
timeline_command!(ClassTimeline {} | query | ResourceScope::class(query.class, query.class_id)?);
timeline_command!(ObjectTimeline {
    #[option(long = "name", help = "Object name", autocomplete = "objects_from_class")]
    name: Option<String>,
    #[option(long = "object-id", help = "Positive object ID")]
    object_id: Option<ObjectId>,
} |query| ResourceScope::class(query.class, query.class_id)?.with_object(query.name, query.object_id)?.require_object()?.into());

history_page_command!(ObjectNameHistory {
    #[option(long = "name", help = "Literal historical name, including numeric names")]
    name: String,
    #[option(long = "revision", help = "Positive resource revision")]
    revision: Option<i64>,
    #[option(long = "sort", help = "history_id or revision; prefix with '-' for descending", autocomplete = "history_sort")]
    sort: Option<String>,
} |query, services, tokens| {
    let page = query.page()?;
    if query.name.trim().is_empty() { return Err(AppError::InvalidOption("--name must not be blank".into())); }
    let options = object_options(query.sort.as_deref(), query.revision)?;
    let scope = ResourceScope::class(query.class, query.class_id)?;
    render_page(tokens, &services.gateway().historical_view(scope, HistoricalQuery::ObjectName { name: query.name, options }, page, PageSelection::from_all(query.all))?)
});

history_page_command!(HistoricalObjects {
    #[option(long = "at", help = "As-of time: RFC3339, UTC date/time, or time ago (e.g. 1d3h)", autocomplete = "history_times")]
    at: String,
    #[option(long = "revision", help = "Positive resource revision")]
    revision: Option<i64>,
    #[option(long = "sort", help = "history_id or revision; prefix with '-' for descending", autocomplete = "history_sort")]
    sort: Option<String>,
} |query, services, tokens| {
    let page = query.page()?;
    let options = object_options(query.sort.as_deref(), query.revision)?;
    let scope = ResourceScope::class(query.class, query.class_id)?;
    render_page(tokens, &services.gateway().historical_view(scope, HistoricalQuery::Objects { at: parse_timestamp(&query.at)?, options }, page, PageSelection::from_all(query.all))?)
});

history_page_command!(HistoricalRelations {
    #[option(long = "name", help = "Object name; omit for class relations", autocomplete = "objects_from_class")]
    name: Option<String>,
    #[option(long = "object-id", help = "Positive object ID; omit for class relations")]
    object_id: Option<ObjectId>,
    #[option(long = "at", help = "As-of time: RFC3339, UTC date/time, or time ago; omit to read changes", autocomplete = "history_times")]
    at: Option<String>,
    #[option(long = "sort", help = "history_id or -history_id", autocomplete = "relation_history_sort")]
    sort: Option<String>,
} |query, services, tokens| {
    let page = query.page()?;
    let mut options = RelationHistoryOptions::default();
    if let Some(sort) = query.sort.as_deref() {
        options = options.order(match sort { "history_id" => SortDirection::Asc, "-history_id" => SortDirection::Desc, _ => return Err(AppError::InvalidOption("Relation history sort must be history_id or -history_id".into())) });
    }
    let scope = ResourceScope::class(query.class, query.class_id)?.with_object(query.name, query.object_id)?;
    let at = query.at.as_deref().map(parse_timestamp).transpose()?;
    render_page(tokens, &services.gateway().historical_view(scope, HistoricalQuery::Relations { at, options }, page, PageSelection::from_all(query.all))?)
});

#[derive(Clone, Default, CommandArgs)]
struct HistoricalComputed {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    class: Option<String>,
    #[option(long = "class-id", help = "Positive class ID")]
    class_id: Option<ClassId>,
    #[option(
        long = "name",
        help = "Object name",
        autocomplete = "objects_from_class"
    )]
    name: Option<String>,
    #[option(long = "object-id", help = "Positive object ID")]
    object_id: Option<ObjectId>,
    #[option(
        long = "at",
        help = "As-of time: RFC3339, UTC date/time, or time ago (e.g. 1d3h)",
        autocomplete = "history_times"
    )]
    at: Option<String>,
}

impl CliCommand for HistoricalComputed {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let scope = ResourceScope::class(query.class, query.class_id)?
            .with_object(query.name, query.object_id)?
            .require_object()?;
        let at = parse_timestamp(&required_option(query.at, "at")?)?;
        let record = services.gateway().historical_computed(scope, at)?;
        if has_pipeline()? {
            append_json(&record)
        } else {
            render_json_record(tokens, &record)
        }
    }
}

fn object_options(
    sort: Option<&str>,
    revision: Option<i64>,
) -> Result<ObjectHistoryOptions, AppError> {
    let mut options = ObjectHistoryOptions::default();
    if let Some(revision) = revision {
        options = options.revision(ResourceRevision::new(revision)?);
    }
    if let Some(sort) = sort {
        let (field, direction) = sort
            .strip_prefix('-')
            .map_or((sort, SortDirection::Asc), |field| {
                (field, SortDirection::Desc)
            });
        options =
            options.sort(
                match field {
                    "history_id" => HistorySortField::HistoryId,
                    "revision" => HistorySortField::Revision,
                    _ => return Err(AppError::InvalidOption(
                        "History sort must be history_id or revision, optionally prefixed with '-'"
                            .into(),
                    )),
                },
                direction,
            );
    }
    Ok(options)
}

fn render_page(tokens: &CommandTokenizer, page: &HistoryPageRecord) -> Result<(), AppError> {
    let rows = page.rows()?;
    warn_on_partial_pipeline(&rows)?;
    if matches!(response_format(tokens)?, OutputFormat::Json) {
        append_json(page)?;
    } else {
        let context = page.context();
        if context.as_object().is_some_and(|object| !object.is_empty()) {
            append_line(format!("Historical context: {context}"))?;
        }
        if let Some(relations) = page.relation_rows() {
            relations.format_noreturn()?;
        } else {
            rows.items.format_noreturn()?;
        }
    }
    append_paging_footer(tokens, &rows)
}

fn render_timeline_page(
    tokens: &CommandTokenizer,
    page: &HistoryPageRecord,
) -> Result<(), AppError> {
    let rows = page.rows()?;
    warn_on_partial_pipeline(&rows)?;
    if matches!(response_format(tokens)?, OutputFormat::Json) {
        append_json(page)?;
    } else {
        page.timeline_rows()?.format_noreturn()?;
    }
    append_paging_footer(tokens, &rows)
}
