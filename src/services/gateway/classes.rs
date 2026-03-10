use hubuum_client::{ClassPatch, ClassPost, FilterOperator};

use crate::domain::{ClassDetails, ClassRecord, ObjectRecord};
use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Clone, Default)]
pub struct ClassFilter {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateClassInput {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub json_schema: Option<serde_json::Value>,
    pub validate_schema: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ClassUpdateInput {
    pub name: String,
    pub rename: Option<String>,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub json_schema: Option<serde_json::Value>,
    pub validate_schema: Option<bool>,
}

impl HubuumGateway {
    pub fn list_class_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .classes()
            .find()
            .execute()?
            .into_iter()
            .map(|class| class.name)
            .collect())
    }

    pub fn create_class(&self, input: CreateClassInput) -> Result<ClassRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let class = self.client.classes().create_raw(ClassPost {
            name: input.name,
            namespace_id: namespace.id(),
            description: input.description,
            json_schema: input.json_schema,
            validate_schema: input.validate_schema,
        })?;
        Ok(ClassRecord::from(class))
    }

    pub fn class_details(&self, name: &str) -> Result<ClassDetails, AppError> {
        let class = self.client.classes().select_by_name(name)?;
        let objects = class
            .objects()?
            .into_iter()
            .map(|object| ObjectRecord::from(object.resource()))
            .collect();

        Ok(ClassDetails {
            class: ClassRecord::from(class.resource()),
            objects,
        })
    }

    pub fn delete_class(&self, name: &str) -> Result<(), AppError> {
        self.client.classes().select_by_name(name)?.delete()?;
        Ok(())
    }

    pub fn update_class(&self, input: ClassUpdateInput) -> Result<ClassRecord, AppError> {
        let class = self.client.classes().select_by_name(&input.name)?;

        let namespace_id = match input.namespace {
            Some(namespace) => self.client.namespaces().select_by_name(&namespace)?.id(),
            None => class.resource().namespace.id,
        };

        let updated = self.client.classes().update_raw(
            class.id(),
            ClassPatch {
                name: input.rename,
                namespace_id,
                description: input.description,
                json_schema: input.json_schema,
                validate_schema: input.validate_schema,
            },
        )?;

        Ok(ClassRecord::from(updated))
    }

    pub fn list_classes(&self, filter: ClassFilter) -> Result<Vec<ClassRecord>, AppError> {
        let mut search = self.client.classes().find();
        if let Some(name) = filter.name {
            search = search.add_filter(
                "name",
                FilterOperator::IContains { is_negated: false },
                name,
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
            .map(ClassRecord::from)
            .collect())
    }
}
