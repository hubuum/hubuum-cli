use std::cmp::Ordering;
use std::collections::HashMap;

use hubuum_client::{FilterOperator, ObjectDataPatchDocument, ObjectPatch, ObjectPost};
use json_patch::{patch as apply_json_patch, Patch};
use reqwest::StatusCode;
use serde_json::Value;

use crate::domain::{
    build_related_object_tree, observed_json_pointers, ObjectDataMutationOutcome,
    ObjectDataMutationRecord, ObjectShowRecord, ResolvedObjectRecord,
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

#[derive(Debug, Clone)]
enum MissingObjectPolicy {
    Error,
    Create { description: String },
}

#[derive(Debug, Clone)]
pub struct ObjectDataPatchInput {
    class_name: String,
    object_name: String,
    patch: ObjectDataPatchDocument,
    missing: MissingObjectPolicy,
}

impl ObjectDataPatchInput {
    pub fn new(
        class_name: impl Into<String>,
        object_name: impl Into<String>,
        patch: ObjectDataPatchDocument,
    ) -> Result<Self, AppError> {
        let class_name = class_name.into();
        let object_name = object_name.into();
        if class_name.trim().is_empty() {
            return Err(AppError::ParseError(
                "Object data patch class name cannot be empty".to_string(),
            ));
        }
        if object_name.trim().is_empty() {
            return Err(AppError::ParseError(
                "Object data patch object name cannot be empty".to_string(),
            ));
        }

        Ok(Self {
            class_name,
            object_name,
            patch,
            missing: MissingObjectPolicy::Error,
        })
    }

    pub fn create_if_missing(mut self, description: impl Into<String>) -> Self {
        self.missing = MissingObjectPolicy::Create {
            description: description.into(),
        };
        self
    }
}

impl HubuumGateway {
    pub fn patch_object_data(
        &self,
        input: ObjectDataPatchInput,
    ) -> Result<ObjectDataMutationRecord, AppError> {
        let objects = self
            .client
            .class_by_name(input.class_name.clone())
            .objects();
        let object = objects.by_name(input.object_name.clone());

        match object.patch_data(&input.patch) {
            Ok(patched) => Ok(ObjectDataMutationRecord::new(
                ObjectDataMutationOutcome::Patched,
                input.class_name,
                patched,
            )),
            Err(error) if error.is_status(StatusCode::NOT_FOUND) => {
                let MissingObjectPolicy::Create { description } = input.missing else {
                    return Err(error.into());
                };
                let initial_data = initial_data_from_patch(&input.patch)?;
                match objects.create(input.object_name.clone(), description, initial_data) {
                    Ok(created) => Ok(ObjectDataMutationRecord::new(
                        ObjectDataMutationOutcome::Created,
                        input.class_name,
                        created,
                    )),
                    Err(error) if error.is_status(StatusCode::CONFLICT) => {
                        let patched = object.patch_data(&input.patch)?;
                        Ok(ObjectDataMutationRecord::new(
                            ObjectDataMutationOutcome::Patched,
                            input.class_name,
                            patched,
                        ))
                    }
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

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
            hubuum_class_id: Some(class.id()),
            collection_id: Some(collection.id()),
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
                "--cursor cannot be combined with S:/P: computed sorting because locally sorted results do not have server cursors"
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

fn initial_data_from_patch(patch: &ObjectDataPatchDocument) -> Result<Value, AppError> {
    let patch = serde_json::from_value::<Patch>(serde_json::to_value(patch)?)?;
    let mut data = serde_json::json!({});
    apply_json_patch(&mut data, &patch.0).map_err(|error| {
        AppError::ParseError(format!(
            "Cannot create the missing object because the patch does not apply to an empty JSON object: {error}"
        ))
    })?;
    Ok(data)
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
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::str::FromStr;
    use std::sync::Arc;
    use std::thread;

    use hubuum_client::{
        blocking::Client as BlockingClient, BaseUrl, ObjectDataPatchDocument,
        ObjectDataPatchOperation, Token,
    };
    use serde_json::json;

    use crate::domain::{ObjectDataMutationOutcome, ResolvedObjectRecord};
    use crate::list_query::{resolve_filter_field_spec, ListQuery, SortClause, SortDirectionArg};

    use super::{
        initial_data_from_patch, sort_objects_locally, validate_object_sort_clauses, HubuumGateway,
        ObjectDataPatchInput, ObjectSortClause, OBJECT_FILTER_SPECS,
    };

    #[test]
    fn create_conflict_retries_exact_name_patch_once() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should have an address");
        let object = json!({
            "id": 42,
            "name": "srv-01",
            "collection_id": 7,
            "hubuum_class_id": 9,
            "description": "Managed by Ansible",
            "data": {"facts": {"os": "Fedora"}},
            "created_at": "2026-07-21T12:00:00Z",
            "updated_at": "2026-07-21T12:00:01Z"
        })
        .to_string();
        let responses = vec![
            http_response(
                "404 Not Found",
                r#"{"error":"not_found","message":"missing"}"#,
            ),
            http_response("409 Conflict", r#"{"error":"conflict","message":"exists"}"#),
            http_response("200 OK", &object),
        ];
        let server = thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut stream, _) = listener.accept().expect("request should connect");
                    let request = read_http_request(&mut stream);
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should be written");
                    request
                })
                .collect::<Vec<_>>()
        });

        let base_url =
            BaseUrl::from_str(&format!("http://{address}")).expect("test base URL should parse");
        let client = BlockingClient::builder(base_url)
            .build()
            .expect("test client should build")
            .authenticate(Token::new("test-token"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let patch = ObjectDataPatchDocument::new([ObjectDataPatchOperation::Add {
            path: "/facts".to_string(),
            value: json!({"os": "Fedora"}),
        }]);
        let input = ObjectDataPatchInput::new("Hosts", "srv-01", patch)
            .expect("input should be valid")
            .create_if_missing("Managed by Ansible");

        let result = gateway
            .patch_object_data(input)
            .expect("conflicting create should retry the patch");
        let requests = server.join().expect("test server should finish");

        assert_eq!(result.outcome, ObjectDataMutationOutcome::Patched);
        assert_eq!(result.object.id, 42);
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with(
            "PATCH /api/v1/classes/by-name/Hosts/objects/by-name/srv-01/data HTTP/1.1"
        ));
        assert!(requests[1].starts_with("POST /api/v1/classes/by-name/Hosts/objects HTTP/1.1"));
        assert!(requests[2].starts_with(
            "PATCH /api/v1/classes/by-name/Hosts/objects/by-name/srv-01/data HTTP/1.1"
        ));
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("content-type: application/json-patch+json"));
        assert!(requests[1].contains(r#""data":{"facts":{"os":"Fedora"}}"#));
    }

    #[test]
    fn create_data_applies_patch_to_an_empty_object() {
        let patch = ObjectDataPatchDocument::new([
            ObjectDataPatchOperation::Add {
                path: "/facts".to_string(),
                value: json!({"os": "Fedora"}),
            },
            ObjectDataPatchOperation::Add {
                path: "/publisher".to_string(),
                value: json!("ansible"),
            },
        ]);

        let data = initial_data_from_patch(&patch).expect("patch should initialize object data");

        assert_eq!(
            data,
            json!({"facts": {"os": "Fedora"}, "publisher": "ansible"})
        );
    }

    #[test]
    fn create_data_rejects_patches_with_missing_parents() {
        let patch = ObjectDataPatchDocument::new([ObjectDataPatchOperation::Add {
            path: "/facts/os".to_string(),
            value: json!("Fedora"),
        }]);

        let error = initial_data_from_patch(&patch)
            .expect_err("patch with a missing parent should not initialize data");

        assert!(error
            .to_string()
            .contains("does not apply to an empty JSON object"));
    }

    #[test]
    fn object_data_patch_input_rejects_empty_names() {
        let patch = ObjectDataPatchDocument::default();

        assert!(ObjectDataPatchInput::new("", "host", patch.clone()).is_err());
        assert!(ObjectDataPatchInput::new("Hosts", " ", patch).is_err());
    }

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

    fn http_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer).expect("request should be read");
            assert!(read > 0, "request ended before its body was complete");
            request.extend_from_slice(&buffer[..read]);

            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("valid content length"))
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                return String::from_utf8(request).expect("request should be UTF-8");
            }
        }
    }
}
