//! Validated list-query grammar and its intentional Hubuum SDK filter integration.
//! No application configuration, rendering, network access, or application errors live here.

use chrono::{DateTime, NaiveDateTime};
use hubuum_client::{types::SortDirection, FilterOperator, QueryFilter};
use serde::{Deserialize, Serialize};
use suggestions::did_you_mean_message;
use thiserror::Error;

mod related;
pub use related::{RelatedObjectFilters, RELATED_OBJECT_FILTER_FIELDS};

pub mod suggestions;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("{0}")]
    ParseError(String),
    #[error("{0}")]
    InvalidOption(String),
}

pub const SERVER_MAX_PAGE_SIZE: usize = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "usize", into = "usize")]
pub struct ServerPageSize {
    requested: usize,
    effective: usize,
}

impl ServerPageSize {
    pub fn from_requested(requested: usize) -> Result<Self, QueryError> {
        if requested == 0 {
            return Err(QueryError::InvalidOption("--limit must be positive".into()));
        }
        Ok(Self {
            requested,
            effective: if requested > SERVER_MAX_PAGE_SIZE {
                SERVER_MAX_PAGE_SIZE
            } else {
                requested
            },
        })
    }

    pub const fn requested(self) -> usize {
        self.requested
    }

    pub const fn effective(self) -> usize {
        self.effective
    }

    pub const fn was_truncated(self) -> bool {
        self.requested != self.effective
    }
}

impl TryFrom<usize> for ServerPageSize {
    type Error = QueryError;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::from_requested(value)
    }
}

impl From<ServerPageSize> for usize {
    fn from(value: ServerPageSize) -> Self {
        value.effective()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PageSelection {
    #[default]
    Single,
    All,
}

impl PageSelection {
    pub const fn from_all(all: bool) -> Self {
        if all {
            Self::All
        } else {
            Self::Single
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Raw syntax; validate against the endpoint field specifications before building SDK filters.
pub struct FilterClause {
    pub field: String,
    pub operator: FilterOperator,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Raw syntax; validate against the endpoint sort specifications before executing a query.
pub struct SortClause {
    pub field: String,
    pub direction: SortDirectionArg,
}

#[derive(Debug, Clone, Copy)]
pub struct FilterFieldSpec {
    public_name: &'static str,
    backend_field: &'static str,
    operator_profile: FilterOperatorProfile,
    value_profile: FilterValueProfile,
    json_root: bool,
    resolver: FilterValueResolver,
}

#[derive(Debug, Clone, Copy)]
pub struct SortFieldSpec {
    public_name: &'static str,
    backend_field: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOperatorProfile {
    String,
    NumericOrDate,
    Boolean,
    EqualityOnly,
    /// Non-negated, case-sensitive equals, as required by task discovery.
    EqualsOnly,
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
    CollectionNameToId,
    ClassNameToId,
}

#[derive(Debug, Clone)]
pub struct ValidatedFilterClause {
    spec: FilterFieldSpec,
    operator: FilterOperator,
    value: String,
    json_path: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ValidatedSortClause {
    spec: SortFieldSpec,
    direction: SortDirectionArg,
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

pub fn parse_where_clause(clause: &str) -> Result<FilterClause, QueryError> {
    let mut parts = clause.trim().splitn(3, char::is_whitespace);
    let field = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| QueryError::ParseError("Filter clause requires a field".to_string()))?;
    let operator = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| QueryError::ParseError("Filter clause requires an operator".to_string()))?;
    let value = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| QueryError::ParseError("Filter clause requires a value".to_string()))?;

    Ok(FilterClause {
        field: field.to_string(),
        operator: parse_filter_operator(operator)?,
        value: value.to_string(),
    })
}

pub fn validate_filter_clauses(
    clauses: &[FilterClause],
    specs: &[FilterFieldSpec],
) -> Result<Vec<ValidatedFilterClause>, QueryError> {
    clauses
        .iter()
        .map(|clause| validate_filter_clause(clause, specs))
        .collect()
}

pub fn parse_sort_clause(clause: &str) -> Result<SortClause, QueryError> {
    let mut parts = clause.trim().splitn(2, char::is_whitespace);
    let field = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| QueryError::ParseError("Sort clause requires a field".to_string()))?;
    let direction = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| QueryError::ParseError("Sort clause requires a direction".to_string()))?;

    Ok(SortClause {
        field: field.to_string(),
        direction: parse_sort_direction(direction)?,
    })
}

pub fn validate_sort_clauses(
    clauses: &[SortClause],
    specs: &[SortFieldSpec],
) -> Result<Vec<ValidatedSortClause>, QueryError> {
    clauses
        .iter()
        .map(|clause| validate_sort_clause(clause, specs))
        .collect()
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
        FilterOperatorProfile::EqualsOnly => &["equals"],
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

impl ValidatedFilterClause {
    pub fn spec(&self) -> &FilterFieldSpec {
        &self.spec
    }
    pub fn value(&self) -> &str {
        &self.value
    }
    pub fn json_path(&self) -> &[String] {
        &self.json_path
    }

    /// Build the SDK filter after resolving a domain name, without mutating the validated clause.
    pub fn resolve_value<E>(
        &self,
        resolve: impl FnOnce(FilterValueResolver, &str) -> Result<String, E>,
    ) -> Result<QueryFilter, E> {
        let value = resolve(self.spec.resolver, &self.value)?;
        let value = if self.json_path.is_empty() {
            value
        } else {
            format!("{}={value}", self.json_path.join(","))
        };
        Ok(QueryFilter::filter(
            self.spec.backend_field,
            self.operator.clone(),
            value,
        ))
    }
}

impl ValidatedSortClause {
    pub fn spec(&self) -> &SortFieldSpec {
        &self.spec
    }
    pub const fn direction(&self) -> SortDirectionArg {
        self.direction
    }
}

fn validate_filter_clause(
    clause: &FilterClause,
    specs: &[FilterFieldSpec],
) -> Result<ValidatedFilterClause, QueryError> {
    let (spec, json_path) = resolve_filter_field_spec(specs, &clause.field).ok_or_else(|| {
        QueryError::ParseError(unknown_filter_field_message(&clause.field, specs))
    })?;

    if !operator_is_allowed(&clause.operator, spec.operator_profile) {
        let operators = completion_operators(spec.operator_profile).join(", ");
        return Err(QueryError::ParseError(format!(
            "Operator '{}' is not supported for field '{}'. Use: {}",
            clause.operator, spec.public_name, operators
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
) -> Result<ValidatedSortClause, QueryError> {
    let spec = resolve_sort_field_spec(specs, &clause.field)
        .ok_or_else(|| QueryError::ParseError(unknown_sort_field_message(&clause.field, specs)))?;

    Ok(ValidatedSortClause {
        spec,
        direction: clause.direction,
    })
}

fn unknown_filter_field_message(field: &str, specs: &[FilterFieldSpec]) -> String {
    let candidates = specs
        .iter()
        .map(|spec| spec.public_name.to_string())
        .collect::<Vec<_>>();
    unknown_value_message("Unknown filter field", field, candidates)
}

fn unknown_sort_field_message(field: &str, specs: &[SortFieldSpec]) -> String {
    let candidates = specs
        .iter()
        .map(|spec| spec.public_name.to_string())
        .collect::<Vec<_>>();
    unknown_value_message("Unknown sort field", field, candidates)
}

fn unknown_value_message(label: &str, value: &str, candidates: Vec<String>) -> String {
    match did_you_mean_message(value, candidates) {
        Some(hint) => format!("{label}: {value}. {hint}"),
        None => format!("{label}: {value}"),
    }
}

fn operator_is_allowed(operator: &FilterOperator, profile: FilterOperatorProfile) -> bool {
    match profile {
        FilterOperatorProfile::Any => !matches!(operator, FilterOperator::Raw),
        FilterOperatorProfile::EqualsOnly => {
            matches!(operator, FilterOperator::Equals { is_negated: false })
        }
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
) -> Result<(), QueryError> {
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

fn split_between(value: &str) -> Result<(&str, &str), QueryError> {
    value.split_once(',').ok_or_else(|| {
        QueryError::ParseError("between filters require values in the form low,high".to_string())
    })
}

fn parse_datetime(value: &str) -> Result<(), QueryError> {
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

fn invalid_value(expected: &str, value: &str) -> QueryError {
    QueryError::ParseError(format!("Invalid {expected} value: {value}"))
}

const FILTER_OPERATOR_NAMES: &[&str] = &[
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
];

fn parse_filter_operator(operator: &str) -> Result<FilterOperator, QueryError> {
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
        _ => Err(QueryError::ParseError(unknown_value_message(
            "Unknown filter operator",
            operator,
            FILTER_OPERATOR_NAMES
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        ))),
    }
}

fn parse_sort_direction(direction: &str) -> Result<SortDirectionArg, QueryError> {
    match direction {
        "asc" => Ok(SortDirectionArg::Asc),
        "desc" => Ok(SortDirectionArg::Desc),
        _ => Err(QueryError::ParseError(unknown_value_message(
            "Unknown sort direction",
            direction,
            completion_sort_directions()
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        ))),
    }
}

impl FilterFieldSpec {
    pub const fn public_name(&self) -> &'static str {
        self.public_name
    }
    pub const fn backend_field(&self) -> &'static str {
        self.backend_field
    }
    pub const fn operator_profile(&self) -> FilterOperatorProfile {
        self.operator_profile
    }
    pub const fn value_profile(&self) -> FilterValueProfile {
        self.value_profile
    }
    pub const fn is_json_root(&self) -> bool {
        self.json_root
    }
}
impl SortFieldSpec {
    pub const fn public_name(&self) -> &'static str {
        self.public_name
    }
    pub const fn backend_field(&self) -> &'static str {
        self.backend_field
    }
}

#[cfg(test)]
mod tests;
