//! Validated Hubuum structured resource-search requests, independent of transport.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_vec, Value};
use thiserror::Error;

mod predicate;

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
#[error("Invalid structured search: {0}")]
pub struct SearchError(String);

fn require(condition: bool, message: impl Into<String>) -> Result<(), SearchError> {
    if condition {
        Ok(())
    } else {
        Err(SearchError(message.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Collection,
    Class,
    Object,
    AuditEvent,
    User,
    Group,
    ServiceAccount,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Class => "class",
            Self::Object => "object",
            Self::AuditEvent => "audit_event",
            Self::User => "user",
            Self::Group => "group",
            Self::ServiceAccount => "service_account",
        }
    }

    pub fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Collection => &[
                "id",
                "name",
                "description",
                "created_at",
                "updated_at",
                "revision",
            ],
            Self::Class => &[
                "id",
                "name",
                "description",
                "collection_id",
                "created_at",
                "updated_at",
                "revision",
                "validate_schema",
                "json_schema",
            ],
            Self::Object => &[
                "id",
                "name",
                "description",
                "collection_id",
                "created_at",
                "updated_at",
                "revision",
                "json_data",
            ],
            Self::AuditEvent => &[
                "id",
                "occurred_at",
                "entity_type",
                "entity_id",
                "entity_name",
                "collection_id",
                "action",
                "actor_kind",
                "actor_user_id",
                "initiator_user_id",
                "summary",
                "metadata",
            ],
            Self::User => &[
                "id",
                "name",
                "identity_scope",
                "proper_name",
                "email",
                "created_at",
                "updated_at",
                "revision",
            ],
            Self::Group => &[
                "id",
                "name",
                "description",
                "identity_scope",
                "managed_by",
                "external_key",
                "last_sync_attempted_at",
                "last_sync_success_at",
                "created_at",
                "updated_at",
                "revision",
            ],
            Self::ServiceAccount => &[
                "id",
                "name",
                "description",
                "identity_scope",
                "owner_group_id",
                "created_by",
                "disabled_at",
                "created_at",
                "updated_at",
                "revision",
            ],
        }
    }

    fn validate_field(self, field: &str, sort: bool) -> Result<(), SearchError> {
        require(
            self.fields().contains(&field),
            format!("field '{field}' is not available for {}", self.as_str()),
        )?;
        let sortable = match self {
            Self::AuditEvent => matches!(field, "id" | "occurred_at"),
            Self::Group => matches!(
                field,
                "id" | "name" | "description" | "created_at" | "updated_at" | "revision"
            ),
            Self::ServiceAccount => matches!(
                field,
                "id" | "name" | "identity_scope" | "created_at" | "updated_at" | "revision"
            ),
            _ => !matches!(field, "json_data" | "json_schema" | "validate_schema"),
        };
        require(
            !sort || sortable,
            format!("field '{field}' cannot sort {} results", self.as_str()),
        )
    }
}

/// Only validated construction and pagination changes are exposed.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SearchRequest(WireRequest);

#[derive(Debug, Clone, Default)]
pub struct PageOptions {
    limit: Option<usize>,
    cursor: Option<String>,
    include_total: Option<bool>,
}

impl PageOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }
    pub fn include_total(mut self, include: bool) -> Self {
        self.include_total = Some(include);
        self
    }
}

impl SearchRequest {
    pub fn new(kind: ResourceKind) -> Self {
        let target = match kind {
            ResourceKind::Collection => Target::Collection {},
            ResourceKind::Class => Target::Class {},
            ResourceKind::Object => Target::Object { class: None },
            ResourceKind::AuditEvent => Target::AuditEvent {},
            ResourceKind::User => Target::User {},
            ResourceKind::Group => Target::Group {},
            ResourceKind::ServiceAccount => Target::ServiceAccount {},
        };
        Self(WireRequest {
            version: 1,
            target,
            filter: None,
            sort: Vec::new(),
            limit: None,
            cursor: None,
            include_total: false,
        })
    }

    pub fn in_class(mut self, name: impl Into<String>) -> Result<Self, SearchError> {
        require(
            self.kind() == ResourceKind::Object,
            "--class requires an object target",
        )?;
        self.0.target = Target::Object {
            class: Some(ClassSelector::Name { name: name.into() }),
        };
        self.validate()?;
        Ok(self)
    }

    pub fn with_predicate(mut self, source: &str) -> Result<Self, SearchError> {
        require(
            source.len() <= MAX_REQUEST_BYTES,
            "predicate exceeds 64 KiB",
        )?;
        self.0.filter = Some(predicate::parse(source)?);
        self.validate()?;
        Ok(self)
    }

    pub fn sort_by(
        mut self,
        field: impl Into<String>,
        direction: SortDirection,
    ) -> Result<Self, SearchError> {
        self.0.sort.push(Sort {
            field: field.into(),
            direction,
        });
        self.validate()?;
        Ok(self)
    }
    pub fn parse(json: &str) -> Result<Self, SearchError> {
        require(json.len() <= MAX_REQUEST_BYTES, "request exceeds 64 KiB")?;
        let request = Self(from_str(json).map_err(|error| SearchError(error.to_string()))?);
        request.validate()?;
        Ok(request)
    }

    pub fn kind(&self) -> ResourceKind {
        self.0.target.kind()
    }
    pub fn cursor(&self) -> Option<&str> {
        self.0.cursor.as_deref()
    }

    pub fn with_page(mut self, options: PageOptions) -> Result<Self, SearchError> {
        if options.limit.is_some() {
            self.0.limit = options.limit;
        }
        if options.cursor.is_some() {
            self.0.cursor = options.cursor;
        }
        if let Some(include) = options.include_total {
            self.0.include_total = include;
        }
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<(), SearchError> {
        let request = &self.0;
        require(request.version == 1, "version must be 1")?;
        request.target.validate()?;
        if let Some(filter) = &request.filter {
            filter.validate(self.kind(), 1, &mut ExpressionBudget::default())?;
        }
        require(
            request.sort.len() <= 8,
            "at most eight sort fields are allowed",
        )?;
        let mut seen = HashSet::new();
        for sort in &request.sort {
            self.kind().validate_field(&sort.field, true)?;
            require(
                seen.insert(&sort.field),
                format!("duplicate sort field '{}'", sort.field),
            )?;
        }
        require(request.limit != Some(0), "limit must be greater than zero")?;
        require(
            request.cursor.as_ref().is_none_or(|c| !c.is_empty()),
            "cursor must not be empty",
        )?;
        require(
            to_vec(request)
                .map_err(|e| SearchError(e.to_string()))?
                .len()
                <= MAX_REQUEST_BYTES,
            "request with pagination exceeds 64 KiB",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    version: u8,
    target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    filter: Option<Expression>,
    #[serde(default)]
    sort: Vec<Sort>,
    limit: Option<usize>,
    cursor: Option<String>,
    #[serde(default)]
    include_total: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Collection {},
    Class {},
    Object {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<ClassSelector>,
    },
    AuditEvent {},
    User {},
    Group {},
    ServiceAccount {},
}

impl Target {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Collection {} => ResourceKind::Collection,
            Self::Class {} => ResourceKind::Class,
            Self::Object { .. } => ResourceKind::Object,
            Self::AuditEvent {} => ResourceKind::AuditEvent,
            Self::User {} => ResourceKind::User,
            Self::Group {} => ResourceKind::Group,
            Self::ServiceAccount {} => ResourceKind::ServiceAccount,
        }
    }
    fn validate(&self) -> Result<(), SearchError> {
        if let Self::Object { class: Some(class) } = self {
            class.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum ClassSelector {
    Id { id: i32 },
    Name { name: String },
}
impl ClassSelector {
    fn validate(&self) -> Result<(), SearchError> {
        match self {
            Self::Id { id } => require(*id > 0, "class ID must be positive"),
            Self::Name { name } => require(!name.trim().is_empty(), "class name must not be empty"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Expression {
    And { args: Vec<Expression> },
    Or { args: Vec<Expression> },
    Not { arg: Box<Expression> },
    Field { predicate: Predicate },
    Related { predicate: Related },
}
#[derive(Default)]
struct ExpressionBudget {
    nodes: usize,
    fields: usize,
    related: usize,
}
impl Expression {
    fn validate(
        &self,
        kind: ResourceKind,
        depth: usize,
        budget: &mut ExpressionBudget,
    ) -> Result<(), SearchError> {
        budget.nodes += 1;
        require(
            depth <= 8 && budget.nodes <= 64,
            "filter is limited to depth 8 and 64 nodes",
        )?;
        match self {
            Self::And { args } | Self::Or { args } => {
                require(args.len() >= 2, "and/or require at least two args")?;
                for arg in args {
                    arg.validate(kind, depth + 1, budget)?;
                }
            }
            Self::Not { arg } => arg.validate(kind, depth + 1, budget)?,
            Self::Field { predicate } => {
                budget.fields += 1;
                predicate.validate(kind)?;
            }
            Self::Related { predicate } => {
                require(
                    kind == ResourceKind::Object,
                    "related predicates require an object target",
                )?;
                budget.related += 1;
                budget.fields += predicate.filters.len();
                require(
                    budget.related <= 4,
                    "at most four related predicates are allowed",
                )?;
                predicate.class.validate()?;
                require(
                    (1..=10).contains(&predicate.depth),
                    "related depth must be from 1 through 10",
                )?;
                require(
                    predicate.filters.len() <= 16,
                    "at most 16 filters per related predicate are allowed",
                )?;
                for filter in &predicate.filters {
                    filter.validate(ResourceKind::Object)?;
                }
            }
        }
        require(
            budget.fields <= 32,
            "at most 32 field predicates are allowed",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Predicate {
    field: String,
    operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
}
impl Predicate {
    fn validate(&self, kind: ResourceKind) -> Result<(), SearchError> {
        kind.validate_field(&self.field, false)?;
        let json = matches!(
            self.field.as_str(),
            "json_data" | "json_schema" | "metadata"
        );
        if json {
            require(self.path.as_ref().is_some_and(|path| path.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$'))), "JSON fields require a dotted path with nonempty ASCII letter, digit, '_' or '$' segments")?;
        } else {
            require(self.path.is_none(), "only JSON fields accept path")?;
        }
        let operators: &[&str] = if json {
            &[
                "equals",
                "iequals",
                "contains",
                "icontains",
                "startswith",
                "istartswith",
                "endswith",
                "iendswith",
                "like",
                "regex",
                "gt",
                "gte",
                "lt",
                "lte",
                "between",
                "within_network",
                "contains_network",
                "contains_ip",
                "overlaps_network",
                "inet_equals",
                "in",
                "all",
                "array_length",
                "has_key",
                "is_null",
            ]
        } else if self.field == "validate_schema" {
            &["equals", "is_null"]
        } else if matches!(
            self.field.as_str(),
            "id" | "collection_id"
                | "revision"
                | "entity_id"
                | "actor_user_id"
                | "initiator_user_id"
                | "owner_group_id"
                | "created_by"
        ) || self.field.ends_with("_at")
        {
            &[
                "equals", "in", "gt", "gte", "lt", "lte", "between", "is_null",
            ]
        } else {
            &[
                "equals",
                "iequals",
                "contains",
                "icontains",
                "startswith",
                "istartswith",
                "endswith",
                "iendswith",
                "like",
                "regex",
                "in",
                "is_null",
            ]
        };
        require(
            operators.contains(&self.operator.as_str()),
            format!(
                "operator '{}' is not available for '{}'",
                self.operator, self.field
            ),
        )?;
        require(
            (self.operator == "is_null") == self.value.is_none(),
            "is_null takes no value; every other operator requires a non-null value",
        )?;
        if let Some(value) = &self.value {
            validate_value(value)?;
        }
        Ok(())
    }
}
fn validate_value(value: &Value) -> Result<(), SearchError> {
    match value {
        Value::String(s) => require(!s.is_empty(), "predicate value must not be empty"),
        Value::Number(_) | Value::Bool(_) => Ok(()),
        Value::Array(items) => {
            require(
                (1..=50).contains(&items.len()),
                "predicate arrays require 1 through 50 scalar values",
            )?;
            for item in items {
                require(
                    matches!(item, Value::String(_) | Value::Number(_) | Value::Bool(_)),
                    "predicate array entries must be non-null scalars",
                )?;
                require(
                    !item.as_str().is_some_and(|s| s.contains(',')),
                    "predicate array strings cannot contain commas",
                )?;
                validate_value(item)?;
            }
            Ok(())
        }
        _ => Err(SearchError(
            "predicate values must be non-null scalars or arrays of scalars".into(),
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Related {
    class: ClassSelector,
    #[serde(default)]
    filters: Vec<Predicate>,
    #[serde(default = "one")]
    depth: u8,
}
fn one() -> u8 {
    1
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sort {
    field: String,
    #[serde(default)]
    direction: SortDirection,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_nested_queries_and_preserves_typed_values_across_pages() {
        let input = json!({"version":1,"target":{"kind":"object","class":{"name":"Hosts"}},"filter":{"op":"and","args":[
            {"op":"field","predicate":{"field":"json_data","path":"cpu.cores","operator":"gte","value":8}},
            {"op":"not","arg":{"op":"related","predicate":{"class":{"id":3},"depth":2}}}
        ]}});
        let request = SearchRequest::parse(&input.to_string()).unwrap();
        let next = request
            .with_page(PageOptions::new().cursor("next").limit(25))
            .unwrap();
        let wire = serde_json::to_value(next).unwrap();
        assert_eq!(wire["filter"]["args"][0]["predicate"]["value"], 8);
        assert_eq!(wire["cursor"], "next");
    }

    #[test]
    fn rejects_invalid_queries_before_transport() {
        for input in [
            json!({"version":2,"target":{"kind":"object"}}),
            json!({"version":1,"target":{"kind":"class","class":{"id":1}}}),
            json!({"version":1,"target":{"kind":"object","class":{"id":1,"name":"Hosts"}}}),
            json!({"version":1,"target":{"kind":"object"},"limit":0}),
            json!({"version":1,"target":{"kind":"object"},"sort":[{"field":"json_data"}]}),
            json!({"version":1,"target":{"kind":"collection"},"filter":{"op":"related","predicate":{"class":{"id":1}}}}),
            json!({"version":1,"target":{"kind":"object"},"filter":{"op":"and","args":[]}}),
            json!({"version":1,"target":{"kind":"object"},"filter":{"op":"field","predicate":{"field":"email","operator":"equals","value":"a"}}}),
            json!({"version":1,"target":{"kind":"object"},"filter":{"op":"field","predicate":{"field":"json_data","path":"bad..path","operator":"equals","value":1}}}),
            json!({"version":1,"target":{"kind":"object"},"typo":true}),
        ] {
            assert!(SearchRequest::parse(&input.to_string()).is_err(), "{input}");
        }
    }

    #[test]
    fn bounds_expressions_and_pagination_request_size() {
        let mut filter =
            json!({"op":"field","predicate":{"field":"name","operator":"equals","value":"a"}});
        for _ in 0..8 {
            filter = json!({"op":"not","arg":filter});
        }
        assert!(SearchRequest::parse(
            &json!({"version":1,"target":{"kind":"object"},"filter":filter}).to_string()
        )
        .is_err());
        let request = SearchRequest::parse(r#"{"version":1,"target":{"kind":"object"}}"#).unwrap();
        assert!(request
            .clone()
            .with_page(PageOptions::new().cursor(""))
            .is_err());
        assert!(request
            .with_page(PageOptions::new().cursor("x".repeat(MAX_REQUEST_BYTES)))
            .is_err());
    }
}
