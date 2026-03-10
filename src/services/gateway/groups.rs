use hubuum_client::{FilterOperator, GroupPost};

use crate::domain::{GroupDetails, GroupRecord, UserRecord};
use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateGroupInput {
    pub groupname: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct GroupFilter {
    pub name: Option<String>,
    pub name_startswith: Option<String>,
    pub name_endswith: Option<String>,
    pub description: Option<String>,
}

impl HubuumGateway {
    pub fn list_group_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .groups()
            .find()
            .execute()?
            .into_iter()
            .map(|group| group.groupname)
            .collect())
    }

    pub fn create_group(&self, input: CreateGroupInput) -> Result<GroupRecord, AppError> {
        let group = self.client.groups().create(GroupPost {
            groupname: input.groupname,
            description: input.description,
        })?;
        Ok(GroupRecord::from(group))
    }

    pub fn add_user_to_group(&self, group_name: &str, username: &str) -> Result<(), AppError> {
        let group = self.client.groups().select_by_name(group_name)?;
        let user = self.client.users().select_by_name(username)?;
        group.add_user(user.id())?;
        Ok(())
    }

    pub fn remove_user_from_group(&self, group_name: &str, username: &str) -> Result<(), AppError> {
        let group = self.client.groups().select_by_name(group_name)?;
        let user = self.client.users().select_by_name(username)?;
        group.remove_user(user.id())?;
        Ok(())
    }

    pub fn group_details(&self, group_name: &str) -> Result<GroupDetails, AppError> {
        let group = self.client.groups().select_by_name(group_name)?;
        let members = group
            .members()?
            .into_iter()
            .map(|user| UserRecord::from(user.resource()))
            .collect::<Vec<_>>();

        Ok(GroupDetails {
            group: GroupRecord::from(group.resource()),
            members,
        })
    }

    pub fn list_groups(&self, filter: GroupFilter) -> Result<Vec<GroupRecord>, AppError> {
        let mut search = self.client.groups().find();

        if let Some(name) = filter.name {
            search = search.add_filter(
                "groupname",
                FilterOperator::IContains { is_negated: false },
                name,
            );
        }
        if let Some(name_startswith) = filter.name_startswith {
            search = search.add_filter(
                "groupname",
                FilterOperator::StartsWith { is_negated: false },
                name_startswith,
            );
        }
        if let Some(name_endswith) = filter.name_endswith {
            search = search.add_filter(
                "groupname",
                FilterOperator::EndsWith { is_negated: false },
                name_endswith,
            );
        }
        if let Some(description) = filter.description {
            search = search.add_filter(
                "description",
                FilterOperator::IContains { is_negated: false },
                description,
            );
        }

        Ok(search
            .execute()?
            .into_iter()
            .map(GroupRecord::from)
            .collect())
    }
}
