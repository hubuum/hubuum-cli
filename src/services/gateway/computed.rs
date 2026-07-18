use std::str::FromStr;

use hubuum_client::{
    blocking::Handle, Class, ComputedFieldDefinitionPatch, ComputedFieldDefinitionRequest,
    ComputedFieldOperation, ComputedFieldPreviewRequest, ComputedResultType,
    PersonalComputedFieldDefinitionRequest,
};
use serde_json::Value;

use crate::domain::{
    ClassComputationStateRecord, ComputedFieldDeleteRecord, ComputedFieldMutationRecord,
    ComputedFieldPreviewRecord, ComputedFieldRecord, SharedComputedFieldListRecord,
};
use crate::errors::AppError;
use crate::list_query::{apply_cursor_request_paging, ListQuery, PagedResult};

use super::HubuumGateway;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputedOperationKind {
    FirstNonNull,
    Sum,
    Average,
    Min,
    Max,
    AllPresent,
    AnyPresent,
    CountPresent,
    AllPresentAndEqual,
}

impl FromStr for ComputedOperationKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().replace('-', "_").as_str() {
            "first_non_null" => Ok(Self::FirstNonNull),
            "sum" => Ok(Self::Sum),
            "average" => Ok(Self::Average),
            "min" => Ok(Self::Min),
            "max" => Ok(Self::Max),
            "all_present" => Ok(Self::AllPresent),
            "any_present" => Ok(Self::AnyPresent),
            "count_present" => Ok(Self::CountPresent),
            "all_present_and_equal" => Ok(Self::AllPresentAndEqual),
            _ => Err(AppError::InvalidOption(format!(
                "Invalid computed operation '{value}'. Valid values: first_non_null, sum, average, min, max, all_present, any_present, count_present, all_present_and_equal"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComputedOperationInput {
    kind: ComputedOperationKind,
    paths: Vec<String>,
}

impl ComputedOperationInput {
    pub fn new(kind: ComputedOperationKind, paths: Vec<String>) -> Result<Self, AppError> {
        if paths.is_empty() {
            return Err(AppError::MissingOptions(vec!["path".to_string()]));
        }
        if let Some(path) = paths
            .iter()
            .find(|path| !path.is_empty() && !path.starts_with('/'))
        {
            return Err(AppError::InvalidOption(format!(
                "Computed path '{path}' is not a JSON Pointer; paths must be empty or start with '/'"
            )));
        }
        Ok(Self { kind, paths })
    }

    fn into_api(self) -> ComputedFieldOperation {
        let paths = self.paths;
        match self.kind {
            ComputedOperationKind::FirstNonNull => ComputedFieldOperation::FirstNonNull { paths },
            ComputedOperationKind::Sum => ComputedFieldOperation::Sum { paths },
            ComputedOperationKind::Average => ComputedFieldOperation::Average { paths },
            ComputedOperationKind::Min => ComputedFieldOperation::Min { paths },
            ComputedOperationKind::Max => ComputedFieldOperation::Max { paths },
            ComputedOperationKind::AllPresent => ComputedFieldOperation::AllPresent { paths },
            ComputedOperationKind::AnyPresent => ComputedFieldOperation::AnyPresent { paths },
            ComputedOperationKind::CountPresent => ComputedFieldOperation::CountPresent { paths },
            ComputedOperationKind::AllPresentAndEqual => {
                ComputedFieldOperation::AllPresentAndEqual { paths }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputedResultKind {
    String,
    Number,
    Integer,
    Boolean,
    Object,
    Array,
}

impl FromStr for ComputedResultKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "string" => Ok(Self::String),
            "number" => Ok(Self::Number),
            "integer" => Ok(Self::Integer),
            "boolean" => Ok(Self::Boolean),
            "object" => Ok(Self::Object),
            "array" => Ok(Self::Array),
            _ => Err(AppError::InvalidOption(format!(
                "Invalid computed result type '{value}'. Valid values: string, number, integer, boolean, object, array"
            ))),
        }
    }
}

impl From<ComputedResultKind> for ComputedResultType {
    fn from(value: ComputedResultKind) -> Self {
        match value {
            ComputedResultKind::String => Self::String,
            ComputedResultKind::Number => Self::Number,
            ComputedResultKind::Integer => Self::Integer,
            ComputedResultKind::Boolean => Self::Boolean,
            ComputedResultKind::Object => Self::Object,
            ComputedResultKind::Array => Self::Array,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComputedDefinitionInput {
    key: String,
    label: String,
    description: String,
    operation: ComputedOperationInput,
    result_type: ComputedResultKind,
    enabled: bool,
}

impl ComputedDefinitionInput {
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        operation: ComputedOperationInput,
        result_type: ComputedResultKind,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: String::new(),
            operation,
            result_type,
            enabled: true,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn into_api(self) -> ComputedFieldDefinitionRequest {
        ComputedFieldDefinitionRequest::new(
            self.key,
            self.label,
            self.operation.into_api(),
            self.result_type.into(),
        )
        .description(self.description)
        .enabled(self.enabled)
    }
}

#[derive(Debug, Clone)]
pub struct ComputedPatchInput {
    expected_revision: i64,
    key: Option<String>,
    label: Option<String>,
    description: Option<String>,
    operation: Option<ComputedOperationInput>,
    result_type: Option<ComputedResultKind>,
    enabled: Option<bool>,
}

impl ComputedPatchInput {
    pub fn new(expected_revision: i64) -> Self {
        Self {
            expected_revision,
            key: None,
            label: None,
            description: None,
            operation: None,
            result_type: None,
            enabled: None,
        }
    }

    pub fn key(mut self, key: Option<String>) -> Self {
        self.key = key;
        self
    }

    pub fn label(mut self, label: Option<String>) -> Self {
        self.label = label;
        self
    }

    pub fn description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn operation(mut self, operation: Option<ComputedOperationInput>) -> Self {
        self.operation = operation;
        self
    }

    pub fn result_type(mut self, result_type: Option<ComputedResultKind>) -> Self {
        self.result_type = result_type;
        self
    }

    pub fn enabled(mut self, enabled: Option<bool>) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.key.is_none()
            && self.label.is_none()
            && self.description.is_none()
            && self.operation.is_none()
            && self.result_type.is_none()
            && self.enabled.is_none()
    }

    fn into_api(self) -> ComputedFieldDefinitionPatch {
        let mut patch = ComputedFieldDefinitionPatch::new(self.expected_revision);
        if let Some(key) = self.key {
            patch = patch.key(key);
        }
        if let Some(label) = self.label {
            patch = patch.label(label);
        }
        if let Some(description) = self.description {
            patch = patch.description(description);
        }
        if let Some(operation) = self.operation {
            patch = patch.operation(operation.into_api());
        }
        if let Some(result_type) = self.result_type {
            patch = patch.result_type(result_type.into());
        }
        if let Some(enabled) = self.enabled {
            patch = patch.enabled(enabled);
        }
        patch
    }
}

#[derive(Debug, Clone)]
pub enum ComputedPreviewTarget {
    Object(String),
    Data(Value),
}

impl HubuumGateway {
    pub fn list_shared_computed_fields(
        &self,
        class_name: &str,
    ) -> Result<SharedComputedFieldListRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let response = self.client.computed_fields(class.id()).list()?;
        Ok(SharedComputedFieldListRecord {
            definitions: response
                .definitions
                .into_iter()
                .map(ComputedFieldRecord::from)
                .collect(),
            state: response.state.into(),
        })
    }

    pub fn create_shared_computed_field(
        &self,
        class_name: &str,
        input: ComputedDefinitionInput,
    ) -> Result<ComputedFieldMutationRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        Ok(self
            .client
            .computed_fields(class.id())
            .create(input.into_api())?
            .into())
    }

    pub fn update_shared_computed_field(
        &self,
        class_name: &str,
        field_key: &str,
        input: ComputedPatchInput,
    ) -> Result<ComputedFieldMutationRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let fields = self.client.computed_fields(class.id());
        let definition = fields
            .list()?
            .definitions
            .into_iter()
            .find(|definition| definition.key == field_key)
            .ok_or_else(|| computed_field_not_found("shared", class_name, field_key))?;
        Ok(fields.update(definition.id, input.into_api())?.into())
    }

    pub fn delete_shared_computed_field(
        &self,
        class_name: &str,
        field_key: &str,
        expected_revision: i64,
    ) -> Result<ComputedFieldDeleteRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let fields = self.client.computed_fields(class.id());
        let definition = fields
            .list()?
            .definitions
            .into_iter()
            .find(|definition| definition.key == field_key)
            .ok_or_else(|| computed_field_not_found("shared", class_name, field_key))?;
        Ok(fields.delete(definition.id, expected_revision)?.into())
    }

    pub fn preview_shared_computed_field(
        &self,
        class_name: &str,
        definition: ComputedDefinitionInput,
        target: ComputedPreviewTarget,
    ) -> Result<ComputedFieldPreviewRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let request = self.computed_preview_request(&class, definition, target, false)?;
        Ok(self
            .client
            .computed_fields(class.id())
            .preview(request)?
            .into())
    }

    pub fn rebuild_shared_computed_fields(
        &self,
        class_name: &str,
    ) -> Result<ClassComputationStateRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        Ok(self.client.computed_fields(class.id()).rebuild()?.into())
    }

    pub fn list_personal_computed_fields(
        &self,
        class_name: Option<&str>,
        query: &ListQuery,
    ) -> Result<PagedResult<ComputedFieldRecord>, AppError> {
        let request = match class_name {
            Some(class_name) => {
                let class = self.client.classes().get_by_name(class_name)?;
                self.client.personal_computed_fields().for_class(class.id())
            }
            None => self.client.personal_computed_fields().query(),
        };
        let page = apply_cursor_request_paging(request, query, &[]).page()?;
        Ok(PagedResult::from_page(page, Into::into))
    }

    pub fn create_personal_computed_field(
        &self,
        class_name: &str,
        input: ComputedDefinitionInput,
    ) -> Result<ComputedFieldRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        Ok(self
            .client
            .personal_computed_fields()
            .create(PersonalComputedFieldDefinitionRequest::new(
                class.id(),
                input.into_api(),
            ))?
            .into())
    }

    pub fn update_personal_computed_field(
        &self,
        class_name: &str,
        field_key: &str,
        input: ComputedPatchInput,
    ) -> Result<ComputedFieldRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let fields = self.client.personal_computed_fields();
        let definition = fields
            .for_class(class.id())
            .all()?
            .into_iter()
            .find(|definition| definition.key == field_key)
            .ok_or_else(|| computed_field_not_found("personal", class_name, field_key))?;
        Ok(fields.update(definition.id, input.into_api())?.into())
    }

    pub fn delete_personal_computed_field(
        &self,
        class_name: &str,
        field_key: &str,
        expected_revision: i64,
    ) -> Result<ComputedFieldRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let fields = self.client.personal_computed_fields();
        let definition = fields
            .for_class(class.id())
            .all()?
            .into_iter()
            .find(|definition| definition.key == field_key)
            .ok_or_else(|| computed_field_not_found("personal", class_name, field_key))?;
        fields.delete(definition.id, expected_revision)?;
        Ok(definition.into())
    }

    pub fn preview_personal_computed_field(
        &self,
        class_name: &str,
        definition: ComputedDefinitionInput,
        target: ComputedPreviewTarget,
    ) -> Result<ComputedFieldPreviewRecord, AppError> {
        let class = self.client.classes().get_by_name(class_name)?;
        let request = self.computed_preview_request(&class, definition, target, true)?;
        Ok(self
            .client
            .personal_computed_fields()
            .preview(request)?
            .into())
    }

    fn computed_preview_request(
        &self,
        class: &Handle<Class>,
        definition: ComputedDefinitionInput,
        target: ComputedPreviewTarget,
        personal: bool,
    ) -> Result<ComputedFieldPreviewRequest, AppError> {
        let definition = definition.into_api();
        let request = match target {
            ComputedPreviewTarget::Object(object_name) => {
                let object = class.object_by_name(&object_name)?;
                ComputedFieldPreviewRequest::for_object(definition, object.id())
            }
            ComputedPreviewTarget::Data(data) => {
                ComputedFieldPreviewRequest::for_data(definition, data)
            }
        };
        Ok(if personal {
            request.for_class(class.id())
        } else {
            request
        })
    }
}

fn computed_field_not_found(visibility: &str, class_name: &str, field_key: &str) -> AppError {
    AppError::EntityNotFound(format!(
        "{visibility} computed field '{field_key}' in class '{class_name}'"
    ))
}

#[cfg(test)]
mod tests {
    use super::{ComputedOperationInput, ComputedOperationKind, ComputedResultKind};
    use std::str::FromStr;

    #[test]
    fn computed_operations_accept_snake_and_kebab_case() {
        assert_eq!(
            ComputedOperationKind::from_str("first_non_null").expect("operation"),
            ComputedOperationKind::FirstNonNull
        );
        assert_eq!(
            ComputedOperationKind::from_str("all-present-and-equal").expect("operation"),
            ComputedOperationKind::AllPresentAndEqual
        );
    }

    #[test]
    fn computed_result_types_are_validated() {
        assert_eq!(
            ComputedResultKind::from_str("boolean").expect("result type"),
            ComputedResultKind::Boolean
        );
        assert!(ComputedResultKind::from_str("float").is_err());
    }

    #[test]
    fn computed_paths_must_be_json_pointers() {
        assert!(ComputedOperationInput::new(
            ComputedOperationKind::Sum,
            vec!["/load/one".to_string()]
        )
        .is_ok());
        assert!(ComputedOperationInput::new(
            ComputedOperationKind::Sum,
            vec!["load.one".to_string()]
        )
        .is_err());
    }
}
