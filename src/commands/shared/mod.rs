use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::hash::Hash;

use hubuum_client::{
    client::sync::Resource, client::GetID, ApiError, ApiResource, Authenticated, ClassRelation,
    FilterOperator, Object, ObjectRelation, SyncClient,
};

/// Extension trait for iterators to remove duplicates.
pub trait Uniqify: Iterator + Sized {
    /// Removes duplicate items from the iterator.
    fn uniqify(self) -> std::collections::hash_set::IntoIter<Self::Item>
    where
        Self::Item: Eq + Hash,
    {
        let set: HashSet<_> = self.collect();
        set.into_iter()
    }
}

impl<I: Iterator + Sized> Uniqify for I {}

/// Extension trait for iterators to join items into a comma-separated string.
pub trait Commafy: Iterator {
    /// Joins the items into a comma-separated string using their `Display` implementation.
    fn commafy(self) -> String
    where
        Self::Item: Display,
        Self: Sized,
    {
        self.map(|item| item.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Joins the items into a comma-separated string using their `Display` implementation, after removing duplicates.
    fn commafy_unique(self) -> String
    where
        Self::Item: Display + Eq + Hash,
        Self: Sized,
    {
        self.uniqify().commafy()
    }
}

impl<I: Iterator> Commafy for I {}

/// Extension trait for collections to provide `commafy_via` method.
pub trait CommafyVia: IntoIterator + Sized {
    /// Maps each item using the provided closure, removes duplicates, and joins them into a comma-separated string.
    fn commafy_via<F, T>(self, f: F) -> String
    where
        Self::IntoIter: Iterator<Item = Self::Item>,
        F: Fn(Self::Item) -> T,
        T: Display + Eq + Hash,
    {
        self.into_iter().map(f).commafy_unique()
    }
}

impl<T: IntoIterator> CommafyVia for T {}

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
    let ids = objects.commafy_via(extract_id);

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

pub fn find_class_relation(
    client: &SyncClient<Authenticated>,
    class_from_id: i32,
    class_to_id: i32,
) -> Result<ClassRelation, ApiError> {
    client
        .class_relation()
        .find()
        .add_filter_equals("from_classes", class_from_id)
        .add_filter_equals("to_classes", class_to_id)
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
    println!("prettify_slice_path: {path}");
    path.trim_start_matches("$")
        .replace("']['", ".")
        .replace("['", "")
        .replace("']", "")
}
