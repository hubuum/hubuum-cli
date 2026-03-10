use hubuum_client::{ClassPatch, ClassPost};

use crate::domain::{ClassDetails, ClassRecord, ObjectRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateClassInput {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub json_schema: Option<serde_json::Value>,
    pub validate_schema: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ClassUpdateInput {
    pub name: String,
    pub rename: Option<String>,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub json_schema: Option<serde_json::Value>,
    pub validate_schema: Option<bool>,
}

impl HubuumGateway {
    pub fn list_class_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .classes()
            .find()
            .execute()?
            .into_iter()
            .map(|class| class.name)
            .collect())
    }

    pub fn create_class(&self, input: CreateClassInput) -> Result<ClassRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let class = self.client.classes().create_raw(ClassPost {
            name: input.name,
            namespace_id: namespace.id(),
            description: input.description,
            json_schema: input.json_schema,
            validate_schema: input.validate_schema,
        })?;
        Ok(ClassRecord::from(class))
    }

    pub fn class_details(&self, name: &str) -> Result<ClassDetails, AppError> {
        let class = self.client.classes().select_by_name(name)?;
        let objects = class
            .objects()?
            .into_iter()
            .map(|object| ObjectRecord::from(object.resource()))
            .collect();

        Ok(ClassDetails {
            class: ClassRecord::from(class.resource()),
            objects,
        })
    }

    pub fn delete_class(&self, name: &str) -> Result<(), AppError> {
        self.client.classes().select_by_name(name)?.delete()?;
        Ok(())
    }

    pub fn update_class(&self, input: ClassUpdateInput) -> Result<ClassRecord, AppError> {
        let class = self.client.classes().select_by_name(&input.name)?;

        let namespace_id = match input.namespace {
            Some(namespace) => self.client.namespaces().select_by_name(&namespace)?.id(),
            None => class.resource().namespace.id,
        };

        let updated = self.client.classes().update_raw(
            class.id(),
            ClassPatch {
                name: input.rename,
                namespace_id,
                description: input.description,
                json_schema: input.json_schema,
                validate_schema: input.validate_schema,
            },
        )?;

        Ok(ClassRecord::from(updated))
    }

    pub fn list_classes(&self, query: &ListQuery) -> Result<PagedResult<ClassRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, CLASS_FILTER_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page =
            apply_query_paging(self.client.classes().find().filters(filters), query).page()?;
        Ok(PagedResult::from_page(page, query.limit, ClassRecord::from))
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
        "namespace",
        "namespace",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::NamespaceNameToId),
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
