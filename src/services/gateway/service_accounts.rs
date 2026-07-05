use std::str::FromStr;

use crate::domain::{PrincipalTokenRecord, ServiceAccountRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::{users::NewTokenInput, HubuumGateway};

#[derive(Debug, Clone)]
pub struct CreateServiceAccountInput {
    pub name: String,
    pub description: Option<String>,
    pub owner_group_id: i32,
}

impl HubuumGateway {
    pub fn create_service_account(
        &self,
        input: CreateServiceAccountInput,
    ) -> Result<ServiceAccountRecord, AppError> {
        let sa = self
            .client
            .service_accounts()
            .create()
            .params(hubuum_client::ServiceAccountPost {
                name: input.name,
                description: input.description,
                owner_group_id: input.owner_group_id,
            })
            .send()?;

        Ok(ServiceAccountRecord::from(sa))
    }

    pub fn list_service_accounts(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<ServiceAccountRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, SERVICE_ACCOUNT_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, SERVICE_ACCOUNT_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let mut query_op = self.client.service_accounts().query();
        for filter in filters {
            query_op = query_op.add_filter(&filter.key, filter.operator, &filter.value);
        }

        let page = apply_query_paging(query_op, query, &validated_sorts).page()?;
        Ok(PagedResult::from_page(
            page,
            query.limit,
            ServiceAccountRecord::from,
        ))
    }

    pub fn service_account(&self, name: &str) -> Result<ServiceAccountRecord, AppError> {
        let sa = self.client.service_accounts().select_by_name(name)?;
        Ok(ServiceAccountRecord::from(sa.resource().clone()))
    }

    pub fn delete_service_account(&self, name: &str) -> Result<(), AppError> {
        let sa = self.client.service_accounts().select_by_name(name)?;
        self.client.service_accounts().delete(sa.id())?;
        Ok(())
    }

    pub fn disable_service_account(&self, name: &str) -> Result<ServiceAccountRecord, AppError> {
        let handle = self.client.service_accounts().select_by_name(name)?;
        let disabled = handle.disable()?;
        Ok(ServiceAccountRecord::from(disabled))
    }

    pub fn service_account_tokens(
        &self,
        name: &str,
    ) -> Result<Vec<PrincipalTokenRecord>, AppError> {
        let handle = self.client.service_accounts().select_by_name(name)?;
        let tokens = handle.tokens()?;
        Ok(tokens.into_iter().map(PrincipalTokenRecord::from).collect())
    }

    pub fn service_account_token_create(
        &self,
        name: &str,
        input: NewTokenInput,
    ) -> Result<String, AppError> {
        let handle = self.client.service_accounts().select_by_name(name)?;
        let mut req = hubuum_client::NewTokenRequest::new();

        if let Some(n) = input.name {
            req = req.name(n);
        }
        if let Some(d) = input.description {
            req = req.description(d);
        }
        if let Some(exp_str) = input.expires_at.as_deref() {
            let dt = chrono::DateTime::parse_from_rfc3339(exp_str)
                .map_err(|e| {
                    AppError::CommandExecutionError(format!(
                        "invalid --expires-at (expected RFC3339, e.g. 2026-12-31T23:59:59Z): {e}"
                    ))
                })?
                .with_timezone(&chrono::Utc);
            req = req.expires_at(hubuum_client::HubuumDateTime(dt));
        }
        if !input.scopes.is_empty() {
            let scopes: Result<Vec<_>, _> = input
                .scopes
                .iter()
                .map(|s| {
                    hubuum_client::Permissions::from_str(s).map_err(|_| {
                        AppError::CommandExecutionError(format!("unknown permission scope: {}", s))
                    })
                })
                .collect();
            req = req.scopes(scopes?);
        }

        Ok(handle.tokens_create(req)?)
    }

    pub fn service_account_token_revoke(&self, name: &str, token_id: i32) -> Result<(), AppError> {
        let handle = self.client.service_accounts().select_by_name(name)?;
        handle.token_revoke(token_id)?;
        Ok(())
    }
}

pub(crate) const SERVICE_ACCOUNT_FILTER_SPECS: &[FilterFieldSpec] = &[
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
        "owner_group_id",
        "owner_group_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
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

pub(crate) const SERVICE_ACCOUNT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("owner_group_id", "owner_group_id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];
