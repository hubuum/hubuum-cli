use std::collections::{HashMap, HashSet};

use hubuum_client::{
    client::sync::Resource, client::GetID, ApiError, ApiResource, Authenticated, Class,
    ClassRelation, FilterOperator, Namespace, Object, ObjectRelation, SyncClient,
};

use crate::errors::AppError;

pub fn ids_to_comma_separated_string<I, F>(objects: I, f: F) -> String
where
    I: IntoIterator,
    I::Item: Copy,
    F: Fn(I::Item) -> i32,
{
    objects
        .into_iter()
        .map(f)
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn find_entities_by_ids<T, I, F>(
    resource: &Resource<T>,
    objects: I,
    extract_id: F,
) -> Result<HashMap<i32, T::GetOutput>, ApiError>
where
    T: ApiResource,
    I: IntoIterator,
    I::Item: Copy,
    F: Fn(I::Item) -> i32,
    T::GetOutput: GetID,
{
    // Extract the comma-separated string of unique IDs
    let ids = ids_to_comma_separated_string(objects, extract_id);

    // Use the Resource<T> to add filter and execute the find operation

    let results = resource
        .find()
        .add_filter("id", FilterOperator::Equals { is_negated: false }, ids)
        .execute()?;

    let map = results
        .into_iter()
        .map(|entity| (entity.id(), entity))
        .collect::<HashMap<i32, T::GetOutput>>();

    Ok(map)
}

pub fn find_classes(
    client: &SyncClient<Authenticated>,
    class_from_name: &str,
    class_to_name: &str,
) -> Result<(Class, Class), AppError> {
    let class_from = find_class_by_name(client, class_from_name)?;
    let class_to = find_class_by_name(client, class_to_name)?;
    Ok((class_from, class_to))
}

pub fn find_class_by_name(
    client: &SyncClient<Authenticated>,
    name: &str,
) -> Result<Class, ApiError> {
    client
        .classes()
        .find()
        .add_filter_name_exact(name)
        .execute_expecting_single_result()
}

pub fn find_namespace_by_name(
    client: &SyncClient<Authenticated>,
    name: &str,
) -> Result<Namespace, ApiError> {
    client
        .namespaces()
        .find()
        .add_filter_name_exact(name)
        .execute_expecting_single_result()
}

pub fn find_class_relation(
    client: &SyncClient<Authenticated>,
    class_from_id: i32,
    class_to_id: i32,
) -> Result<ClassRelation, ApiError> {
    client
        .class_relation()
        .find()
        .add_filter_equals("from_classes", class_to_id)
        .add_filter_equals("to_classes", class_from_id)
        .execute_expecting_single_result()
}

pub fn find_object_by_name(
    client: &SyncClient<Authenticated>,
    class_id: i32,
    name: &str,
) -> Result<Object, ApiError> {
    client
        .objects(class_id)
        .find()
        .add_filter_name_exact(name)
        .execute_expecting_single_result()
}

pub fn find_object_relation(
    client: &SyncClient<Authenticated>,
    class_relation: &ClassRelation,
    object_from: &Object,
    object_to: &Object,
) -> Result<ObjectRelation, ApiError> {
    client
        .object_relation()
        .find()
        .add_filter_equals("id", class_relation.id)
        .add_filter_equals("to_objects", object_to.id)
        .add_filter_equals("from_objects", object_from.id)
        .execute_expecting_single_result()
}

// Convert $.['location'].['country'] to location.country (etc)
pub fn prettify_slice_path(path: &str) -> String {
    path.trim_start_matches("$.")
        .replace("['", "")
        .replace("']", "")
}
