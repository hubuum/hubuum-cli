use std::collections::{HashMap, HashSet};

use hubuum_client::{
    client::{sync::Handle as SyncHandle, sync::Resource, GetID},
    ApiError as ClientApiError, ApiResource, Class, ClassRelation, FilterOperator, Namespace,
    Object, ObjectRelation, QueryFilter,
};
use reqwest::StatusCode;

use crate::errors::AppError;
use crate::list_query::{
    validated_clause_to_query_filter, FilterValueResolver, ValidatedFilterClause,
};

use super::HubuumGateway;

const MAX_EQUALS_FILTER_VALUES: usize = 50;

impl HubuumGateway {
    pub(super) fn class_pair(
        &self,
        class_from: &str,
        class_to: &str,
    ) -> Result<(Class, Class), AppError> {
        Ok((
            self.client
                .classes()
                .select_by_name(class_from)?
                .resource()
                .clone(),
            self.client
                .classes()
                .select_by_name(class_to)?
                .resource()
                .clone(),
        ))
    }

    pub(super) fn class_map_from_classes<'a, I>(&self, classes: I) -> HashMap<i32, Class>
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
    ) -> Result<HashMap<i32, Class>, AppError>
    where
        I: IntoIterator<Item = i32>,
    {
        fetch_entities_for_ids(&self.client.classes(), unique_ids(class_ids))
    }

    pub(super) fn class_map_from_relation_ids(
        &self,
        relations: &[ClassRelation],
    ) -> Result<HashMap<i32, Class>, AppError> {
        fetch_entities_for_ids(
            &self.client.classes(),
            relations
                .iter()
                .flat_map(|relation| [relation.from_hubuum_class_id, relation.to_hubuum_class_id]),
        )
    }

    pub(super) fn object_map_for_relation(
        &self,
        relations: &[ObjectRelation],
        from_class_id: i32,
        to_class_id: i32,
    ) -> Result<HashMap<i32, Object>, AppError> {
        let object_ids =
            unique_ids(relations.iter().flat_map(|relation| {
                [relation.from_hubuum_object_id, relation.to_hubuum_object_id]
            }));
        let mut objects = HashMap::new();
        objects.extend(fetch_entities_for_ids(
            &self.client.objects(from_class_id),
            object_ids.iter().copied(),
        )?);
        objects.extend(fetch_entities_for_ids(
            &self.client.objects(to_class_id),
            object_ids,
        )?);

        Ok(objects)
    }

    pub(super) fn find_class_relation(
        &self,
        class_from_id: i32,
        class_to_id: i32,
    ) -> Result<ClassRelation, AppError> {
        Ok(self
            .client
            .class_relation()
            .find()
            .add_filter_equals("from_classes", class_from_id)
            .add_filter_equals("to_classes", class_to_id)
            .execute_expecting_single_result()?)
    }

    pub(super) fn find_class_relation_between(
        &self,
        class_a_id: i32,
        class_b_id: i32,
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
    ) -> Result<hubuum_client::client::sync::Handle<Class>, AppError> {
        Ok(self.client.classes().select_by_name(class_name)?)
    }

    pub(super) fn object_handle_by_name(
        &self,
        class_name: &str,
        object_name: &str,
    ) -> Result<SyncHandle<Object>, AppError> {
        let class = self.class_handle_by_name(class_name)?;
        match class.object_by_name(object_name) {
            Ok(object) => Ok(object),
            Err(error) if is_missing_api_error(&error) => {
                let matches = self
                    .client
                    .objects(class.id())
                    .find()
                    .add_filter_startswith("name", object_name)
                    .limit(2)
                    .execute()?;
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

    pub(super) fn namespace_id(&self, name: &str) -> Result<i32, AppError> {
        Ok(self.client.namespaces().select_by_name(name)?.id())
    }

    pub(super) fn namespace_map_from_ids<I>(
        &self,
        namespace_ids: I,
    ) -> Result<HashMap<i32, Namespace>, AppError>
    where
        I: IntoIterator<Item = i32>,
    {
        fetch_entities_for_ids(&self.client.namespaces(), unique_ids(namespace_ids))
    }

    pub(super) fn resolve_validated_filter(
        &self,
        clause: &ValidatedFilterClause,
    ) -> Result<QueryFilter, AppError> {
        let resolved_value = match clause.spec.resolver {
            FilterValueResolver::None => clause.value.clone(),
            FilterValueResolver::NamespaceNameToId => self.namespace_id(&clause.value)?.to_string(),
        };

        let mut resolved = clause.clone();
        resolved.value = resolved_value;
        Ok(validated_clause_to_query_filter(&resolved))
    }
}

fn is_missing_relation_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::ApiError(ClientApiError::HttpWithBody { status, .. })
            if *status == StatusCode::NOT_FOUND
    ) || matches!(error, AppError::ApiError(ClientApiError::EmptyResult(_)))
}

fn is_missing_api_error(error: &ClientApiError) -> bool {
    matches!(
        error,
        ClientApiError::HttpWithBody { status, .. } if *status == StatusCode::NOT_FOUND
    ) || matches!(error, ClientApiError::EmptyResult(_))
}

pub(super) fn find_entities_by_ids<T, I, F>(
    resource: &Resource<T>,
    objects: I,
    extract_id: F,
) -> Result<HashMap<i32, T::GetOutput>, AppError>
where
    T: ApiResource,
    I: IntoIterator,
    I::Item: Copy,
    F: Fn(I::Item) -> i32,
    T::GetOutput: GetID,
{
    fetch_entities_for_ids(resource, unique_ids(objects.into_iter().map(extract_id)))
}

fn unique_ids<I>(ids: I) -> Vec<i32>
where
    I: IntoIterator<Item = i32>,
{
    ids.into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn fetch_entities_for_ids<T, I>(
    resource: &Resource<T>,
    ids: I,
) -> Result<HashMap<i32, T::GetOutput>, AppError>
where
    T: ApiResource,
    I: IntoIterator<Item = i32>,
    T::GetOutput: GetID,
{
    let ids = unique_ids(ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut entities = HashMap::new();
    for chunk in ids.chunks(MAX_EQUALS_FILTER_VALUES) {
        let joined = chunk
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let results = resource
            .find()
            .add_filter("id", FilterOperator::Equals { is_negated: false }, joined)
            .execute()?;
        entities.extend(results.into_iter().map(|entity| (entity.id(), entity)));
    }

    Ok(entities)
}
