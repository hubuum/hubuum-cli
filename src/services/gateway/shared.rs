use std::collections::{HashMap, HashSet};

use hubuum_client::{
    client::{sync::Resource, GetID},
    ApiResource, Class, ClassRelation, FilterOperator, Object, ObjectRelation, QueryFilter,
};

use crate::errors::AppError;
use crate::list_query::{
    validated_clause_to_query_filter, FilterValueResolver, ValidatedFilterClause,
};

use super::HubuumGateway;

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

    pub(super) fn class_map_from_relation_ids(
        &self,
        relations: &[ClassRelation],
    ) -> Result<HashMap<i32, Class>, AppError> {
        let mut class_ids = HashSet::new();
        for relation in relations {
            class_ids.insert(relation.from_hubuum_class_id);
            class_ids.insert(relation.to_hubuum_class_id);
        }

        let joined = class_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        Ok(self
            .client
            .classes()
            .find()
            .add_filter_id(joined)
            .execute()?
            .into_iter()
            .map(|class| (class.id, class))
            .collect())
    }

    pub(super) fn object_map_for_relation(
        &self,
        relations: &[ObjectRelation],
        from_class_id: i32,
        to_class_id: i32,
    ) -> Result<HashMap<i32, Object>, AppError> {
        let joined = relations
            .iter()
            .flat_map(|relation| {
                [
                    relation.from_hubuum_object_id.to_string(),
                    relation.to_hubuum_object_id.to_string(),
                ]
            })
            .collect::<Vec<_>>()
            .join(",");

        let mut objects = HashMap::new();

        for object in self
            .client
            .objects(from_class_id)
            .find()
            .add_filter_equals("id", &joined)
            .execute()?
        {
            objects.insert(object.id, object);
        }

        for object in self
            .client
            .objects(to_class_id)
            .find()
            .add_filter_equals("id", &joined)
            .execute()?
        {
            objects.insert(object.id, object);
        }

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

    pub(super) fn find_object_by_name(
        &self,
        class_id: i32,
        name: &str,
    ) -> Result<Object, AppError> {
        Ok(self
            .client
            .objects(class_id)
            .find()
            .add_filter_name_exact(name)
            .execute_expecting_single_result()?)
    }

    pub(super) fn find_object_relation(
        &self,
        class_relation: &ClassRelation,
        object_from: &Object,
        object_to: &Object,
    ) -> Result<ObjectRelation, AppError> {
        Ok(self
            .client
            .object_relation()
            .find()
            .add_filter_equals("id", class_relation.id)
            .add_filter_equals("to_objects", object_to.id)
            .add_filter_equals("from_objects", object_from.id)
            .execute_expecting_single_result()?)
    }

    pub(super) fn namespace_id(&self, name: &str) -> Result<i32, AppError> {
        Ok(self.client.namespaces().select_by_name(name)?.id())
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
    let ids = objects
        .into_iter()
        .map(extract_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let results = resource
        .find()
        .add_filter("id", FilterOperator::Equals { is_negated: false }, ids)
        .execute()?;

    Ok(results
        .into_iter()
        .map(|entity| (entity.id(), entity))
        .collect())
}
