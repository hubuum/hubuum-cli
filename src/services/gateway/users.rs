use chrono::{DateTime, NaiveDateTime, Utc};
use hubuum_client::{FilterOperator, HubuumDateTime, NewTokenRequest, Permissions, UserPatch};
use std::str::FromStr;

use crate::domain::{CreatedUser, PrincipalTokenRecord, UserRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone, Default)]
pub struct UserFilter {
    pub username: Option<String>,
    pub email: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone)]
pub struct CreateUserInput {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct UserUpdateInput {
    pub username: String,
    pub rename: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTokenInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub expires_at: Option<String>,
    pub scopes: Vec<String>,
}

impl HubuumGateway {
    pub fn list_user_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .list_users(&ListQuery {
                limit: Some(200),
                ..ListQuery::default()
            })?
            .items
            .into_iter()
            .map(|user| user.0.name)
            .collect())
    }

    pub fn create_user(&self, input: CreateUserInput) -> Result<CreatedUser, AppError> {
        // Create user with name/email/password
        let mut create = self
            .client
            .users()
            .create_checked()
            .name(input.username.clone())
            .password(input.password.clone());
        if let Some(email) = input.email {
            create = create.email(email);
        }
        let user = create.send()?;

        Ok(CreatedUser {
            user: UserRecord::from(user),
            password: input.password,
        })
    }

    pub fn find_user(&self, filter: UserFilter) -> Result<UserRecord, AppError> {
        let mut search = self.client.users().query();
        if let Some(username) = filter.username {
            search = search.filter(
                "name",
                FilterOperator::Equals { is_negated: false },
                username,
            );
        }
        if let Some(email) = filter.email {
            search = search.filter("email", FilterOperator::Equals { is_negated: false }, email);
        }
        if let Some(created_at) = filter.created_at {
            search = search.filter(
                "created_at",
                FilterOperator::Equals { is_negated: false },
                created_at.to_string(),
            );
        }
        if let Some(updated_at) = filter.updated_at {
            search = search.filter(
                "updated_at",
                FilterOperator::Equals { is_negated: false },
                updated_at.to_string(),
            );
        }
        let user = search.one()?;
        Ok(UserRecord::from(user))
    }

    pub fn list_users(&self, query: &ListQuery) -> Result<PagedResult<UserRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, USER_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, USER_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let mut query_op = self.client.users().query();
        for filter in filters {
            query_op = query_op.filter(&filter.key, filter.operator, &filter.value);
        }

        let page = apply_query_paging(query_op, query, &validated_sorts).page()?;
        Ok(PagedResult::from_page(page, UserRecord::from))
    }

    pub fn delete_user(&self, username: &str) -> Result<(), AppError> {
        let user = self.client.users().get_by_name(username)?;
        self.client.users().delete(user.id())?;
        Ok(())
    }

    pub fn update_user(&self, input: UserUpdateInput) -> Result<UserRecord, AppError> {
        // The principal model does not expose username renaming via the user
        // update body (`UserPatch` excludes `name`; renaming lives on the principal).
        // Reject `--rename` explicitly rather than silently ignoring it.
        if input.rename.is_some() {
            return Err(AppError::CommandExecutionError(
                "renaming a user is not supported by the server in this version".into(),
            ));
        }

        let handle = self.client.users().get_by_name(&input.username)?;
        let updated = self
            .client
            .users()
            .update(handle.id())
            .params(UserPatch {
                email: input.email,
                proper_name: None,
            })
            .send()?;

        Ok(UserRecord::from(updated))
    }

    pub fn user_tokens(&self, username: &str) -> Result<Vec<PrincipalTokenRecord>, AppError> {
        let handle = self.client.users().get_by_name(username)?;
        let tokens = handle.tokens()?;
        Ok(tokens.into_iter().map(PrincipalTokenRecord::from).collect())
    }

    pub fn user_token_create(
        &self,
        username: &str,
        input: NewTokenInput,
    ) -> Result<String, AppError> {
        let handle = self.client.users().get_by_name(username)?;
        let mut req = NewTokenRequest::new();

        if let Some(n) = input.name {
            req = req.name(n);
        }
        if let Some(d) = input.description {
            req = req.description(d);
        }
        if let Some(exp_str) = input.expires_at.as_deref() {
            let dt = DateTime::parse_from_rfc3339(exp_str)
                .map_err(|e| {
                    AppError::CommandExecutionError(format!(
                        "invalid --expires-at (expected RFC3339, e.g. 2026-12-31T23:59:59Z): {e}"
                    ))
                })?
                .with_timezone(&Utc);
            req = req.expires_at(HubuumDateTime(dt));
        }
        if !input.scopes.is_empty() {
            let scopes: Result<Vec<_>, _> = input
                .scopes
                .iter()
                .map(|s| {
                    Permissions::from_str(s).map_err(|_| {
                        AppError::CommandExecutionError(format!("unknown permission scope: {}", s))
                    })
                })
                .collect();
            req = req.scopes(scopes?);
        }

        Ok(handle.tokens_create(req)?)
    }

    pub fn user_token_revoke(&self, username: &str, token_id: i32) -> Result<(), AppError> {
        let handle = self.client.users().get_by_name(username)?;
        handle.token_revoke(token_id)?;
        Ok(())
    }

    pub fn set_user_password(&self, username: &str, password: &str) -> Result<(), AppError> {
        let handle = self.client.users().get_by_name(username)?;
        handle.set_password(password)?;
        Ok(())
    }
}

pub(crate) const USER_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "username",
        "username",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "email",
        "email",
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

pub(crate) const USER_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("username", "username"),
    SortFieldSpec::new("email", "email"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];
