use chrono::NaiveDateTime;
use hubuum_client::{FilterOperator, UserPatch, UserPost};

use crate::domain::{CreatedUser, UserRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile, ListQuery, PagedResult,
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
        let user = self.client.users().create_raw(UserPost {
            username: input.username,
            email: input.email,
            password: input.password.clone(),
        })?;

        Ok(CreatedUser {
            user: UserRecord::from(user),
            password: input.password,
        })
    }

    pub fn find_user(&self, filter: UserFilter) -> Result<UserRecord, AppError> {
        let mut search = self.client.users().find();
        if let Some(username) = filter.username {
            search = search.add_filter(
                "username",
                FilterOperator::IContains { is_negated: false },
                username,
            );
        }
        if let Some(email) = filter.email {
            search = search.add_filter(
                "email",
                FilterOperator::IContains { is_negated: false },
                email,
            );
        }
        if let Some(created_at) = filter.created_at {
            search = search.add_filter(
                "created_at",
                FilterOperator::Equals { is_negated: false },
                created_at.to_string(),
            );
        }
        if let Some(updated_at) = filter.updated_at {
            search = search.add_filter(
                "updated_at",
                FilterOperator::Equals { is_negated: false },
                updated_at.to_string(),
            );
        }
        let user = search.execute_expecting_single_result()?;
        Ok(UserRecord::from(user))
    }

    pub fn list_users(&self, query: &ListQuery) -> Result<PagedResult<UserRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, USER_FILTER_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page = apply_query_paging(self.client.users().find().filters(filters), query).page()?;
        Ok(PagedResult::from_page(page, query.limit, UserRecord::from))
    }

    pub fn delete_user(&self, username: &str) -> Result<(), AppError> {
        let user = self.client.users().select_by_name(username)?;
        self.client.users().delete(user.id())?;
        Ok(())
    }

    pub fn update_user(&self, input: UserUpdateInput) -> Result<UserRecord, AppError> {
        let user = self.client.users().select_by_name(&input.username)?;
        let updated = self.client.users().update_raw(
            user.id(),
            UserPatch {
                username: input.rename,
                email: input.email,
            },
        )?;

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
