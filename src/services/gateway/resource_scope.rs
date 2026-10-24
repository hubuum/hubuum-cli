use super::HubuumGateway;
use crate::diagnostics::client_call;
use crate::errors::AppError;
use hubuum_client::{ClassId, ObjectId};

#[derive(Clone, PartialEq, Eq, Hash)]
enum ResourceSelector<I> {
    Name(String),
    Id(I),
}

#[derive(Clone, PartialEq, Eq, Hash)]
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

    pub fn require_object(self) -> Result<ObjectScope, AppError> {
        let object = self
            .object
            .ok_or_else(|| AppError::MissingOptions(vec!["name or object-id".into()]))?;
        Ok(ObjectScope {
            class: self.class,
            object,
        })
    }
}

#[derive(Clone)]
pub struct ObjectScope {
    class: ResourceSelector<ClassId>,
    object: ResourceSelector<ObjectId>,
}

impl From<ObjectScope> for ResourceScope {
    fn from(scope: ObjectScope) -> Self {
        Self {
            class: scope.class,
            object: Some(scope.object),
        }
    }
}

impl HubuumGateway {
    fn resolve_class_selector(
        &self,
        selector: ResourceSelector<ClassId>,
    ) -> Result<ClassId, AppError> {
        Ok(match selector {
            ResourceSelector::Id(id) => id,
            ResourceSelector::Name(name) => self.class_handle_by_name(&name)?.id(),
        })
    }

    fn resolve_object_selector(
        &self,
        class: ClassId,
        selector: ResourceSelector<ObjectId>,
    ) -> Result<ObjectId, AppError> {
        Ok(match selector {
            ResourceSelector::Id(id) => id,
            ResourceSelector::Name(name) => client_call("objects.get_by_name", || {
                self.client().objects(class).get_by_name(&name)
            })?
            .id(),
        })
    }

    pub(super) fn resolve_object_scope(
        &self,
        scope: ObjectScope,
    ) -> Result<(ClassId, ObjectId), AppError> {
        let class = self.resolve_class_selector(scope.class)?;
        Ok((class, self.resolve_object_selector(class, scope.object)?))
    }

    pub(super) fn resolve_resource_scope(
        &self,
        scope: ResourceScope,
    ) -> Result<(ClassId, Option<ObjectId>), AppError> {
        let class = self.resolve_class_selector(scope.class)?;
        let object = scope
            .object
            .map(|object| self.resolve_object_selector(class, object))
            .transpose()?;
        Ok((class, object))
    }
}

#[cfg(test)]
mod tests {
    use super::ResourceScope;

    #[test]
    fn object_scope_requires_one_valid_class_and_object_selector() {
        assert!(ResourceScope::class(None, None).is_err());
        assert!(ResourceScope::class(Some(" ".into()), None).is_err());
        assert!(ResourceScope::class(Some("Hosts".into()), Some(1.try_into().unwrap())).is_err());
        assert!("0".parse::<super::ClassId>().is_err());
        let class = || ResourceScope::class(None, Some(1.try_into().unwrap())).unwrap();
        assert!(class().require_object().is_err());
        assert!("-1".parse::<super::ObjectId>().is_err());
        assert!(class().with_object(Some(" ".into()), None).is_err());
        assert!(class()
            .with_object(Some("host".into()), Some(2.try_into().unwrap()))
            .is_err());
        assert!(class()
            .with_object(None, Some(2.try_into().unwrap()))
            .unwrap()
            .require_object()
            .is_ok());
        assert!(class()
            .with_object(Some("host".into()), None)
            .unwrap()
            .require_object()
            .is_ok());
    }
}
