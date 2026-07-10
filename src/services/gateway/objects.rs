use std::collections::HashMap;

use hubuum_client::{FilterOperator, ObjectPatch, ObjectPost};
use serde_json::Value;

use crate::domain::{build_related_object_tree, ObjectShowRecord, ResolvedObjectRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
    SortFieldSpec,
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
            hubuum_class_id: class.id().into(),
            collection_id: collection.id().into(),
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
        let object_record = ResolvedObjectRecord::new(object.resource(), &classmap, &collectionmap);
        let related_graph = object
            .related_graph()
            .filter(
                "depth",
                FilterOperator::Lte { is_negated: false },
                options.max_depth,
            )
            .fetch()?;
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
    ) -> Result<PagedResult<ResolvedObjectRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, OBJECT_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, OBJECT_SORT_SPECS)?;
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
                limit: query.limit,
                returned_count: 0,
            });
        }

        let classmap = find_entities_by_ids(&self.client.classes(), page.items.iter(), |object| {
            object.hubuum_class_id
        })?;
        let collectionmap =
            find_entities_by_ids(&self.client.collections(), page.items.iter(), |object| {
                object.collection_id
            })?;

        Ok(PagedResult::from_page(page, query.limit, |object| {
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
            patch.collection_id = Some(collection.id().into());
        }
        if let Some(reclass) = input.reclass {
            let reclass = self.client.classes().get_by_name(&reclass)?;
            patch.hubuum_class_id = Some(reclass.id().into());
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
    use crate::list_query::resolve_filter_field_spec;

    use super::OBJECT_FILTER_SPECS;

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
}
