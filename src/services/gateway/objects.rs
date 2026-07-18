use std::cmp::Ordering;
use std::collections::HashMap;

use hubuum_client::{FilterOperator, ObjectPatch, ObjectPost};
use serde_json::Value;

use crate::domain::{
    build_related_object_tree, observed_json_pointers, ObjectShowRecord, ResolvedObjectRecord,
};
use crate::errors::AppError;
use crate::list_query::{
    apply_cursor_request_paging, apply_query_paging, validate_filter_clauses,
    validate_sort_clauses, FilterFieldSpec, FilterOperatorProfile, FilterValueProfile,
    FilterValueResolver, ListQuery, PagedResult, SortDirectionArg, SortFieldSpec,
    ValidatedSortClause,
};

use super::{shared::find_entities_by_ids, HubuumGateway, RelationTraversalOptions};

#[derive(Debug, Clone)]
pub struct CreateObjectInput {
    pub name: String,
    pub class_name: String,
    pub collection: String,
    pub description: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ObjectUpdateInput {
    pub name: String,
    pub class_name: String,
    pub rename: Option<String>,
    pub collection: Option<String>,
    pub reclass: Option<String>,
    pub description: Option<String>,
    pub data: Option<Value>,
}

impl HubuumGateway {
    pub fn observed_object_data_pointers(
        &self,
        class_name: &str,
        sample_limit: usize,
        max_depth: usize,
    ) -> Result<Vec<String>, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let objects = self
            .client
            .objects(class.id())
            .query()
            .limit(sample_limit)
            .list()?;
        Ok(observed_json_pointers(
            objects.iter().filter_map(|object| object.data.as_ref()),
            max_depth,
        ))
    }

    pub fn list_object_names_for_class(&self, class_name: &str) -> Result<Vec<String>, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        Ok(self
            .client
            .objects(class.id())
            .query()
            .list()?
            .into_iter()
            .map(|object| object.name)
            .collect())
    }

    pub fn list_object_names_for_class_prefix(
        &self,
        class_name: &str,
        prefix: &str,
    ) -> Result<Vec<String>, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        Ok(self
            .client
            .objects(class.id())
            .query()
            .filter(
                "name",
                FilterOperator::StartsWith { is_negated: false },
                prefix,
            )
            .limit(100)
            .list()?
            .into_iter()
            .map(|object| object.name)
            .collect())
    }

    pub fn create_object(
        &self,
        input: CreateObjectInput,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let collection = self.client.collections().get_by_name(&input.collection)?;
        let class = self.client.classes().get_by_name(&input.class_name)?;

        let object = self.client.objects(class.id()).create_raw(ObjectPost {
            name: input.name,
            hubuum_class_id: class.id(),
            collection_id: collection.id(),
            description: input.description,
            data: input.data,
        })?;

        let classmap = HashMap::from([(class.id().into(), class.resource().clone())]);
        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);

        Ok(ResolvedObjectRecord::new(
            &object,
            &classmap,
            &collectionmap,
        ))
    }

    pub fn object_details(
        &self,
        class_name: &str,
        object_name: &str,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        let collection = self
            .client
            .collections()
            .get(object.resource().collection_id)?;

        let classmap = HashMap::from([(class.id().into(), class.resource().clone())]);
        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);

        Ok(ResolvedObjectRecord::new(
            object.resource(),
            &classmap,
            &collectionmap,
        ))
    }

    pub fn object_show_details(
        &self,
        class_name: &str,
        object_name: &str,
        options: &RelationTraversalOptions,
        include_computed: bool,
    ) -> Result<ObjectShowRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        let collection = self
            .client
            .collections()
            .get(object.resource().collection_id)?;

        let classmap = HashMap::from([(class.id().into(), class.resource().clone())]);
        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);
        let mut object_record =
            ResolvedObjectRecord::new(object.resource(), &classmap, &collectionmap);
        if include_computed {
            let computed = self.client.computed_object(class.id(), object.id())?;
            object_record = object_record.with_computed(serde_json::to_value(computed.computed)?);
        }
        let related_graph = object
            .related_graph()
            .filter(
                "depth",
                FilterOperator::Lte { is_negated: false },
                options.max_depth,
            )
            .send()?;
        let graph_class_map = self.class_map_from_ids(
            related_graph
                .objects
                .iter()
                .map(|related_object| related_object.hubuum_class_id)
                .collect::<Vec<_>>(),
        )?;
        let graph_collection_map = self.collection_map_from_ids(
            related_graph
                .objects
                .iter()
                .map(|related_object| related_object.collection_id)
                .collect::<Vec<_>>(),
        )?;

        Ok(ObjectShowRecord {
            object: object_record,
            related_objects: build_related_object_tree(
                &related_graph.objects,
                &graph_class_map,
                &graph_collection_map,
                object.id().into(),
                class.id().into(),
                !options.include_self_class,
            ),
        })
    }

    pub fn delete_object(&self, class_name: &str, object_name: &str) -> Result<(), AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        self.client.objects(class.id()).delete(object.id())?;
        Ok(())
    }

    pub fn list_objects(
        &self,
        query: &ListQuery,
        include_computed: bool,
    ) -> Result<PagedResult<ResolvedObjectRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, OBJECT_FILTER_SPECS)?;
        let object_sorts = validate_object_sort_clauses(query)?;
        let has_computed_sort = object_sorts
            .iter()
            .any(|sort| matches!(sort, ObjectSortClause::Computed { .. }));
        if has_computed_sort && !include_computed {
            return Err(AppError::ParseError(
                "S:/P: computed sorting requires computed values to be fetched".to_string(),
            ));
        }
        if has_computed_sort && query.cursor.is_some() {
            return Err(AppError::ParseError(
                "--cursor cannot be combined with S:/P: computed sorting because server v0.0.2 does not provide cursors for computed orderings"
                    .to_string(),
            ));
        }
        let validated_sorts = object_sorts
            .iter()
            .filter_map(|sort| match sort {
                ObjectSortClause::Standard(sort) => Some(sort.clone()),
                ObjectSortClause::Computed { .. } => None,
            })
            .collect::<Vec<_>>();
        let class_filter = validated
            .iter()
            .find(|clause| clause.spec.public_name == "class")
            .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
        let class = self.client.classes().get_by_name(&class_filter.value)?;

        let filters = validated
            .iter()
            .filter(|clause| clause.spec.public_name != "class")
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        if has_computed_sort {
            let fetched = self
                .client
                .computed_objects(class.id())
                .filters(filters)
                .all()?;
            let classmap =
                find_entities_by_ids(&self.client.classes(), fetched.iter(), |object| {
                    object.object.hubuum_class_id
                })?;
            let collectionmap =
                find_entities_by_ids(&self.client.collections(), fetched.iter(), |object| {
                    object.object.collection_id
                })?;
            let mut items = fetched
                .into_iter()
                .map(|object| {
                    Ok(
                        ResolvedObjectRecord::new(&object.object, &classmap, &collectionmap)
                            .with_computed(serde_json::to_value(object.computed)?),
                    )
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            sort_objects_locally(&mut items, &object_sorts);
            let total_count = query.include_total.then_some(items.len() as u64);
            if let Some(limit) = query.limit {
                items.truncate(limit);
            }
            let returned_count = items.len();
            return Ok(PagedResult {
                items,
                next_cursor: None,
                returned_count,
                total_count,
            });
        }

        if include_computed {
            let page = apply_cursor_request_paging(
                self.client.computed_objects(class.id()).filters(filters),
                query,
                &validated_sorts,
            )
            .page()?;
            if page.items.is_empty() {
                return Ok(PagedResult {
                    items: Vec::new(),
                    next_cursor: page.next_cursor,
                    returned_count: 0,
                    total_count: page.total_count,
                });
            }

            let classmap =
                find_entities_by_ids(&self.client.classes(), page.items.iter(), |object| {
                    object.object.hubuum_class_id
                })?;
            let collectionmap =
                find_entities_by_ids(&self.client.collections(), page.items.iter(), |object| {
                    object.object.collection_id
                })?;
            let returned_count = page.items.len();
            let items = page
                .items
                .into_iter()
                .map(|object| {
                    Ok(
                        ResolvedObjectRecord::new(&object.object, &classmap, &collectionmap)
                            .with_computed(serde_json::to_value(object.computed)?),
                    )
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            return Ok(PagedResult {
                items,
                next_cursor: page.next_cursor,
                returned_count,
                total_count: page.total_count,
            });
        }

        let page = apply_query_paging(
            self.client.objects(class.id()).query().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                returned_count: 0,
                total_count: page.total_count,
            });
        }

        let classmap = find_entities_by_ids(&self.client.classes(), page.items.iter(), |object| {
            object.hubuum_class_id
        })?;
        let collectionmap =
            find_entities_by_ids(&self.client.collections(), page.items.iter(), |object| {
                object.collection_id
            })?;

        Ok(PagedResult::from_page(page, |object| {
            ResolvedObjectRecord::new(&object, &classmap, &collectionmap)
        }))
    }

    pub fn update_object(
        &self,
        input: ObjectUpdateInput,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().get_by_name(&input.class_name)?;
        let object = class.object_by_name(&input.name)?;

        let mut patch = ObjectPatch {
            data: input.data,
            ..ObjectPatch::default()
        };

        if let Some(collection) = input.collection {
            let collection = self.client.collections().get_by_name(&collection)?;
            patch.collection_id = Some(collection.id());
        }
        if let Some(reclass) = input.reclass {
            let reclass = self.client.classes().get_by_name(&reclass)?;
            patch.hubuum_class_id = Some(reclass.id());
        }
        if let Some(rename) = input.rename {
            patch.name = Some(rename);
        }
        if let Some(description) = input.description {
            patch.description = Some(description);
        }

        let result = self
            .client
            .objects(class.id())
            .update_raw(object.id(), patch)?;
        let collection = self.client.collections().get(result.collection_id)?;

        let classmap = HashMap::from([(class.id().into(), class.resource().clone())]);
        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);

        Ok(ResolvedObjectRecord::new(
            &result,
            &classmap,
            &collectionmap,
        ))
    }
}

#[derive(Debug, Clone, Copy)]
enum ComputedSortScope {
    Shared,
    Personal,
}

impl ComputedSortScope {
    fn parse_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "S" => Some(Self::Shared),
            "P" => Some(Self::Personal),
            _ => None,
        }
    }

    const fn response_key(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Personal => "personal",
        }
    }
}

#[derive(Debug, Clone)]
struct ComputedSortField {
    scope: ComputedSortScope,
    key: String,
}

impl ComputedSortField {
    fn parse(field: &str) -> Result<Option<Self>, AppError> {
        let Some((prefix, key)) = field.split_once(':') else {
            return Ok(None);
        };
        let Some(scope) = ComputedSortScope::parse_prefix(prefix) else {
            return Ok(None);
        };
        if key.is_empty() {
            return Err(AppError::ParseError(format!(
                "Computed sort field '{field}' requires a key after the colon"
            )));
        }
        Ok(Some(Self {
            scope,
            key: key.to_string(),
        }))
    }

    fn value<'a>(&self, object: &'a ResolvedObjectRecord) -> Option<&'a Value> {
        object
            .computed
            .as_ref()?
            .get(self.scope.response_key())?
            .get("values")?
            .get(&self.key)
            .filter(|value| !value.is_null())
    }
}

#[derive(Debug, Clone)]
enum ObjectSortClause {
    Standard(ValidatedSortClause),
    Computed {
        field: ComputedSortField,
        direction: SortDirectionArg,
    },
}

fn validate_object_sort_clauses(query: &ListQuery) -> Result<Vec<ObjectSortClause>, AppError> {
    query
        .sorts
        .iter()
        .map(|clause| {
            if let Some(field) = ComputedSortField::parse(&clause.field)? {
                return Ok(ObjectSortClause::Computed {
                    field,
                    direction: clause.direction,
                });
            }
            let mut validated =
                validate_sort_clauses(std::slice::from_ref(clause), OBJECT_SORT_SPECS)?;
            Ok(ObjectSortClause::Standard(validated.remove(0)))
        })
        .collect()
}

fn sort_objects_locally(objects: &mut [ResolvedObjectRecord], sorts: &[ObjectSortClause]) {
    objects.sort_by(|left, right| {
        for sort in sorts {
            let (ordering, direction) = match sort {
                ObjectSortClause::Standard(sort) => (
                    compare_standard_object_field(sort.spec.public_name, left, right),
                    sort.direction,
                ),
                ObjectSortClause::Computed { field, direction } => (
                    compare_json_sort_values(field.value(left), field.value(right)),
                    *direction,
                ),
            };
            let ordering = match direction {
                SortDirectionArg::Asc => ordering,
                SortDirectionArg::Desc => ordering.reverse(),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        left.id.cmp(&right.id)
    });
}

fn compare_standard_object_field(
    field: &str,
    left: &ResolvedObjectRecord,
    right: &ResolvedObjectRecord,
) -> Ordering {
    match field {
        "id" => left.id.cmp(&right.id),
        "name" => left.name.cmp(&right.name),
        "description" => left.description.cmp(&right.description),
        "collection" => left.collection.cmp(&right.collection),
        "class" => left.class.cmp(&right.class),
        "created_at" => left.created_at.cmp(&right.created_at),
        "updated_at" => left.updated_at.cmp(&right.updated_at),
        _ => Ordering::Equal,
    }
}

fn compare_json_sort_values(left: Option<&Value>, right: Option<&Value>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => compare_json_values(left, right),
    }
}

fn compare_json_values(left: &Value, right: &Value) -> Ordering {
    let rank = |value: &Value| match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    };
    let rank_ordering = rank(left).cmp(&rank(right));
    if rank_ordering != Ordering::Equal {
        return rank_ordering;
    }
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .map(|(left, right)| left.total_cmp(&right))
            .unwrap_or_else(|| left.to_string().cmp(&right.to_string())),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Array(_), Value::Array(_)) | (Value::Object(_), Value::Object(_)) => {
            left.to_string().cmp(&right.to_string())
        }
        _ => Ordering::Equal,
    }
}

pub(crate) const OBJECT_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "class",
        "hubuum_class_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
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

pub(crate) const OBJECT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("class", "hubuum_class_id"),
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("collection", "collection_id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::domain::ResolvedObjectRecord;
    use crate::list_query::{resolve_filter_field_spec, ListQuery, SortClause, SortDirectionArg};

    use super::{
        sort_objects_locally, validate_object_sort_clauses, ObjectSortClause, OBJECT_FILTER_SPECS,
    };

    #[test]
    fn object_filters_accept_json_data_paths() {
        let (spec, json_path) = resolve_filter_field_spec(OBJECT_FILTER_SPECS, "json_data.contact")
            .expect("json_data path should resolve");

        assert_eq!(spec.public_name, "json_data");
        assert_eq!(spec.backend_field, "json_data");
        assert_eq!(json_path, vec!["contact"]);
    }

    #[test]
    fn object_filters_keep_data_alias_for_json_data_paths() {
        let (spec, json_path) = resolve_filter_field_spec(OBJECT_FILTER_SPECS, "data.contact")
            .expect("data alias path should resolve");

        assert_eq!(spec.public_name, "data");
        assert_eq!(spec.backend_field, "json_data");
        assert_eq!(json_path, vec!["contact"]);
    }

    #[test]
    fn object_sorts_accept_scoped_computed_fields() {
        let sorts = validate_object_sort_clauses(&ListQuery {
            sorts: vec![
                SortClause {
                    field: "S:os_version".to_string(),
                    direction: SortDirectionArg::Asc,
                },
                SortClause {
                    field: "P:preferred_name".to_string(),
                    direction: SortDirectionArg::Desc,
                },
            ],
            ..ListQuery::default()
        })
        .expect("computed sorts should validate");

        assert!(matches!(sorts[0], ObjectSortClause::Computed { .. }));
        assert!(matches!(sorts[1], ObjectSortClause::Computed { .. }));
    }

    #[test]
    fn object_sorts_reject_empty_computed_keys() {
        let error = validate_object_sort_clauses(&ListQuery {
            sorts: vec![SortClause {
                field: "S:".to_string(),
                direction: SortDirectionArg::Asc,
            }],
            ..ListQuery::default()
        })
        .expect_err("empty computed sort key should fail");

        assert!(error.to_string().contains("requires a key"));
    }

    #[test]
    fn computed_sorting_orders_numbers_and_treats_errors_as_empty() {
        let mut objects = vec![
            computed_object(1, json!({"shared": {"values": {"load": 10}, "errors": {}}})),
            computed_object(
                2,
                json!({"shared": {"values": {}, "errors": {"load": {"message": "bad"}}}}),
            ),
            computed_object(3, json!({"shared": {"values": {"load": 2}, "errors": {}}})),
        ];
        let sorts = validate_object_sort_clauses(&ListQuery {
            sorts: vec![SortClause {
                field: "S:load".to_string(),
                direction: SortDirectionArg::Asc,
            }],
            ..ListQuery::default()
        })
        .expect("sort should validate");

        sort_objects_locally(&mut objects, &sorts);

        assert_eq!(
            objects.iter().map(|object| object.id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    fn computed_object(id: i32, computed: serde_json::Value) -> ResolvedObjectRecord {
        ResolvedObjectRecord {
            id,
            name: format!("object-{id}"),
            description: String::new(),
            collection: "Collection".to_string(),
            class: "Hosts".to_string(),
            data: None,
            computed: Some(computed),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}
