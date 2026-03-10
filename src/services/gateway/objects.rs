use std::collections::HashMap;

use hubuum_client::{FilterOperator, ObjectPatch, ObjectPost};

use crate::domain::ResolvedObjectRecord;
use crate::errors::AppError;

use super::{shared::find_entities_by_ids, HubuumGateway};

#[derive(Debug, Clone)]
pub struct CreateObjectInput {
    pub name: String,
    pub class_name: String,
    pub namespace: String,
    pub description: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ObjectFilter {
    pub class_name: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectUpdateInput {
    pub name: String,
    pub class_name: String,
    pub rename: Option<String>,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub data: Option<serde_json::Value>,
}

impl HubuumGateway {
    pub fn list_object_names_for_class(&self, class_name: &str) -> Result<Vec<String>, AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        Ok(self
            .client
            .objects(class.id())
            .find()
            .execute()?
            .into_iter()
            .map(|object| object.name)
            .collect())
    }

    pub fn create_object(
        &self,
        input: CreateObjectInput,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let class = self.client.classes().select_by_name(&input.class_name)?;

        let object = self.client.objects(class.id()).create(ObjectPost {
            name: input.name,
            hubuum_class_id: class.id(),
            namespace_id: namespace.id(),
            description: input.description,
            data: input.data,
        })?;

        let classmap = HashMap::from([(class.id(), class.resource().clone())]);
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ResolvedObjectRecord::new(&object, &classmap, &namespacemap))
    }

    pub fn object_details(
        &self,
        class_name: &str,
        object_name: &str,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        let namespace = self
            .client
            .namespaces()
            .select(object.resource().namespace_id)?;

        let classmap = HashMap::from([(class.id(), class.resource().clone())]);
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ResolvedObjectRecord::new(
            object.resource(),
            &classmap,
            &namespacemap,
        ))
    }

    pub fn delete_object(&self, class_name: &str, object_name: &str) -> Result<(), AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        self.client.objects(class.id()).delete(object.id())?;
        Ok(())
    }

    pub fn list_objects(
        &self,
        filter: ObjectFilter,
    ) -> Result<Vec<ResolvedObjectRecord>, AppError> {
        let class = self.client.classes().select_by_name(&filter.class_name)?;
        let mut search = self.client.objects(class.id()).find();

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

        let objects = search.execute()?;
        if objects.is_empty() {
            return Ok(Vec::new());
        }

        let classmap = find_entities_by_ids(&self.client.classes(), &objects, |object| {
            object.hubuum_class_id
        })?;
        let namespacemap = find_entities_by_ids(&self.client.namespaces(), &objects, |object| {
            object.namespace_id
        })?;

        Ok(objects
            .iter()
            .map(|object| ResolvedObjectRecord::new(object, &classmap, &namespacemap))
            .collect())
    }

    pub fn update_object(
        &self,
        input: ObjectUpdateInput,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().select_by_name(&input.class_name)?;
        let object = class.object_by_name(&input.name)?;

        let mut patch = ObjectPatch {
            data: input.data,
            ..ObjectPatch::default()
        };

        if let Some(namespace) = input.namespace {
            let namespace = self.client.namespaces().select_by_name(&namespace)?;
            patch.namespace_id = Some(namespace.id());
        }
        if let Some(rename) = input.rename {
            patch.name = Some(rename);
        }
        if let Some(description) = input.description {
            patch.description = Some(description);
        }

        let result = self.client.objects(class.id()).update(object.id(), patch)?;
        let namespace = self.client.namespaces().select(result.namespace_id)?;

        let classmap = HashMap::from([(class.id(), class.resource().clone())]);
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ResolvedObjectRecord::new(&result, &classmap, &namespacemap))
    }
}
