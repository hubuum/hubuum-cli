use crate::domain::{GroupDetails, GroupRecord, PrincipalMemberRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
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
            .query()
            .list()?
            .into_iter()
            .map(|group| group.groupname.clone())
            .collect())
    }

    pub fn group_id_by_name(&self, group_name: &str) -> Result<i32, AppError> {
        Ok(self.client.groups().get_by_name(group_name)?.id().into())
    }

    pub fn create_group(&self, input: CreateGroupInput) -> Result<GroupRecord, AppError> {
        let group = self
            .client
            .groups()
            .create_checked()
            .groupname(input.groupname)
            .description(input.description)
            .send()?;
        Ok(GroupRecord::from(group))
    }

    pub fn add_user_to_group(&self, group_name: &str, username: &str) -> Result<(), AppError> {
        let group = self.client.groups().get_by_name(group_name)?;
        let principal_id = self.client.users().get_by_name(username)?.id();
        group.add_member(principal_id)?;
        Ok(())
    }

    pub fn remove_user_from_group(&self, group_name: &str, username: &str) -> Result<(), AppError> {
        let group = self.client.groups().get_by_name(group_name)?;
        let principal_id = self.client.users().get_by_name(username)?.id();
        group.remove_member(principal_id)?;
        Ok(())
    }

    pub fn group_details(&self, group_name: &str) -> Result<GroupDetails, AppError> {
        let handle = self.client.groups().get_by_name(group_name)?;
        let members = handle
            .members()?
            .into_iter()
            .map(PrincipalMemberRecord::from)
            .collect::<Vec<_>>();

        Ok(GroupDetails {
            group: GroupRecord::from(handle.resource().clone()),
            members,
        })
    }

    pub fn update_group(&self, input: GroupUpdateInput) -> Result<GroupRecord, AppError> {
        let handle = self.client.groups().get_by_name(&input.groupname)?;
        let updated = self
            .client
            .groups()
            .update(handle.id())
            .params(GroupPatch {
                groupname: input.rename,
                description: input.description,
            })
            .send()?;

        Ok(GroupRecord::from(updated))
    }

    pub fn list_groups(&self, query: &ListQuery) -> Result<PagedResult<GroupRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, GROUP_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, GROUP_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let mut query_op = self.client.groups().query();
        for filter in filters {
            query_op = query_op.filter(&filter.key, filter.operator, &filter.value);
        }

        let page = apply_query_paging(query_op, query, &validated_sorts).page()?;
        Ok(PagedResult::from_page(page, GroupRecord::from))
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

pub(crate) const GROUP_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("groupname", "groupname"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];
use hubuum_client::GroupPatch;
