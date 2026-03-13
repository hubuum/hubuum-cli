use std::collections::HashMap;

use hubuum_client::{ObjectPatch, ObjectPost};

use crate::domain::ResolvedObjectRecord;
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
    SortFieldSpec,
};

use super::{shared::find_entities_by_ids, HubuumGateway};

#[derive(Debug, Clone)]
pub struct CreateObjectInput {
    pub name: String,
    pub class_name: String,
    pub namespace: String,
    pub description: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ObjectUpdateInput {
    pub name: String,
    pub class_name: String,
    pub rename: Option<String>,
    pub namespace: Option<String>,
    pub reclass: Option<String>,
    pub description: Option<String>,
    pub data: Option<serde_json::Value>,
}

impl HubuumGateway {
    pub fn list_object_names_for_class(&self, class_name: &str) -> Result<Vec<String>, AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        Ok(self
            .client
            .objects(class.id())
            .find()
            .execute()?
            .into_iter()
            .map(|object| object.name)
            .collect())
    }

    pub fn create_object(
        &self,
        input: CreateObjectInput,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let class = self.client.classes().select_by_name(&input.class_name)?;

        let object = self.client.objects(class.id()).create_raw(ObjectPost {
            name: input.name,
            hubuum_class_id: class.id(),
            namespace_id: namespace.id(),
            description: input.description,
            data: input.data,
        })?;

        let classmap = HashMap::from([(class.id(), class.resource().clone())]);
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ResolvedObjectRecord::new(&object, &classmap, &namespacemap))
    }

    pub fn object_details(
        &self,
        class_name: &str,
        object_name: &str,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        let namespace = self
            .client
            .namespaces()
            .select(object.resource().namespace_id)?;

        let classmap = HashMap::from([(class.id(), class.resource().clone())]);
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ResolvedObjectRecord::new(
            object.resource(),
            &classmap,
            &namespacemap,
        ))
    }

    pub fn delete_object(&self, class_name: &str, object_name: &str) -> Result<(), AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
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
        let class = self.client.classes().select_by_name(&class_filter.value)?;

        let filters = validated
            .iter()
            .filter(|clause| clause.spec.public_name != "class")
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page = apply_query_paging(
            self.client.objects(class.id()).find().filters(filters),
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
        let namespacemap =
            find_entities_by_ids(&self.client.namespaces(), page.items.iter(), |object| {
                object.namespace_id
            })?;

        Ok(PagedResult::from_page(page, query.limit, |object| {
            ResolvedObjectRecord::new(&object, &classmap, &namespacemap)
        }))
    }

    pub fn update_object(
        &self,
        input: ObjectUpdateInput,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().select_by_name(&input.class_name)?;
        let object = class.object_by_name(&input.name)?;

        let mut patch = ObjectPatch {
            data: input.data,
            ..ObjectPatch::default()
        };

        if let Some(namespace) = input.namespace {
            let namespace = self.client.namespaces().select_by_name(&namespace)?;
            patch.namespace_id = Some(namespace.id());
        }
        if let Some(reclass) = input.reclass {
            let reclass = self.client.classes().select_by_name(&reclass)?;
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
        let namespace = self.client.namespaces().select(result.namespace_id)?;

        let classmap = HashMap::from([(class.id(), class.resource().clone())]);
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ResolvedObjectRecord::new(&result, &classmap, &namespacemap))
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
        "namespace",
        "namespace_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::NamespaceNameToId),
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
    SortFieldSpec::new("namespace", "namespace_id"),
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
