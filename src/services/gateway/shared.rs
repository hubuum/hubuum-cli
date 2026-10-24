use hubuum_client::{ClassId, CollectionId, ObjectId};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use hubuum_client::{
    client::{sync::Handle as SyncHandle, sync::Resource, GetID},
    types::SortDirection,
    ApiError as ClientApiError, ApiResource, Class, ClassRelation, Collection, EntityTag,
    FilterOperator, Object, ObjectRelation, ObjectWithPath, QueryFilter,
};

use crate::diagnostics::client_call;
use crate::errors::AppError;
use crate::list_query::{FilterValueResolver, ValidatedFilterClause};

use super::HubuumGateway;

const MAX_EQUALS_FILTER_VALUES: usize = 50;

pub(super) fn parse_single_sort(
    sort: Option<&str>,
) -> Result<Option<(&str, SortDirection)>, AppError> {
    let Some(sort) = sort.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if sort.contains(',') {
        return Err(AppError::InvalidOption(
            "only one sort field is supported by the official hubuum_client request type"
                .to_string(),
        ));
    }
    let sort = sort.trim();
    if let Some(field) = sort.strip_prefix('-') {
        Ok(Some((field, SortDirection::Desc)))
    } else {
        Ok(Some((sort, SortDirection::Asc)))
    }
}

pub(super) fn class_collection_id(class: &Class) -> Option<CollectionId> {
    class
        .collection_id
        .or_else(|| class.collection.as_ref().map(|collection| collection.id))
}

pub(super) fn object_from_path(object: &ObjectWithPath) -> Result<Object, AppError> {
    Ok(serde_json::from_value(serde_json::to_value(object)?)?)
}

pub(super) fn required_entity_tag(
    etag: Option<&EntityTag>,
    resource: &str,
) -> Result<EntityTag, AppError> {
    etag.cloned().ok_or_else(|| {
        AppError::CommandExecutionError(format!(
            "server omitted the ETag required to modify the {resource} safely"
        ))
    })
}

impl HubuumGateway {
    pub(super) fn class_pair(
        &self,
        class_from: &str,
        class_to: &str,
    ) -> Result<(SyncHandle<Class>, SyncHandle<Class>), AppError> {
        let from = self.class_handle_by_name(class_from)?;
        let to = if class_from == class_to {
            from.clone()
        } else {
            self.class_handle_by_name(class_to)?
        };
        Ok((from, to))
    }

    pub(super) fn class_map_from_classes<'a, I>(&self, classes: I) -> HashMap<ClassId, Class>
    where
        I: IntoIterator<Item = &'a Class>,
    {
        classes
            .into_iter()
            .map(|class| (class.id, class.clone()))
            .collect()
    }

    pub(super) fn class_map_from_ids<I>(
        &self,
        class_ids: I,
    ) -> Result<HashMap<ClassId, Class>, AppError>
    where
        I: IntoIterator<Item = ClassId>,
    {
        fetch_entities_for_ids(&self.client().classes(), unique_ids(class_ids))
    }

    pub(super) fn class_map_with_known<'a, I>(
        &self,
        class_ids: I,
        known: impl IntoIterator<Item = &'a Class>,
    ) -> Result<HashMap<ClassId, Class>, AppError>
    where
        I: IntoIterator<Item = ClassId>,
    {
        let mut classes = self.class_map_from_classes(known);
        let missing = unique_ids(class_ids)
            .into_iter()
            .filter(|id| !classes.contains_key(id));
        let fetched = self.class_map_from_ids(missing)?;
        classes.extend(fetched);
        Ok(classes)
    }

    pub(super) fn object_map_for_relation(
        &self,
        relations: &[ObjectRelation],
        from_class_id: ClassId,
        to_class_id: ClassId,
    ) -> Result<HashMap<ObjectId, Object>, AppError> {
        let object_ids =
            unique_ids(relations.iter().flat_map(|relation| {
                [relation.from_hubuum_object_id, relation.to_hubuum_object_id]
            }));
        let mut objects = HashMap::new();
        objects.extend(fetch_entities_for_ids(
            &self.client().objects(from_class_id),
            object_ids.iter().copied(),
        )?);
        if from_class_id != to_class_id {
            objects.extend(fetch_entities_for_ids(
                &self.client().objects(to_class_id),
                object_ids,
            )?);
        }

        Ok(objects)
    }

    pub(super) fn find_class_relation(
        &self,
        class_from_id: ClassId,
        class_to_id: ClassId,
    ) -> Result<ClassRelation, AppError> {
        Ok(client_call("class_relation.one", || {
            self.client()
                .class_relation()
                .query()
                .filter(
                    "from_classes",
                    FilterOperator::Equals { is_negated: false },
                    class_from_id,
                )
                .filter(
                    "to_classes",
                    FilterOperator::Equals { is_negated: false },
                    class_to_id,
                )
                .one()
        })?)
    }

    pub(super) fn find_class_relation_between(
        &self,
        class_a_id: ClassId,
        class_b_id: ClassId,
    ) -> Result<ClassRelation, AppError> {
        match self.find_class_relation(class_a_id, class_b_id) {
            Ok(relation) => Ok(relation),
            Err(error) if is_missing_relation_error(&error) => {
                self.find_class_relation(class_b_id, class_a_id)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn class_handle_by_name(
        &self,
        class_name: &str,
    ) -> Result<SyncHandle<Class>, AppError> {
        Ok(client_call("classes.get_by_name", || {
            self.client().classes().get_by_name(class_name)
        })?)
    }

    pub(super) fn object_handle_by_name(
        &self,
        class_name: &str,
        object_name: &str,
    ) -> Result<SyncHandle<Object>, AppError> {
        let class = self.class_handle_by_name(class_name)?;
        match client_call("class.object_by_name", || class.object_by_name(object_name)) {
            Ok(object) => Ok(object),
            Err(error) if is_missing_api_error(&error) => {
                let matches = client_call("objects.list", || {
                    self.client()
                        .objects(class.id())
                        .query()
                        .filter(
                            "name",
                            FilterOperator::StartsWith { is_negated: false },
                            object_name,
                        )
                        .limit(2)
                        .list()
                })?;
                match matches.as_slice() {
                    [object] => Ok(SyncHandle::new(class.client().clone(), object.clone())),
                    [] => Err(AppError::EntityNotFound(format!(
                        "object '{object_name}' in class '{class_name}'"
                    ))),
                    _ => Err(AppError::MultipleEntitiesFound(format!(
                        "objects in class '{class_name}' starting with '{object_name}'"
                    ))),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn collection_id(&self, name: &str) -> Result<CollectionId, AppError> {
        Ok(client_call("collections.get_by_name", || {
            self.client().collections().get_by_name(name)
        })?
        .id())
    }

    pub(super) fn collection_map_from_ids<I>(
        &self,
        collection_ids: I,
    ) -> Result<HashMap<CollectionId, Collection>, AppError>
    where
        I: IntoIterator<Item = CollectionId>,
    {
        fetch_entities_for_ids(&self.client().collections(), unique_ids(collection_ids))
    }

    pub(super) fn collection_map_with_classes<'a, I>(
        &self,
        collection_ids: I,
        classes: impl IntoIterator<Item = &'a Class>,
    ) -> Result<HashMap<CollectionId, Collection>, AppError>
    where
        I: IntoIterator<Item = CollectionId>,
    {
        self.collection_map_with_known(
            collection_ids,
            classes
                .into_iter()
                .filter_map(|class| class.collection.as_ref()),
        )
    }

    pub(super) fn collection_map_with_known<'a, I>(
        &self,
        collection_ids: I,
        known: impl IntoIterator<Item = &'a Collection>,
    ) -> Result<HashMap<CollectionId, Collection>, AppError>
    where
        I: IntoIterator<Item = CollectionId>,
    {
        let mut collections = known
            .into_iter()
            .map(|collection| (collection.id, collection.clone()))
            .collect::<HashMap<_, _>>();
        let missing = unique_ids(collection_ids)
            .into_iter()
            .filter(|id| !collections.contains_key(id));
        let fetched = self.collection_map_from_ids(missing)?;
        collections.extend(fetched);
        Ok(collections)
    }

    pub(super) fn resolve_validated_filter(
        &self,
        clause: &ValidatedFilterClause,
    ) -> Result<QueryFilter, AppError> {
        clause.resolve_value(|resolver, value| {
            Ok(match resolver {
                FilterValueResolver::None => value.to_owned(),
                FilterValueResolver::ClassNameToId => {
                    self.class_handle_by_name(value)?.id().to_string()
                }
                FilterValueResolver::CollectionNameToId => self.collection_id(value)?.to_string(),
            })
        })
    }
}

fn is_missing_relation_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::ApiError(ClientApiError::HttpWithBody { status, .. })
            if status.as_u16() == 404
    ) || matches!(error, AppError::ApiError(ClientApiError::EmptyResult(_)))
}

fn is_missing_api_error(error: &ClientApiError) -> bool {
    matches!(
        error,
        ClientApiError::HttpWithBody { status, .. } if status.as_u16() == 404
    ) || matches!(error, ClientApiError::EmptyResult(_))
}

pub(super) fn find_entities_by_ids<T, I, F>(
    resource: &Resource<T>,
    objects: I,
    extract_id: F,
) -> Result<HashMap<T::Id, T::GetOutput>, AppError>
where
    T: ApiResource,
    I: IntoIterator,
    I::Item: Copy,
    F: Fn(I::Item) -> T::Id,
    T::Id: Hash,
    T::GetOutput: GetID<Id = T::Id>,
{
    fetch_entities_for_ids(resource, unique_ids(objects.into_iter().map(extract_id)))
}

fn unique_ids<I, Id>(ids: I) -> Vec<Id>
where
    I: IntoIterator<Item = Id>,
    Id: Eq + Hash,
{
    ids.into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn fetch_entities_for_ids<T, I>(
    resource: &Resource<T>,
    ids: I,
) -> Result<HashMap<T::Id, T::GetOutput>, AppError>
where
    T: ApiResource,
    I: IntoIterator<Item = T::Id>,
    T::Id: Hash,
    T::GetOutput: GetID<Id = T::Id>,
{
    let ids = unique_ids(ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut entities = HashMap::new();
    for chunk in ids.chunks(MAX_EQUALS_FILTER_VALUES) {
        let joined = chunk
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let results = client_call("QueryOp.all", || {
            resource
                .query()
                .filter("id", FilterOperator::Equals { is_negated: false }, joined)
                .all()
        })?;
        entities.extend(results.into_iter().map(|entity| (entity.id(), entity)));
    }

    Ok(entities)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{
        header::{HeaderName, HeaderValue},
        StatusCode,
    };
    use serde_json::json;

    use super::fetch_entities_for_ids;

    #[test]
    fn id_resolution_follows_cursor_pagination() {
        let transport = MockTransport::default();
        let mut first_page = TransportResponse::json(
            StatusCode::OK,
            &json!([{
                "id": 1,
                "name": "First",
                "description": "",
                "parent_collection_id": null,
                "revision": 1,
                "created_at": "2026-07-25T12:00:00Z",
                "updated_at": "2026-07-25T12:00:00Z"
            }]),
        )
        .expect("first page should serialize");
        first_page.headers.insert(
            HeaderName::from_static("x-next-cursor"),
            HeaderValue::from_static("page-2"),
        );
        transport.push_response(first_page);
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!([{
                    "id": 2,
                    "name": "Second",
                    "description": "",
                    "parent_collection_id": null,
                    "revision": 1,
                    "created_at": "2026-07-25T12:00:00Z",
                    "updated_at": "2026-07-25T12:00:00Z"
                }]),
            )
            .expect("second page should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should be valid")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));

        let collections = fetch_entities_for_ids(
            &client.collections(),
            [1.try_into().unwrap(), 2.try_into().unwrap()],
        )
        .expect("all collection pages should resolve");

        assert_eq!(collections.len(), 2);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .url
            .query_pairs()
            .any(|(key, value)| key == "cursor" && value == "page-2"));
    }
}
