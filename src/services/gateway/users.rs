use chrono::NaiveDateTime;

use crate::domain::{CreatedUser, UserRecord};
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

impl HubuumGateway {
    pub fn create_user(&self, input: CreateUserInput) -> Result<CreatedUser, AppError> {
        // Create user with name/email/password
        let user = self.client.users().create()
            .params(hubuum_client::UserPost {
                name: input.username.clone(),
                password: input.password.clone(),
                email: input.email.clone(),
                proper_name: None,
            })
            .send()?;

        Ok(CreatedUser {
            user: UserRecord::from(user),
            password: input.password,
        })
    }

    pub fn find_user(&self, filter: UserFilter) -> Result<UserRecord, AppError> {
        let mut search = self.client.users().query();
        if let Some(username) = filter.username {
            search = search.add_filter_equals("name", username);
        }
        if let Some(email) = filter.email {
            search = search.add_filter_equals("email", email);
        }
        if let Some(created_at) = filter.created_at {
            search = search.add_filter_equals("created_at", created_at.to_string());
        }
        if let Some(updated_at) = filter.updated_at {
            search = search.add_filter_equals("updated_at", updated_at.to_string());
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
            query_op = query_op.add_filter(&filter.key, filter.operator, &filter.value);
        }

        let page = apply_query_paging(query_op, query, &validated_sorts).page()?;
        Ok(PagedResult::from_page(page, query.limit, UserRecord::from))
    }

    pub fn delete_user(&self, username: &str) -> Result<(), AppError> {
        let user = self.client.users().select_by_name(username)?;
        self.client.users().delete(user.id())?;
        Ok(())
    }

    pub fn update_user(&self, input: UserUpdateInput) -> Result<UserRecord, AppError> {
        // The 0.0.3 principal model does not expose username renaming via the user
        // update body (`UserPatch` excludes `name`; renaming lives on the principal).
        // Reject `--rename` explicitly rather than silently ignoring it.
        if input.rename.is_some() {
            return Err(AppError::CommandExecutionError(
                "renaming a user is not supported by the server in this version".into(),
            ));
        }

        let handle = self.client.users().select_by_name(&input.username)?;
        let updated = self.client.users().update(handle.id())
            .params(hubuum_client::UserPatch {
                email: input.email,
                proper_name: None,
            })
            .send()?;

        Ok(UserRecord::from(updated))
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
