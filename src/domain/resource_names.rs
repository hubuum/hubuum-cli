use crate::errors::AppError;

/// A literal resource name. Preserve whitespace and numeric names after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceName(String);

impl ResourceName {
    pub fn new(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(AppError::InvalidOption(
                "Resource name must not be blank".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ObjectReference {
    class: ResourceName,
    object: ResourceName,
}

impl ObjectReference {
    pub fn new(class: ResourceName, object: ResourceName) -> Self {
        Self { class, object }
    }
    pub fn class(&self) -> &str {
        self.class.as_str()
    }
    pub fn object(&self) -> &str {
        self.object.as_str()
    }
}
