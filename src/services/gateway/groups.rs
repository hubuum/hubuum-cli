use hubuum_client::{GroupPatch, GroupPost};

use crate::domain::{GroupDetails, GroupRecord, UserRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile, ListQuery, PagedResult,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateGroupInput {
    pub groupname: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct GroupUpdateInput {
    pub groupname: String,
    pub rename: Option<String>,
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
        let group = self.client.groups().create_raw(GroupPost {
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

    pub fn update_group(&self, input: GroupUpdateInput) -> Result<GroupRecord, AppError> {
        let group = self.client.groups().select_by_name(&input.groupname)?;
        let updated = self.client.groups().update_raw(
            group.id(),
            GroupPatch {
                groupname: input.rename,
                description: input.description,
            },
        )?;

        Ok(GroupRecord::from(updated))
    }

    pub fn list_groups(&self, query: &ListQuery) -> Result<PagedResult<GroupRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, GROUP_FILTER_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page =
            apply_query_paging(self.client.groups().find().filters(filters), query).page()?;
        Ok(PagedResult::from_page(page, query.limit, GroupRecord::from))
    }
}

pub(crate) const GROUP_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "groupname",
        "groupname",
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
