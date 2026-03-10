use chrono::NaiveDateTime;
use hubuum_client::{FilterOperator, UserPost};

use crate::domain::{CreatedUser, UserRecord};
use crate::errors::AppError;

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

impl HubuumGateway {
    pub fn create_user(&self, input: CreateUserInput) -> Result<CreatedUser, AppError> {
        let user = self.client.users().create(UserPost {
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

    pub fn list_users(&self, filter: UserFilter) -> Result<Vec<UserRecord>, AppError> {
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

        Ok(search
            .execute()?
            .into_iter()
            .map(UserRecord::from)
            .collect())
    }

    pub fn delete_user(&self, username: &str) -> Result<(), AppError> {
        let user = self.client.users().select_by_name(username)?;
        self.client.users().delete(user.id())?;
        Ok(())
    }
}
