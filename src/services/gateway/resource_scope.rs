use super::HubuumGateway;
use crate::diagnostics::client_call;
use crate::errors::AppError;
use hubuum_client::{ClassId, ObjectId};

#[derive(Clone)]
enum ResourceSelector<I> {
    Name(String),
    Id(I),
}

#[derive(Clone)]
pub struct ResourceScope {
    class: ResourceSelector<ClassId>,
    object: Option<ResourceSelector<ObjectId>>,
}

impl ResourceScope {
    pub fn class(name: Option<String>, id: Option<ClassId>) -> Result<Self, AppError> {
        let class = match (name, id) {
            (Some(name), None) if !name.trim().is_empty() => ResourceSelector::Name(name),
            (None, Some(id)) if id.get() > 0 => ResourceSelector::Id(id),
            _ => {
                return Err(AppError::InvalidOption(
                    "Select exactly one nonblank --class or positive --class-id".into(),
                ))
            }
        };
        Ok(Self {
            class,
            object: None,
        })
    }

    pub fn with_object(
        mut self,
        name: Option<String>,
        id: Option<ObjectId>,
    ) -> Result<Self, AppError> {
        self.object = match (name, id) {
            (Some(name), None) if !name.trim().is_empty() => Some(ResourceSelector::Name(name)),
            (None, Some(id)) if id.get() > 0 => Some(ResourceSelector::Id(id)),
            (None, None) => None,
            _ => {
                return Err(AppError::InvalidOption(
                    "Select exactly one nonblank --name or positive --object-id".into(),
                ))
            }
        };
        Ok(self)
    }

    pub fn require_object(self) -> Result<Self, AppError> {
        if self.object.is_none() {
            return Err(AppError::MissingOptions(vec!["name or object-id".into()]));
        }
        Ok(self)
    }
}

impl HubuumGateway {
    pub(super) fn resolve_resource_scope(
        &self,
        scope: ResourceScope,
    ) -> Result<(ClassId, Option<ObjectId>), AppError> {
        let class = match scope.class {
            ResourceSelector::Id(id) => id,
            ResourceSelector::Name(name) => self.class_handle_by_name(&name)?.id(),
        };
        let object = match scope.object {
            Some(ResourceSelector::Id(id)) => Some(id),
            Some(ResourceSelector::Name(name)) => Some(
                client_call("objects.get_by_name", || {
                    self.client().objects(class).get_by_name(&name)
                })?
                .id(),
            ),
            None => None,
        };
        Ok((class, object))
    }
}
