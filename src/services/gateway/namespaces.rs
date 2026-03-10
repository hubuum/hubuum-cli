use hubuum_client::{FilterOperator, NamespacePost};

use crate::domain::{
    GroupPermissionsRecord, GroupPermissionsSummary, NamespacePermission, NamespacePermissionsView,
    NamespaceRecord,
};
use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateNamespaceInput {
    pub name: String,
    pub description: String,
    pub owner: String,
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
        let namespace = self.client.namespaces().create(NamespacePost {
            name: input.name,
            description: input.description,
            group_id: group.id(),
        })?;
        Ok(NamespaceRecord::from(namespace))
    }

    pub fn list_namespaces(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<Vec<NamespaceRecord>, AppError> {
        let mut search = self.client.namespaces().find();
        if let Some(name) = name {
            search =
                search.add_filter("name", FilterOperator::Contains { is_negated: false }, name);
        }
        if let Some(description) = description {
            search = search.add_filter(
                "description",
                FilterOperator::Contains { is_negated: false },
                description,
            );
        }

        Ok(search
            .execute()?
            .into_iter()
            .map(NamespaceRecord::from)
            .collect())
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
