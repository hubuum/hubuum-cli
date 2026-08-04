use std::collections::HashSet;
use std::iter::once;
use std::slice::from_ref;
use std::str::FromStr;

use hubuum_client::{
    ComputedFieldSelector, ObjectAggregateDimension, ObjectAggregateJsonPath,
    ObjectAggregateMeasure, ObjectAggregateMeasureField, ObjectAggregateMeasureOperation,
    ObjectAggregateSort, QueryFilter,
};

use crate::domain::ObjectAggregateRecord;
use crate::errors::AppError;
use crate::list_query::{
    validate_filter_clauses, FilterClause, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile, FilterValueResolver, PageSelection, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

const MAX_DIMENSIONS: usize = 3;
const MAX_MEASURES: usize = 4;
const MAX_COMPUTED_FILTERS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectAggregateDimensionInput {
    value: ObjectAggregateDimension,
    label: String,
}

impl ObjectAggregateDimensionInput {
    fn api_value(&self) -> ObjectAggregateDimension {
        self.value.clone()
    }

    fn wire_value(&self) -> String {
        self.value.to_string()
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl FromStr for ObjectAggregateDimensionInput {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let (dimension, label) = match value {
            "name" => (ObjectAggregateDimension::Name, "name".to_string()),
            "description" => (
                ObjectAggregateDimension::Description,
                "description".to_string(),
            ),
            "collection" | "collection_id" => (
                ObjectAggregateDimension::CollectionId,
                "collection_id".to_string(),
            ),
            "created_at" => (
                ObjectAggregateDimension::CreatedAt,
                "created_at".to_string(),
            ),
            "updated_at" => (
                ObjectAggregateDimension::UpdatedAt,
                "updated_at".to_string(),
            ),
            _ => {
                let field = parse_aggregate_field(value, "group-by")?;
                match field {
                    AggregateField::Json { path, label } => {
                        (ObjectAggregateDimension::json_data(path), label)
                    }
                    AggregateField::SharedComputed { key, label } => {
                        (ObjectAggregateDimension::shared_computed(key), label)
                    }
                    AggregateField::PersonalComputed { key, label } => {
                        (ObjectAggregateDimension::personal_computed(key), label)
                    }
                }
            }
        };

        Ok(Self {
            value: dimension,
            label,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectAggregateMeasureInput {
    value: ObjectAggregateMeasure,
    label: String,
}

impl ObjectAggregateMeasureInput {
    fn api_value(&self) -> ObjectAggregateMeasure {
        self.value.clone()
    }

    fn wire_value(&self) -> String {
        self.value.to_string()
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl FromStr for ObjectAggregateMeasureInput {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let (operation, field) = value.split_once(':').ok_or_else(|| {
            AppError::InvalidOption(format!(
                "Invalid aggregate measure '{value}'; expected operation:field"
            ))
        })?;
        let (operation, operation_label) = match operation {
            "sum" => (ObjectAggregateMeasureOperation::Sum, "sum"),
            "average" | "avg" => (ObjectAggregateMeasureOperation::Average, "average"),
            "min" => (ObjectAggregateMeasureOperation::Min, "min"),
            "max" => (ObjectAggregateMeasureOperation::Max, "max"),
            _ => {
                return Err(AppError::InvalidOption(format!(
                    "Invalid aggregate operation '{operation}'; use sum, average, min, or max"
                )))
            }
        };
        let field = parse_aggregate_field(field, "aggregate")?;
        let (field, field_label) = match field {
            AggregateField::Json { path, label } => {
                (ObjectAggregateMeasureField::json_data(path), label)
            }
            AggregateField::SharedComputed { key, label } => {
                (ObjectAggregateMeasureField::shared_computed(key), label)
            }
            AggregateField::PersonalComputed { key, label } => {
                (ObjectAggregateMeasureField::personal_computed(key), label)
            }
        };

        Ok(Self {
            value: ObjectAggregateMeasure::new(operation, field),
            label: format!("{operation_label}:{field_label}"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectAggregateSortInput {
    value: ObjectAggregateSort,
}

impl ObjectAggregateSortInput {
    fn api_value(self) -> ObjectAggregateSort {
        self.value
    }
}

impl FromStr for ObjectAggregateSortInput {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split_whitespace();
        let field = parts.next().ok_or_else(|| {
            AppError::InvalidOption(
                "Aggregate sort requires dimensions or object_count".to_string(),
            )
        })?;
        let direction = parts.next().ok_or_else(|| {
            AppError::InvalidOption("Aggregate sort requires asc or desc".to_string())
        })?;
        if parts.next().is_some() {
            return Err(AppError::InvalidOption(format!(
                "Invalid aggregate sort '{value}'; expected FIELD DIRECTION"
            )));
        }
        let value = match (field, direction) {
            ("dimensions", "asc") => ObjectAggregateSort::DimensionsAsc,
            ("dimensions", "desc") => ObjectAggregateSort::DimensionsDesc,
            ("object_count", "asc") => ObjectAggregateSort::ObjectCountAsc,
            ("object_count", "desc") => ObjectAggregateSort::ObjectCountDesc,
            (_, "asc" | "desc") => {
                return Err(AppError::InvalidOption(format!(
                    "Invalid aggregate sort field '{field}'; use dimensions or object_count"
                )))
            }
            _ => {
                return Err(AppError::InvalidOption(format!(
                    "Invalid aggregate sort direction '{direction}'; use asc or desc"
                )))
            }
        };
        Ok(Self { value })
    }
}

#[derive(Debug, Clone)]
pub struct ObjectAggregateInput {
    class_name: String,
    dimensions: Vec<ObjectAggregateDimensionInput>,
    measures: Vec<ObjectAggregateMeasureInput>,
    filters: Vec<FilterClause>,
    sort: Option<ObjectAggregateSortInput>,
    limit: Option<usize>,
    cursor: Option<String>,
    include_total: bool,
    page_selection: PageSelection,
}

impl ObjectAggregateInput {
    pub fn new(
        class_name: impl Into<String>,
        dimensions: Vec<ObjectAggregateDimensionInput>,
        measures: Vec<ObjectAggregateMeasureInput>,
    ) -> Result<Self, AppError> {
        let class_name = class_name.into();
        if class_name.trim().is_empty() {
            return Err(AppError::InvalidOption(
                "Aggregate class name cannot be empty".to_string(),
            ));
        }
        if dimensions.is_empty() && measures.is_empty() {
            return Err(AppError::ParseError(
                "Object aggregation requires at least one --group-by dimension or --aggregate measure"
                    .to_string(),
            ));
        }
        if dimensions.len() > MAX_DIMENSIONS {
            return Err(AppError::InvalidOption(format!(
                "Object aggregation supports at most {MAX_DIMENSIONS} --group-by dimensions"
            )));
        }
        if measures.len() > MAX_MEASURES {
            return Err(AppError::InvalidOption(format!(
                "Object aggregation supports at most {MAX_MEASURES} --aggregate measures"
            )));
        }
        reject_duplicates(
            dimensions
                .iter()
                .map(ObjectAggregateDimensionInput::wire_value),
            "group-by dimension",
        )?;
        reject_duplicates(
            measures.iter().map(ObjectAggregateMeasureInput::wire_value),
            "aggregate measure",
        )?;

        Ok(Self {
            class_name,
            dimensions,
            measures,
            filters: Vec::new(),
            sort: None,
            limit: None,
            cursor: None,
            include_total: false,
            page_selection: PageSelection::Single,
        })
    }

    pub fn filters(mut self, filters: Vec<FilterClause>) -> Self {
        self.filters = filters;
        self
    }

    pub fn sort(mut self, sort: Option<ObjectAggregateSortInput>) -> Self {
        self.sort = sort;
        self
    }

    pub fn limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }

    pub fn cursor(mut self, cursor: Option<String>) -> Self {
        self.cursor = cursor;
        self
    }

    pub fn include_total(mut self, include_total: bool) -> Self {
        self.include_total = include_total;
        self
    }

    pub fn page_selection(mut self, page_selection: PageSelection) -> Self {
        self.page_selection = page_selection;
        self
    }

    pub fn columns(&self) -> Vec<String> {
        self.dimensions
            .iter()
            .map(|dimension| dimension.label().to_string())
            .chain(once("object_count".to_string()))
            .chain(
                self.measures
                    .iter()
                    .map(|measure| measure.label().to_string()),
            )
            .collect()
    }
}

impl HubuumGateway {
    pub fn aggregate_objects(
        &self,
        input: &ObjectAggregateInput,
    ) -> Result<PagedResult<ObjectAggregateRecord>, AppError> {
        let filters = self.resolve_object_aggregate_filters(&input.filters)?;
        let mut request = self
            .client
            .class_by_name(input.class_name.clone())
            .object_aggregates()
            .filters(filters)
            .group_by_all(
                input
                    .dimensions
                    .iter()
                    .map(ObjectAggregateDimensionInput::api_value),
            )
            .aggregate_all(
                input
                    .measures
                    .iter()
                    .map(ObjectAggregateMeasureInput::api_value),
            )
            .include_total(input.include_total);

        if let Some(sort) = input.sort {
            request = request.aggregate_sort(sort.api_value());
        }
        if let Some(limit) = input.limit {
            request = request.limit(limit);
        }
        if let Some(cursor) = &input.cursor {
            request = request.cursor(cursor.clone());
        }

        if matches!(input.page_selection, PageSelection::All) {
            Ok(PagedResult::from_pages(request.pages())?.map(Into::into))
        } else {
            Ok(PagedResult::from_page(request.page()?, Into::into))
        }
    }

    fn resolve_object_aggregate_filters(
        &self,
        filters: &[FilterClause],
    ) -> Result<Vec<QueryFilter>, AppError> {
        let mut resolved = Vec::with_capacity(filters.len());
        let mut computed_count = 0;

        for filter in filters {
            if let Some(selector) = parse_computed_selector(&filter.field)? {
                computed_count += 1;
                if computed_count > MAX_COMPUTED_FILTERS {
                    return Err(AppError::InvalidOption(format!(
                        "Object aggregation supports at most {MAX_COMPUTED_FILTERS} computed filters"
                    )));
                }
                resolved.push(QueryFilter::filter(
                    selector.to_string(),
                    filter.operator.clone(),
                    filter.value.clone(),
                ));
                continue;
            }

            let mut validated =
                validate_filter_clauses(from_ref(filter), OBJECT_AGGREGATE_FILTER_SPECS)?;
            resolved.push(self.resolve_validated_filter(&validated.remove(0))?);
        }

        Ok(resolved)
    }
}

enum AggregateField {
    Json {
        path: ObjectAggregateJsonPath,
        label: String,
    },
    SharedComputed {
        key: String,
        label: String,
    },
    PersonalComputed {
        key: String,
        label: String,
    },
}

fn parse_aggregate_field(value: &str, option: &str) -> Result<AggregateField, AppError> {
    if let Some(path) = value
        .strip_prefix("data.")
        .or_else(|| value.strip_prefix("json_data."))
    {
        let path = parse_json_path(path, value)?;
        let label = format!("data.{}", path.segments().join("."));
        return Ok(AggregateField::Json { path, label });
    }
    if let Some(key) = value
        .strip_prefix("S:")
        .or_else(|| value.strip_prefix("computed.shared."))
    {
        validate_computed_key(key, value)?;
        return Ok(AggregateField::SharedComputed {
            key: key.to_string(),
            label: format!("S:{key}"),
        });
    }
    if let Some(key) = value
        .strip_prefix("P:")
        .or_else(|| value.strip_prefix("computed.personal."))
    {
        validate_computed_key(key, value)?;
        return Ok(AggregateField::PersonalComputed {
            key: key.to_string(),
            label: format!("P:{key}"),
        });
    }

    Err(AppError::InvalidOption(format!(
        "Invalid --{option} field '{value}'; use data.path, S:key, or P:key"
    )))
}

fn parse_json_path(path: &str, original: &str) -> Result<ObjectAggregateJsonPath, AppError> {
    let segments = path
        .split(['.', ','])
        .map(str::to_string)
        .collect::<Vec<_>>();
    ObjectAggregateJsonPath::new(segments).map_err(|error| {
        AppError::InvalidOption(format!("Invalid aggregate JSON path '{original}': {error}"))
    })
}

fn parse_computed_selector(value: &str) -> Result<Option<ComputedFieldSelector>, AppError> {
    if let Some(key) = value
        .strip_prefix("S:")
        .or_else(|| value.strip_prefix("computed.shared."))
    {
        validate_computed_key(key, value)?;
        return Ok(Some(ComputedFieldSelector::shared(key)));
    }
    if let Some(key) = value
        .strip_prefix("P:")
        .or_else(|| value.strip_prefix("computed.personal."))
    {
        validate_computed_key(key, value)?;
        return Ok(Some(ComputedFieldSelector::personal(key)));
    }
    Ok(None)
}

fn validate_computed_key(key: &str, original: &str) -> Result<(), AppError> {
    if key.trim().is_empty() {
        return Err(AppError::InvalidOption(format!(
            "Invalid computed selector '{original}'; the key cannot be empty"
        )));
    }
    Ok(())
}

fn reject_duplicates(
    values: impl IntoIterator<Item = String>,
    description: &str,
) -> Result<(), AppError> {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value.clone()) {
            return Err(AppError::InvalidOption(format!(
                "Duplicate object {description} '{value}'"
            )));
        }
    }
    Ok(())
}

pub(crate) const OBJECT_AGGREGATE_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "description",
        "description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "collection",
        "collection_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::CollectionNameToId),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "json_data",
        "json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
    FilterFieldSpec::new(
        "data",
        "json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
];

pub(crate) const OBJECT_AGGREGATE_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("dimensions", "dimensions"),
    SortFieldSpec::new("object_count", "object_count"),
];

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{
        blocking::Client, FilterOperator, MockTransport, Token, TransportResponse,
    };
    use reqwest::{
        header::{HeaderName, HeaderValue},
        StatusCode,
    };
    use serde_json::{json, to_value};

    use crate::list_query::FilterClause;

    use super::{
        ObjectAggregateDimensionInput, ObjectAggregateInput, ObjectAggregateMeasureInput,
        ObjectAggregateSortInput,
    };
    use crate::services::HubuumGateway;

    #[test]
    fn aggregate_inputs_parse_cli_selectors() {
        let dimension = "data.region.zone"
            .parse::<ObjectAggregateDimensionInput>()
            .expect("dimension");
        let computed = "S:risk"
            .parse::<ObjectAggregateDimensionInput>()
            .expect("computed dimension");
        let measure = "avg:data.metrics.latency_ms"
            .parse::<ObjectAggregateMeasureInput>()
            .expect("measure");

        assert_eq!(dimension.wire_value(), "json_data.region,zone");
        assert_eq!(dimension.label(), "data.region.zone");
        assert_eq!(computed.wire_value(), "computed.shared.risk");
        assert_eq!(measure.wire_value(), "average:json_data.metrics,latency_ms");
        assert_eq!(measure.label(), "average:data.metrics.latency_ms");
    }

    #[test]
    fn aggregate_input_validates_bounds_and_duplicates() {
        let dimension = "name"
            .parse::<ObjectAggregateDimensionInput>()
            .expect("dimension");
        let duplicate = ObjectAggregateInput::new(
            "Hosts",
            vec![dimension.clone(), dimension.clone()],
            Vec::new(),
        )
        .expect_err("duplicates should fail");
        assert!(duplicate.to_string().contains("Duplicate"));

        let missing = ObjectAggregateInput::new("Hosts", Vec::new(), Vec::new())
            .expect_err("empty aggregate should fail");
        assert!(missing.to_string().contains("--group-by"));
        assert!(missing.to_string().contains("--aggregate"));
    }

    #[test]
    fn aggregate_gateway_uses_server_endpoint_and_typed_query() {
        let transport = MockTransport::default();
        let mut response = TransportResponse::json(
            StatusCode::OK,
            &json!([{
                "dimensions": [{
                    "field": "json_data.region,zone",
                    "state": "value",
                    "value": "eu-west"
                }],
                "measures": [{
                    "field": "json_data.metrics,latency_ms",
                    "operation": "average",
                    "state": "value",
                    "value_count": 3,
                    "skipped_count": 1,
                    "value": 12.5
                }],
                "object_count": 4
            }]),
        )
        .expect("response");
        response.headers.insert(
            HeaderName::from_static("x-next-cursor"),
            HeaderValue::from_static("next-page"),
        );
        response.headers.insert(
            HeaderName::from_static("x-total-count"),
            HeaderValue::from_static("9"),
        );
        transport.push_response(response);
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let input = ObjectAggregateInput::new(
            "Hosts",
            vec!["data.region.zone"
                .parse::<ObjectAggregateDimensionInput>()
                .expect("dimension")],
            vec!["avg:data.metrics.latency_ms"
                .parse::<ObjectAggregateMeasureInput>()
                .expect("measure")],
        )
        .expect("input")
        .filters(vec![
            FilterClause {
                field: "name".to_string(),
                operator: FilterOperator::Contains { is_negated: false },
                value: "edge".to_string(),
            },
            FilterClause {
                field: "S:risk".to_string(),
                operator: FilterOperator::Gte { is_negated: false },
                value: "1".to_string(),
            },
        ])
        .sort(Some(
            "object_count desc"
                .parse::<ObjectAggregateSortInput>()
                .expect("sort"),
        ))
        .limit(Some(25))
        .cursor(Some("current-page".to_string()))
        .include_total(true);

        let page = gateway
            .aggregate_objects(&input)
            .expect("aggregate request");

        assert_eq!(page.items.len(), 1);
        assert_eq!(
            to_value(&page.items[0]).expect("serialized row")["object_count"],
            json!(4)
        );
        assert_eq!(page.next_cursor.as_deref(), Some("next-page"));
        assert_eq!(page.total_count, Some(9));
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url.path(),
            "/api/v1/classes/by-name/Hosts/object-aggregates"
        );
        let query = requests[0]
            .url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        assert!(query.contains(&("group_by".to_string(), "json_data.region,zone".to_string())));
        assert!(query.contains(&(
            "aggregate".to_string(),
            "average:json_data.metrics,latency_ms".to_string()
        )));
        assert!(query.contains(&("sort".to_string(), "object_count.desc".to_string())));
        assert!(query.contains(&("name__contains".to_string(), "edge".to_string())));
        assert!(query.contains(&("computed.shared.risk__gte".to_string(), "1".to_string())));
        assert!(query.contains(&("limit".to_string(), "25".to_string())));
        assert!(query.contains(&("cursor".to_string(), "current-page".to_string())));
        assert!(query.contains(&("include_total".to_string(), "true".to_string())));
    }

    #[test]
    fn aggregate_gateway_limits_computed_filters() {
        let input = ObjectAggregateInput::new(
            "Hosts",
            vec!["name"
                .parse::<ObjectAggregateDimensionInput>()
                .expect("dimension")],
            Vec::new(),
        )
        .expect("input")
        .filters(
            ["S:first", "S:second", "P:third"]
                .into_iter()
                .map(|field| FilterClause {
                    field: field.to_string(),
                    operator: FilterOperator::Equals { is_negated: false },
                    value: "1".to_string(),
                })
                .collect(),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL")
            .with_transport(Arc::new(MockTransport::default()))
            .build()
            .expect("client")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let error = gateway
            .aggregate_objects(&input)
            .expect_err("too many computed filters should fail");

        assert!(error.to_string().contains("at most 2 computed filters"));
    }
}
