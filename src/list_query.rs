use chrono::{DateTime, NaiveDateTime};
use hubuum_client::{
    client::{sync::CursorRequest, sync::QueryOp, Page},
    types::SortDirection,
    FilterOperator, QueryFilter,
};
use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::{append_line, set_next_page_command};
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListQuery {
    pub filters: Vec<FilterClause>,
    pub sorts: Vec<SortClause>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterClause {
    pub field: String,
    pub operator: FilterOperator,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SortClause {
    pub field: String,
    pub direction: SortDirectionArg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub limit: Option<usize>,
    pub returned_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct FilterFieldSpec {
    pub public_name: &'static str,
    pub backend_field: &'static str,
    pub operator_profile: FilterOperatorProfile,
    pub value_profile: FilterValueProfile,
    pub json_root: bool,
    pub resolver: FilterValueResolver,
}

#[derive(Debug, Clone, Copy)]
pub struct SortFieldSpec {
    pub public_name: &'static str,
    pub backend_field: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOperatorProfile {
    String,
    NumericOrDate,
    Boolean,
    EqualityOnly,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterValueProfile {
    String,
    Integer,
    DateTime,
    Boolean,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterValueResolver {
    None,
    NamespaceNameToId,
}

#[derive(Debug, Clone)]
pub struct ValidatedFilterClause {
    pub spec: FilterFieldSpec,
    pub operator: FilterOperator,
    pub value: String,
    pub json_path: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ValidatedSortClause {
    pub spec: SortFieldSpec,
    pub direction: SortDirectionArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirectionArg {
    Asc,
    Desc,
}

impl FilterFieldSpec {
    pub const fn new(
        public_name: &'static str,
        backend_field: &'static str,
        operator_profile: FilterOperatorProfile,
        value_profile: FilterValueProfile,
    ) -> Self {
        Self {
            public_name,
            backend_field,
            operator_profile,
            value_profile,
            json_root: false,
            resolver: FilterValueResolver::None,
        }
    }

    pub const fn json_root(mut self) -> Self {
        self.json_root = true;
        self
    }

    pub const fn resolver(mut self, resolver: FilterValueResolver) -> Self {
        self.resolver = resolver;
        self
    }
}

impl SortFieldSpec {
    pub const fn new(public_name: &'static str, backend_field: &'static str) -> Self {
        Self {
            public_name,
            backend_field,
        }
    }
}

impl SortDirectionArg {
    pub const fn to_api(self) -> SortDirection {
        match self {
            Self::Asc => SortDirection::Asc,
            Self::Desc => SortDirection::Desc,
        }
    }
}

impl<T> PagedResult<T> {
    pub fn from_page<U, F>(page: Page<U>, limit: Option<usize>, map: F) -> Self
    where
        F: Fn(U) -> T,
    {
        let items = page.items.into_iter().map(map).collect::<Vec<_>>();
        let returned_count = items.len();
        Self {
            items,
            next_cursor: page.next_cursor,
            limit,
            returned_count,
        }
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
        limit,
        cursor,
    })
}

pub fn parse_where_clause(clause: &str) -> Result<FilterClause, AppError> {
    let mut parts = clause.trim().splitn(3, char::is_whitespace);
    let field = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ParseError("Filter clause requires a field".to_string()))?;
    let operator = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ParseError("Filter clause requires an operator".to_string()))?;
    let value = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ParseError("Filter clause requires a value".to_string()))?;

    Ok(FilterClause {
        field: field.to_string(),
        operator: parse_filter_operator(operator)?,
        value: value.to_string(),
    })
}

pub fn validate_filter_clauses(
    clauses: &[FilterClause],
    specs: &[FilterFieldSpec],
) -> Result<Vec<ValidatedFilterClause>, AppError> {
    clauses
        .iter()
        .map(|clause| validate_filter_clause(clause, specs))
        .collect()
}

pub fn parse_sort_clause(clause: &str) -> Result<SortClause, AppError> {
    let mut parts = clause.trim().splitn(2, char::is_whitespace);
    let field = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ParseError("Sort clause requires a field".to_string()))?;
    let direction = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ParseError("Sort clause requires a direction".to_string()))?;

    Ok(SortClause {
        field: field.to_string(),
        direction: parse_sort_direction(direction)?,
    })
}

pub fn validate_sort_clauses(
    clauses: &[SortClause],
    specs: &[SortFieldSpec],
) -> Result<Vec<ValidatedSortClause>, AppError> {
    clauses
        .iter()
        .map(|clause| validate_sort_clause(clause, specs))
        .collect()
}

pub fn render_paged_result<T>(
    tokens: &CommandTokenizer,
    paged: &PagedResult<T>,
    format: OutputFormat,
) -> Result<(), AppError>
where
    T: serde::Serialize + Clone + crate::formatting::TableRenderable,
{
    match format {
        OutputFormat::Json => {
            if should_wrap_paged_json(tokens, paged) {
                append_line(serde_json::to_string_pretty(paged)?)?;
            } else {
                append_line(serde_json::to_string_pretty(&paged.items)?)?;
            }
        }
        OutputFormat::Text => {
            paged.items.format_noreturn()?;
            append_paging_footer(tokens, paged)?;
        }
    }
    Ok(())
}

pub fn apply_query_paging<T>(
    query: QueryOp<T>,
    list_query: &ListQuery,
    sorts: &[ValidatedSortClause],
) -> QueryOp<T>
where
    T: hubuum_client::ApiResource,
{
    let query = if sorts.is_empty() {
        query
    } else {
        query.sort_by_fields(
            sorts
                .iter()
                .map(|clause| (clause.spec.backend_field, clause.direction.to_api()))
                .collect::<Vec<_>>(),
        )
    };

    let query = if let Some(limit) = list_query.limit {
        query.limit(limit)
    } else {
        query
    };

    if let Some(cursor) = &list_query.cursor {
        query.cursor(cursor)
    } else {
        query
    }
}

pub fn resolve_filter_field_spec(
    specs: &[FilterFieldSpec],
    field: &str,
) -> Option<(FilterFieldSpec, Vec<String>)> {
    specs.iter().find_map(|spec| match field {
        value if value == spec.public_name => Some((*spec, Vec::new())),
        value if spec.json_root && value.starts_with(&format!("{}.", spec.public_name)) => {
            let path = value
                .trim_start_matches(spec.public_name)
                .trim_start_matches('.')
                .split('.')
                .map(str::to_string)
                .collect::<Vec<_>>();
            Some((*spec, path))
        }
        _ => None,
    })
}

pub fn completion_operators(profile: FilterOperatorProfile) -> &'static [&'static str] {
    match profile {
        FilterOperatorProfile::String => &[
            "equals",
            "not_equals",
            "iequals",
            "not_iequals",
            "contains",
            "not_contains",
            "icontains",
            "not_icontains",
            "startswith",
            "not_startswith",
            "istartswith",
            "not_istartswith",
            "endswith",
            "not_endswith",
            "iendswith",
            "not_iendswith",
            "like",
            "not_like",
            "regex",
            "not_regex",
        ],
        FilterOperatorProfile::NumericOrDate => &[
            "equals",
            "not_equals",
            "gt",
            "not_gt",
            "gte",
            "not_gte",
            "lt",
            "not_lt",
            "lte",
            "not_lte",
            "between",
            "not_between",
        ],
        FilterOperatorProfile::Boolean => &["equals", "not_equals"],
        FilterOperatorProfile::EqualityOnly => &["equals", "not_equals", "iequals", "not_iequals"],
        FilterOperatorProfile::Any => &[
            "equals",
            "not_equals",
            "iequals",
            "not_iequals",
            "contains",
            "not_contains",
            "icontains",
            "not_icontains",
            "startswith",
            "not_startswith",
            "istartswith",
            "not_istartswith",
            "endswith",
            "not_endswith",
            "iendswith",
            "not_iendswith",
            "like",
            "not_like",
            "regex",
            "not_regex",
            "gt",
            "not_gt",
            "gte",
            "not_gte",
            "lt",
            "not_lt",
            "lte",
            "not_lte",
            "between",
            "not_between",
        ],
    }
}

pub const fn completion_sort_directions() -> &'static [&'static str] {
    &["asc", "desc"]
}

pub fn apply_cursor_request_paging<T>(
    request: CursorRequest<T>,
    list_query: &ListQuery,
    sorts: &[ValidatedSortClause],
) -> CursorRequest<T> {
    let request = if sorts.is_empty() {
        request
    } else {
        request.sort_by_fields(
            sorts
                .iter()
                .map(|clause| (clause.spec.backend_field, clause.direction.to_api()))
                .collect::<Vec<_>>(),
        )
    };

    let request = if let Some(limit) = list_query.limit {
        request.limit(limit)
    } else {
        request
    };

    if let Some(cursor) = &list_query.cursor {
        request.cursor(cursor)
    } else {
        request
    }
}

pub fn filter_clause(
    field: impl Into<String>,
    operator: FilterOperator,
    value: impl Into<String>,
) -> FilterClause {
    FilterClause {
        field: field.into(),
        operator,
        value: value.into(),
    }
}

pub fn resolve_sort_field_spec(specs: &[SortFieldSpec], field: &str) -> Option<SortFieldSpec> {
    specs.iter().copied().find(|spec| spec.public_name == field)
}

pub fn validated_clause_to_query_filter(clause: &ValidatedFilterClause) -> QueryFilter {
    if clause.json_path.is_empty() {
        QueryFilter::filter(
            clause.spec.backend_field,
            clause.operator.clone(),
            clause.value.clone(),
        )
    } else {
        let path = clause.json_path.join(",");
        QueryFilter::filter(
            clause.spec.backend_field,
            clause.operator.clone(),
            format!("{path}={}", clause.value),
        )
    }
}

fn validate_filter_clause(
    clause: &FilterClause,
    specs: &[FilterFieldSpec],
) -> Result<ValidatedFilterClause, AppError> {
    let (spec, json_path) = resolve_filter_field_spec(specs, &clause.field)
        .ok_or_else(|| AppError::ParseError(format!("Unknown filter field: {}", clause.field)))?;

    if !operator_is_allowed(&clause.operator, spec.operator_profile) {
        return Err(AppError::ParseError(format!(
            "Operator '{}' is not supported for field '{}'",
            clause.operator, spec.public_name
        )));
    }

    validate_value(spec.value_profile, clause.operator.clone(), &clause.value)?;

    Ok(ValidatedFilterClause {
        spec,
        operator: clause.operator.clone(),
        value: clause.value.clone(),
        json_path,
    })
}

fn validate_sort_clause(
    clause: &SortClause,
    specs: &[SortFieldSpec],
) -> Result<ValidatedSortClause, AppError> {
    let spec = resolve_sort_field_spec(specs, &clause.field)
        .ok_or_else(|| AppError::ParseError(format!("Unknown sort field: {}", clause.field)))?;

    Ok(ValidatedSortClause {
        spec,
        direction: clause.direction,
    })
}

fn operator_is_allowed(operator: &FilterOperator, profile: FilterOperatorProfile) -> bool {
    match profile {
        FilterOperatorProfile::Any => !matches!(operator, FilterOperator::Raw),
        FilterOperatorProfile::EqualityOnly => matches!(
            operator,
            FilterOperator::Equals { .. } | FilterOperator::IEquals { .. }
        ),
        FilterOperatorProfile::Boolean => matches!(
            operator,
            FilterOperator::Equals { .. } | FilterOperator::IEquals { .. }
        ),
        FilterOperatorProfile::String => matches!(
            operator,
            FilterOperator::Equals { .. }
                | FilterOperator::IEquals { .. }
                | FilterOperator::Contains { .. }
                | FilterOperator::IContains { .. }
                | FilterOperator::StartsWith { .. }
                | FilterOperator::IStartsWith { .. }
                | FilterOperator::EndsWith { .. }
                | FilterOperator::IEndsWith { .. }
                | FilterOperator::Like { .. }
                | FilterOperator::Regex { .. }
        ),
        FilterOperatorProfile::NumericOrDate => matches!(
            operator,
            FilterOperator::Equals { .. }
                | FilterOperator::IEquals { .. }
                | FilterOperator::Gt { .. }
                | FilterOperator::Gte { .. }
                | FilterOperator::Lt { .. }
                | FilterOperator::Lte { .. }
                | FilterOperator::Between { .. }
        ),
    }
}

fn validate_value(
    profile: FilterValueProfile,
    operator: FilterOperator,
    value: &str,
) -> Result<(), AppError> {
    match profile {
        FilterValueProfile::Any | FilterValueProfile::String => Ok(()),
        FilterValueProfile::Integer => {
            if matches!(operator, FilterOperator::Between { .. }) {
                let (low, high) = split_between(value)?;
                low.parse::<i64>()
                    .map_err(|_| invalid_value("integer", value))?;
                high.parse::<i64>()
                    .map_err(|_| invalid_value("integer", value))?;
            } else {
                value
                    .parse::<i64>()
                    .map_err(|_| invalid_value("integer", value))?;
            }
            Ok(())
        }
        FilterValueProfile::Boolean => {
            value
                .parse::<bool>()
                .map_err(|_| invalid_value("bool", value))?;
            Ok(())
        }
        FilterValueProfile::DateTime => {
            if matches!(operator, FilterOperator::Between { .. }) {
                let (low, high) = split_between(value)?;
                parse_datetime(low)?;
                parse_datetime(high)?;
            } else {
                parse_datetime(value)?;
            }
            Ok(())
        }
    }
}

fn split_between(value: &str) -> Result<(&str, &str), AppError> {
    value.split_once(',').ok_or_else(|| {
        AppError::ParseError("between filters require values in the form low,high".to_string())
    })
}

fn parse_datetime(value: &str) -> Result<(), AppError> {
    if DateTime::parse_from_rfc3339(value).is_ok() {
        return Ok(());
    }
    const FORMATS: &[&str] = &["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"];
    if FORMATS
        .iter()
        .any(|format| NaiveDateTime::parse_from_str(value, format).is_ok())
    {
        return Ok(());
    }

    if NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").is_ok() {
        return Ok(());
    }

    Err(invalid_value("date-time", value))
}

fn invalid_value(expected: &str, value: &str) -> AppError {
    AppError::ParseError(format!("Invalid {expected} value: {value}"))
}

fn parse_filter_operator(operator: &str) -> Result<FilterOperator, AppError> {
    match operator {
        "=" | "equals" => Ok(FilterOperator::Equals { is_negated: false }),
        "!=" | "not_equals" => Ok(FilterOperator::Equals { is_negated: true }),
        "iequals" => Ok(FilterOperator::IEquals { is_negated: false }),
        "not_iequals" => Ok(FilterOperator::IEquals { is_negated: true }),
        "contains" => Ok(FilterOperator::Contains { is_negated: false }),
        "not_contains" => Ok(FilterOperator::Contains { is_negated: true }),
        "icontains" => Ok(FilterOperator::IContains { is_negated: false }),
        "not_icontains" => Ok(FilterOperator::IContains { is_negated: true }),
        "startswith" => Ok(FilterOperator::StartsWith { is_negated: false }),
        "not_startswith" => Ok(FilterOperator::StartsWith { is_negated: true }),
        "istartswith" => Ok(FilterOperator::IStartsWith { is_negated: false }),
        "not_istartswith" => Ok(FilterOperator::IStartsWith { is_negated: true }),
        "endswith" => Ok(FilterOperator::EndsWith { is_negated: false }),
        "not_endswith" => Ok(FilterOperator::EndsWith { is_negated: true }),
        "iendswith" => Ok(FilterOperator::IEndsWith { is_negated: false }),
        "not_iendswith" => Ok(FilterOperator::IEndsWith { is_negated: true }),
        "like" => Ok(FilterOperator::Like { is_negated: false }),
        "not_like" => Ok(FilterOperator::Like { is_negated: true }),
        "regex" => Ok(FilterOperator::Regex { is_negated: false }),
        "not_regex" => Ok(FilterOperator::Regex { is_negated: true }),
        ">" | "gt" => Ok(FilterOperator::Gt { is_negated: false }),
        "not_gt" => Ok(FilterOperator::Gt { is_negated: true }),
        ">=" | "gte" => Ok(FilterOperator::Gte { is_negated: false }),
        "not_gte" => Ok(FilterOperator::Gte { is_negated: true }),
        "<" | "lt" => Ok(FilterOperator::Lt { is_negated: false }),
        "not_lt" => Ok(FilterOperator::Lt { is_negated: true }),
        "<=" | "lte" => Ok(FilterOperator::Lte { is_negated: false }),
        "not_lte" => Ok(FilterOperator::Lte { is_negated: true }),
        "between" => Ok(FilterOperator::Between { is_negated: false }),
        "not_between" => Ok(FilterOperator::Between { is_negated: true }),
        _ => Err(AppError::ParseError(format!(
            "Unknown filter operator: {operator}"
        ))),
    }
}

fn parse_sort_direction(direction: &str) -> Result<SortDirectionArg, AppError> {
    match direction {
        "asc" => Ok(SortDirectionArg::Asc),
        "desc" => Ok(SortDirectionArg::Desc),
        _ => Err(AppError::ParseError(format!(
            "Unknown sort direction: {direction}"
        ))),
    }
}

fn append_paging_footer<T>(
    tokens: &CommandTokenizer,
    paged: &PagedResult<T>,
) -> Result<(), AppError> {
    if paged.limit.is_none() && paged.next_cursor.is_none() {
        return Ok(());
    }

    append_line(format!(
        "Returned {} item(s){}",
        paged.returned_count,
        paged
            .limit
            .map(|limit| format!(" (limit: {limit})"))
            .unwrap_or_default()
    ))?;
    if let Some(next_cursor) = &paged.next_cursor {
        let next_command = next_cursor_command(tokens, next_cursor);
        set_next_page_command(next_command)?;
        if crate::config::get_config().repl.enter_fetches_next_page {
            append_line(
                "Paginated results available. Press Enter for the next page, or Ctrl-C to stop.",
            )?;
        } else {
            append_line("Paginated results available. Type 'next' for the next page.")?;
        }
    }
    Ok(())
}

fn should_wrap_paged_json<T>(tokens: &CommandTokenizer, paged: &PagedResult<T>) -> bool {
    tokens.get_options().contains_key("limit")
        || tokens.get_options().contains_key("cursor")
        || paged.next_cursor.is_some()
}

fn next_cursor_command(tokens: &CommandTokenizer, cursor: &str) -> String {
    let mut rebuilt = Vec::new();
    let mut skip_next = false;

    for token in tokens.raw_tokens() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if token == "--cursor" {
            skip_next = true;
            continue;
        }
        if token.starts_with("--cursor=") {
            continue;
        }
        rebuilt.push(shell_escape(token));
    }

    rebuilt.push("--cursor".to_string());
    rebuilt.push(shell_escape(cursor));
    rebuilt.join(" ")
}

fn shell_escape(token: &str) -> String {
    if token.is_empty() {
        return "''".to_string();
    }

    if token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        token.to_string()
    } else {
        format!("'{}'", token.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use serde::Serialize;
    use serial_test::serial;

    use super::{
        completion_operators, filter_clause, list_query_from_raw, resolve_filter_field_spec,
        validate_filter_clauses, validate_sort_clauses, FilterFieldSpec, FilterOperatorProfile,
        FilterValueProfile, PagedResult, SortDirectionArg, SortFieldSpec,
    };
    use crate::config::{init_config, AppConfig};
    use crate::models::OutputFormat;
    use crate::output::{reset_output, take_output};
    use crate::tokenizer::CommandTokenizer;

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
        assert_eq!(query.limit, Some(10));
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
    fn validates_json_root_fields() {
        let specs = [FilterFieldSpec::new(
            "data",
            "data",
            FilterOperatorProfile::Any,
            FilterValueProfile::Any,
        )
        .json_root()];
        let clauses = vec![filter_clause(
            "data.owner.email",
            hubuum_client::FilterOperator::IContains { is_negated: false },
            "alice",
        )];

        let validated =
            validate_filter_clauses(&clauses, &specs).expect("json root should validate");
        assert_eq!(validated[0].json_path, vec!["owner", "email"]);
    }

    #[test]
    fn rejects_unsupported_operators() {
        let specs = [FilterFieldSpec::new(
            "validate_schema",
            "validate_schema",
            FilterOperatorProfile::Boolean,
            FilterValueProfile::Boolean,
        )];
        let err = validate_filter_clauses(
            &[filter_clause(
                "validate_schema",
                hubuum_client::FilterOperator::Contains { is_negated: false },
                "true",
            )],
            &specs,
        )
        .expect_err("invalid boolean operator should fail");

        assert!(err.to_string().contains("not supported"));
    }

    #[test]
    fn rejects_unknown_sort_fields() {
        let specs = [SortFieldSpec::new("name", "name")];
        let err = validate_sort_clauses(
            &[super::SortClause {
                field: "unknown".to_string(),
                direction: SortDirectionArg::Asc,
            }],
            &specs,
        )
        .expect_err("unknown sort field should fail");

        assert!(err.to_string().contains("Unknown sort field"));
    }

    #[test]
    fn resolves_json_root_specs_for_completion() {
        let specs = [FilterFieldSpec::new(
            "data",
            "data",
            FilterOperatorProfile::Any,
            FilterValueProfile::Any,
        )
        .json_root()];

        let (spec, json_path) =
            resolve_filter_field_spec(&specs, "data.owner.email").expect("json field should match");
        assert_eq!(spec.public_name, "data");
        assert_eq!(json_path, vec!["owner", "email"]);
    }

    #[test]
    fn completion_operator_profiles_expose_expected_words() {
        assert!(completion_operators(FilterOperatorProfile::String).contains(&"icontains"));
        assert!(completion_operators(FilterOperatorProfile::Boolean).contains(&"equals"));
        assert!(!completion_operators(FilterOperatorProfile::Boolean).contains(&"regex"));
    }

    #[test]
    fn paged_json_wraps_when_cursor_or_limit_exists() {
        let tokens = CommandTokenizer::new("class list --limit 10", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![1, 2],
            next_cursor: None,
            limit: Some(10),
            returned_count: 2,
        };

        assert!(super::should_wrap_paged_json(&tokens, &paged));
    }

    #[derive(Clone, Serialize)]
    struct DummyRow {
        id: i32,
    }

    impl crate::formatting::TableRenderable for DummyRow {
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
        reset_output().expect("output should reset");
        let tokens = CommandTokenizer::new("class list --limit 1", "list", &[])
            .expect("tokenization should succeed");
        let paged = PagedResult {
            items: vec![DummyRow { id: 1 }],
            next_cursor: Some("abc123".to_string()),
            limit: Some(1),
            returned_count: 1,
        };

        super::render_paged_result(&tokens, &paged, OutputFormat::Text)
            .expect("text output should render");

        let snapshot = take_output().expect("snapshot should be captured");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Paginated results available.")));
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("Type 'next' for the next page.")));
        assert_eq!(
            snapshot.next_page_command.as_deref(),
            Some("class list --limit 1 --cursor abc123")
        );
    }
}
