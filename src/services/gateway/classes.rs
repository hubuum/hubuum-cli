use hubuum_client::{ClassPatch, ClassPost, FilterOperator};
use serde_json::Value;

use crate::domain::{build_related_class_tree, ClassRecord, ClassShowRecord, ObjectRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
    SortFieldSpec,
};

use super::{HubuumGateway, RelationTraversalOptions};

#[derive(Debug, Clone)]
pub struct CreateClassInput {
    pub name: String,
    pub collection: String,
    pub description: String,
    pub json_schema: Option<Value>,
    pub validate_schema: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ClassUpdateInput {
    pub name: String,
    pub rename: Option<String>,
    pub collection: Option<String>,
    pub description: Option<String>,
    pub json_schema: Option<Value>,
    pub validate_schema: Option<bool>,
}

impl HubuumGateway {
    pub fn list_class_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .classes()
            .query()
            .list()?
            .into_iter()
            .map(|class| class.name)
            .collect())
    }

    pub fn class_schema(&self, name: &str) -> Result<Option<Value>, AppError> {
        Ok(self
            .client
            .classes()
            .get_by_name(name)?
            .resource()
            .json_schema
            .clone())
    }

    pub fn create_class(&self, input: CreateClassInput) -> Result<ClassRecord, AppError> {
        let collection = self.client.collections().get_by_name(&input.collection)?;
        let class = self.client.classes().create_raw(ClassPost {
            name: input.name,
            collection_id: collection.id(),
            description: input.description,
            json_schema: input.json_schema,
            validate_schema: input.validate_schema,
        })?;
        Ok(ClassRecord::from(class))
    }

    pub fn class_show_details(
        &self,
        name: &str,
        options: &RelationTraversalOptions,
    ) -> Result<ClassShowRecord, AppError> {
        let class = self.client.classes().get_by_name(name)?;
        let objects = class
            .objects()?
            .into_iter()
            .map(|object| ObjectRecord::from(object.resource()))
            .collect();
        let related_graph = class
            .related_graph()
            .filter(
                "depth",
                FilterOperator::Lte { is_negated: false },
                options.max_depth,
            )
            .send()?;
        let collection_map = self.collection_map_from_ids(
            related_graph
                .classes
                .iter()
                .map(|related_class| related_class.collection_id)
                .collect::<Vec<_>>(),
        )?;

        Ok(ClassShowRecord {
            class: ClassRecord::from(class.resource()),
            objects,
            related_classes: build_related_class_tree(
                &related_graph.classes,
                &collection_map,
                class.id().into(),
                !options.include_self_class,
            ),
        })
    }

    pub fn delete_class(&self, name: &str) -> Result<(), AppError> {
        self.client.classes().get_by_name(name)?.delete()?;
        Ok(())
    }

    pub fn update_class(&self, input: ClassUpdateInput) -> Result<ClassRecord, AppError> {
        let class = self.client.classes().get_by_name(&input.name)?;

        let collection_id = match input.collection {
            Some(collection) => self.client.collections().get_by_name(&collection)?.id(),
            None => class.resource().collection.id,
        };

        let updated = self.client.classes().update_raw(
            class.id(),
            ClassPatch {
                name: input.rename,
                collection_id,
                description: input.description,
                json_schema: input.json_schema,
                validate_schema: input.validate_schema,
            },
        )?;

        Ok(ClassRecord::from(updated))
    }

    pub fn list_classes(&self, query: &ListQuery) -> Result<PagedResult<ClassRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, CLASS_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, CLASS_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page = apply_query_paging(
            self.client.classes().query().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        Ok(PagedResult::from_page(page, ClassRecord::from))
    }
}

pub(crate) const CLASS_FILTER_SPECS: &[FilterFieldSpec] = &[
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
        "collection",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::CollectionNameToId),
    FilterFieldSpec::new(
        "validate_schema",
        "validate_schema",
        FilterOperatorProfile::Boolean,
        FilterValueProfile::Boolean,
    ),
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
        "json_schema",
        "json_schema",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
];

pub(crate) const CLASS_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("collection", "collection"),
    SortFieldSpec::new("validate_schema", "validate_schema"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];
