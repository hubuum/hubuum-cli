use hubuum_client::{NamespacePatch, NamespacePost};

use crate::domain::{
    GroupPermissionsRecord, GroupPermissionsSummary, NamespacePermission, NamespacePermissionsView,
    NamespaceRecord,
};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateNamespaceInput {
    pub name: String,
    pub description: String,
    pub owner: String,
}

#[derive(Debug, Clone)]
pub struct NamespaceUpdateInput {
    pub name: String,
    pub rename: Option<String>,
    pub description: Option<String>,
}

impl HubuumGateway {
    pub fn list_namespace_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .namespaces()
            .find()
            .execute()?
            .into_iter()
            .map(|namespace| namespace.name)
            .collect())
    }

    pub fn create_namespace(
        &self,
        input: CreateNamespaceInput,
    ) -> Result<NamespaceRecord, AppError> {
        let group = self.client.groups().select_by_name(&input.owner)?;
        let namespace = self.client.namespaces().create_raw(NamespacePost {
            name: input.name,
            description: input.description,
            group_id: group.id(),
        })?;
        Ok(NamespaceRecord::from(namespace))
    }

    pub fn list_namespaces(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<NamespaceRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, NAMESPACE_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, NAMESPACE_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page = apply_query_paging(
            self.client.namespaces().find().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        Ok(PagedResult::from_page(
            page,
            query.limit,
            NamespaceRecord::from,
        ))
    }

    pub fn get_namespace(&self, name: &str) -> Result<NamespaceRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(name)?;
        Ok(NamespaceRecord::from(namespace.resource()))
    }

    pub fn delete_namespace(&self, name: &str) -> Result<(), AppError> {
        let namespace = self.client.namespaces().select_by_name(name)?;
        self.client.namespaces().delete(namespace.id())?;
        Ok(())
    }

    pub fn update_namespace(
        &self,
        input: NamespaceUpdateInput,
    ) -> Result<NamespaceRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.name)?;
        let updated = self.client.namespaces().update_raw(
            namespace.id(),
            NamespacePatch {
                name: input.rename,
                description: input.description,
            },
        )?;

        Ok(NamespaceRecord::from(updated))
    }

    pub fn list_namespace_permissions(
        &self,
        name: &str,
    ) -> Result<NamespacePermissionsView, AppError> {
        let permissions = self
            .client
            .namespaces()
            .select_by_name(name)?
            .permissions()?;
        let entries = permissions
            .iter()
            .cloned()
            .map(GroupPermissionsRecord::from)
            .collect::<Vec<_>>();
        let summary = permissions
            .into_iter()
            .map(GroupPermissionsSummary::from)
            .collect::<Vec<_>>();

        Ok(NamespacePermissionsView { entries, summary })
    }

    pub fn grant_namespace_permissions(
        &self,
        namespace_name: &str,
        group_name: &str,
        permissions: &[NamespacePermission],
    ) -> Result<(), AppError> {
        let namespace = self.client.namespaces().select_by_name(namespace_name)?;
        let group = self.client.groups().select_by_name(group_name)?;
        namespace.grant_permissions(
            group.id(),
            permissions
                .iter()
                .map(|permission| permission.api_name())
                .collect(),
        )?;
        Ok(())
    }
}

pub(crate) const NAMESPACE_FILTER_SPECS: &[FilterFieldSpec] = &[
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
];

pub(crate) const NAMESPACE_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];
