use hubuum_client::{
    ClassComputationState, ComputedFieldDefinition, ComputedFieldDeleteResponse,
    ComputedFieldError, ComputedFieldMutationResponse, ComputedFieldOperation,
    ComputedFieldPreviewResponse, ComputedFieldVisibility, ComputedResultType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ComputedFieldSelectorKind {
    All,
    None,
    Shared(String),
    Personal(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComputedFieldSelector {
    kind: ComputedFieldSelectorKind,
}

impl ComputedFieldSelector {
    pub fn is_all(&self) -> bool {
        matches!(self.kind, ComputedFieldSelectorKind::All)
    }

    pub fn is_none(&self) -> bool {
        matches!(self.kind, ComputedFieldSelectorKind::None)
    }

    pub fn scoped_parts(&self) -> Option<(&'static str, &str)> {
        match &self.kind {
            ComputedFieldSelectorKind::Shared(key) => Some(("S", key)),
            ComputedFieldSelectorKind::Personal(key) => Some(("P", key)),
            ComputedFieldSelectorKind::All | ComputedFieldSelectorKind::None => None,
        }
    }
}

impl Display for ComputedFieldSelector {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ComputedFieldSelectorKind::All => formatter.write_str("all"),
            ComputedFieldSelectorKind::None => formatter.write_str("none"),
            ComputedFieldSelectorKind::Shared(key) => write!(formatter, "S:{key}"),
            ComputedFieldSelectorKind::Personal(key) => write!(formatter, "P:{key}"),
        }
    }
}

impl FromStr for ComputedFieldSelector {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "all" => Ok(Self {
                kind: ComputedFieldSelectorKind::All,
            }),
            "none" => Ok(Self {
                kind: ComputedFieldSelectorKind::None,
            }),
            _ => {
                let Some((scope, key)) = value.split_once(':') else {
                    return Err(format!(
                        "Invalid computed field '{value}'; expected S:key, P:key, all, or none"
                    ));
                };
                if key.is_empty() {
                    return Err(format!(
                        "Invalid computed field '{value}'; the key cannot be empty"
                    ));
                }
                let kind = match scope {
                    "S" => ComputedFieldSelectorKind::Shared(key.to_string()),
                    "P" => ComputedFieldSelectorKind::Personal(key.to_string()),
                    _ => {
                        return Err(format!(
                            "Invalid computed field '{value}'; scope must be S or P"
                        ));
                    }
                };
                Ok(Self { kind })
            }
        }
    }
}

impl Serialize for ComputedFieldSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ComputedFieldSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComputedFieldSet {
    selectors: Vec<ComputedFieldSelector>,
}

impl ComputedFieldSet {
    pub fn from_values(values: &[String]) -> Result<Self, String> {
        let selectors = values
            .iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ComputedFieldSelector::from_str)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(selectors)
    }

    pub fn selectors(&self) -> &[ComputedFieldSelector] {
        &self.selectors
    }

    pub fn is_empty(&self) -> bool {
        self.selectors.is_empty()
    }

    pub fn is_all(&self) -> bool {
        self.selectors
            .first()
            .is_some_and(ComputedFieldSelector::is_all)
    }

    fn new(selectors: Vec<ComputedFieldSelector>) -> Result<Self, String> {
        let has_all = selectors.iter().any(ComputedFieldSelector::is_all);
        let has_none = selectors.iter().any(ComputedFieldSelector::is_none);
        if (has_all || has_none) && selectors.len() != 1 {
            let exclusive = if has_all { "all" } else { "none" };
            return Err(format!(
                "Computed field '{exclusive}' cannot be combined with other selections"
            ));
        }
        if has_none {
            return Ok(Self::default());
        }

        let mut seen = BTreeSet::new();
        let selectors = selectors
            .into_iter()
            .filter(|selector| seen.insert(selector.clone()))
            .collect();
        Ok(Self { selectors })
    }
}

impl Serialize for ComputedFieldSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.selectors.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ComputedFieldSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let selectors = Vec::<ComputedFieldSelector>::deserialize(deserializer)?;
        Self::new(selectors).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedFieldRecord {
    pub id: i32,
    pub class_id: i32,
    pub visibility: String,
    pub owner_user_id: Option<i32>,
    pub key: String,
    pub label: String,
    pub description: String,
    pub operation: String,
    pub paths: Vec<String>,
    pub result_type: String,
    pub enabled: bool,
    pub revision: i64,
    pub semantics_version: i16,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ComputedFieldDefinition> for ComputedFieldRecord {
    fn from(value: ComputedFieldDefinition) -> Self {
        Self {
            id: value.id.into(),
            class_id: value.class_id.into(),
            visibility: visibility_name(value.visibility).to_string(),
            owner_user_id: value.owner_user_id.map(Into::into),
            key: value.key,
            label: value.label,
            description: value.description,
            operation: operation_name(&value.operation).to_string(),
            paths: value.operation.paths().to_vec(),
            result_type: result_type_name(value.result_type).to_string(),
            enabled: value.enabled,
            revision: value.revision,
            semantics_version: value.semantics_version,
            created_by: value.created_by.map(Into::into),
            updated_by: value.updated_by.map(Into::into),
            created_at: value.created_at.to_string(),
            updated_at: value.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassComputationStateRecord {
    pub class_id: i32,
    pub evaluation_revision: i64,
    pub rebuild_status: String,
    pub active_task_id: Option<i32>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ClassComputationState> for ClassComputationStateRecord {
    fn from(value: ClassComputationState) -> Self {
        Self {
            class_id: value.class_id.into(),
            evaluation_revision: value.evaluation_revision,
            rebuild_status: value.rebuild_status,
            active_task_id: value.active_task_id.map(Into::into),
            last_error: value.last_error,
            created_at: value.created_at.to_string(),
            updated_at: value.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedComputedFieldListRecord {
    pub definitions: Vec<ComputedFieldRecord>,
    pub state: ClassComputationStateRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedFieldMutationRecord {
    pub definition: ComputedFieldRecord,
    pub state: ClassComputationStateRecord,
}

impl From<ComputedFieldMutationResponse> for ComputedFieldMutationRecord {
    fn from(value: ComputedFieldMutationResponse) -> Self {
        Self {
            definition: value.definition.into(),
            state: value.state.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedFieldDeleteRecord {
    pub deleted_definition_id: i32,
    pub state: ClassComputationStateRecord,
}

impl From<ComputedFieldDeleteResponse> for ComputedFieldDeleteRecord {
    fn from(value: ComputedFieldDeleteResponse) -> Self {
        Self {
            deleted_definition_id: value.deleted_definition_id.into(),
            state: value.state.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedFieldErrorRecord {
    pub code: String,
    pub path: Option<String>,
    pub message: String,
}

impl From<ComputedFieldError> for ComputedFieldErrorRecord {
    fn from(value: ComputedFieldError) -> Self {
        Self {
            code: value.code,
            path: value.path,
            message: value.message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedFieldPreviewRecord {
    pub value: Value,
    pub error: Option<ComputedFieldErrorRecord>,
}

impl From<ComputedFieldPreviewResponse> for ComputedFieldPreviewRecord {
    fn from(value: ComputedFieldPreviewResponse) -> Self {
        Self {
            value: value.value,
            error: value.error.map(Into::into),
        }
    }
}

fn visibility_name(value: ComputedFieldVisibility) -> &'static str {
    match value {
        ComputedFieldVisibility::Shared => "shared",
        ComputedFieldVisibility::Personal => "personal",
        ComputedFieldVisibility::Unknown => "unknown",
        _ => "unknown",
    }
}

fn result_type_name(value: ComputedResultType) -> &'static str {
    match value {
        ComputedResultType::String => "string",
        ComputedResultType::Number => "number",
        ComputedResultType::Integer => "integer",
        ComputedResultType::Boolean => "boolean",
        ComputedResultType::Object => "object",
        ComputedResultType::Array => "array",
        _ => "unknown",
    }
}

fn operation_name(value: &ComputedFieldOperation) -> &'static str {
    match value {
        ComputedFieldOperation::FirstNonNull { .. } => "first_non_null",
        ComputedFieldOperation::Sum { .. } => "sum",
        ComputedFieldOperation::Average { .. } => "average",
        ComputedFieldOperation::Min { .. } => "min",
        ComputedFieldOperation::Max { .. } => "max",
        ComputedFieldOperation::AllPresent { .. } => "all_present",
        ComputedFieldOperation::AnyPresent { .. } => "any_present",
        ComputedFieldOperation::CountPresent { .. } => "count_present",
        ComputedFieldOperation::AllPresentAndEqual { .. } => "all_present_and_equal",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ComputedFieldSet;

    #[test]
    fn computed_field_sets_validate_scopes_and_exclusive_values() {
        let fields =
            ComputedFieldSet::from_values(&["S:load,P:note".to_string(), "S:load".to_string()])
                .expect("scoped fields should parse");

        assert_eq!(
            fields
                .selectors()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["S:load", "P:note"]
        );
        assert!(ComputedFieldSet::from_values(&["all,S:load".to_string()]).is_err());
        assert!(ComputedFieldSet::from_values(&["none,P:note".to_string()]).is_err());
        assert!(ComputedFieldSet::from_values(&["load".to_string()]).is_err());
    }

    #[test]
    fn computed_field_sets_serialize_as_string_arrays() {
        let fields = ComputedFieldSet::from_values(&["S:load,P:note".to_string()])
            .expect("fields should parse");

        assert_eq!(
            serde_json::to_value(&fields).expect("serialize"),
            json!(["S:load", "P:note"])
        );
        assert!(serde_json::from_value::<ComputedFieldSet>(json!(["all"]))
            .expect("deserialize all")
            .is_all());
    }
}
