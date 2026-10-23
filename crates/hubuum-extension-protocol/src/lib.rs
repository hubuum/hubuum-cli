mod manifest;
pub use manifest::{CommandDeclaration, CommandImplementation, ExtensionManifest};
mod workflow;
use workflow::validate_declared_default;
use workflow::validate_declared_value;
use workflow::validate_workflow_calls;
pub use workflow::{
    WorkflowAssertStep, WorkflowBinding, WorkflowBindingName, WorkflowCallStep, WorkflowCapability,
    WorkflowDeclaration, WorkflowForEachStep, WorkflowInputDeclaration, WorkflowLetStep,
    WorkflowOutputDeclaration, WorkflowRunStep, WorkflowStep, WorkflowStepId,
};
mod response;
pub use response::{ExtensionFailure, ExtensionResponse, SemanticOutput, SemanticOutputShape};

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use jsonc_parser::ParseOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_V1: &str = "hubuum-cli.extension/v1";
pub const MANIFEST_FILENAME: &str = "hubuum-extension.jsonc";

const MANIFEST_PARSE_OPTIONS: ParseOptions = ParseOptions {
    allow_comments: true,
    allow_loose_object_property_names: false,
    allow_trailing_commas: true,
    allow_missing_commas: false,
    allow_single_quoted_strings: false,
    allow_hexadecimal_numbers: false,
    allow_unary_plus_numbers: false,
    allow_bare_decimal_point_numbers: false,
    allow_extended_string_escapes: false,
    allow_non_finite_numbers: false,
};

const RESERVED_PACK_NAMES: &[&str] = &[
    "contract", "disable", "doctor", "enable", "explain", "init", "install", "list", "reload",
    "remove", "show", "upgrade", "validate",
];
const RESERVED_LONG_OPTIONS: &[&str] = &["help", "json", "output", "table-headers", "debug"];
const RESERVED_SHORT_OPTIONS: &[char] = &['h', 'j', 'o'];
const RESERVED_OPTION_KEYS: &[&str] = &[
    "h",
    "help",
    "j",
    "json",
    "o",
    "output",
    "table-headers",
    "debug",
];

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid JSONC manifest: {0}")]
    InvalidManifestJsonc(String),
    #[error("invalid JSON response: {0}")]
    InvalidResponseJson(#[from] serde_json::Error),
    #[error("unsupported manifest schema version {0}; expected 1")]
    UnsupportedSchemaVersion(u32),
    #[error("unsupported extension protocol '{0}'; expected '{PROTOCOL_V1}'")]
    UnsupportedProtocol(String),
    #[error("invalid pack name '{0}': use lowercase ASCII kebab-case")]
    InvalidPackName(String),
    #[error("pack name '{0}' is reserved by extension management commands")]
    ReservedPackName(String),
    #[error("invalid command path: {0}")]
    InvalidCommandPath(String),
    #[error("invalid command declaration name '{0}': use lowercase ASCII snake_case")]
    InvalidCommandDeclarationName(String),
    #[error("invalid executable path '{0}': use a relative path confined to the pack")]
    InvalidExecutablePath(String),
    #[error("invalid {kind} extension pack: {message}")]
    InvalidPackKind { kind: String, message: String },
    #[error("command '{0}' requires an executable or a workflow")]
    MissingCommandImplementation(String),
    #[error("command '{command}' has an invalid workflow: {message}")]
    InvalidWorkflow { command: String, message: String },
    #[error("workflow '{workflow}' is invalid: {message}")]
    InvalidNamedWorkflow { workflow: String, message: String },
    #[error("extension configuration key '{key}' is invalid: {message}")]
    InvalidConfig { key: String, message: String },
    #[error("invalid extension version '{value}': {source}")]
    InvalidVersion {
        value: String,
        source: semver::Error,
    },
    #[error("invalid CLI compatibility requirement '{value}': {source}")]
    InvalidVersionRequirement {
        value: String,
        source: semver::Error,
    },
    #[error("command '{command}' has invalid option '{option}': {message}")]
    InvalidOption {
        command: String,
        option: String,
        message: String,
    },
    #[error("duplicate command path '{0}'")]
    DuplicateCommandPath(String),
    #[error("command path '{prefix}' cannot prefix command path '{command}'")]
    CommandPathPrefix { prefix: String, command: String },
    #[error("response output is invalid: {0}")]
    InvalidOutput(String),
    #[error("invalid diagnostic code '{0}': use lowercase ASCII snake_case")]
    InvalidDiagnosticCode(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PackName(String);

impl PackName {
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        if !is_kebab_word(&value) {
            return Err(ProtocolError::InvalidPackName(value));
        }
        if RESERVED_PACK_NAMES.contains(&value.as_str()) {
            return Err(ProtocolError::ReservedPackName(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CommandPath(Vec<String>);

impl CommandPath {
    pub fn new(segments: Vec<String>) -> Result<Self, ProtocolError> {
        if segments.is_empty() {
            return Err(ProtocolError::InvalidCommandPath(
                "at least one segment is required".to_string(),
            ));
        }
        if let Some(segment) = segments.iter().find(|segment| !is_kebab_word(segment)) {
            return Err(ProtocolError::InvalidCommandPath(format!(
                "segment '{segment}' must use lowercase ASCII kebab-case"
            )));
        }
        Ok(Self(segments))
    }

    pub fn segments(&self) -> &[String] {
        &self.0
    }

    pub fn split_last(&self) -> (&str, &[String]) {
        let (name, parents) = self
            .0
            .split_last()
            .expect("validated command paths are non-empty");
        (name, parents)
    }

    pub fn display(&self) -> String {
        self.0.join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExecutablePath(PathBuf);

impl ExecutablePath {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, ProtocolError> {
        let value = value.into();
        let valid = !value.as_os_str().is_empty()
            && !value.is_absolute()
            && value
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
        if !valid {
            return Err(ProtocolError::InvalidExecutablePath(
                value.display().to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProtocolVersion(String);

impl ProtocolVersion {
    pub fn v1() -> Self {
        Self(PROTOCOL_V1.to_string())
    }

    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        if value != PROTOCOL_V1 {
            return Err(ProtocolError::UnsupportedProtocol(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DiagnosticCode(String);

impl DiagnosticCode {
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        if !is_snake_word(&value) {
            return Err(ProtocolError::InvalidDiagnosticCode(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionPackKind {
    Portable,
    Executable,
}

impl ExtensionPackKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portable => "portable",
            Self::Executable => "executable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct CommandDeclarationName(String);

impl CommandDeclarationName {
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        if is_snake_word(&value) {
            Ok(Self(value))
        } else {
            Err(ProtocolError::InvalidCommandDeclarationName(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct WorkflowName(String);

impl WorkflowName {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if is_snake_word(&value) {
            Ok(Self(value))
        } else {
            Err(format!("'{value}'; use lowercase ASCII snake_case"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowValueType {
    String,
    Integer,
    Number,
    Boolean,
    Json,
}

impl WorkflowValueType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Json => "json",
        }
    }

    pub fn accepts(self, value: &Value) -> bool {
        match self {
            Self::String => value.is_string(),
            Self::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
            Self::Number => value.is_number(),
            Self::Boolean => value.is_boolean(),
            Self::Json => true,
        }
    }

    pub fn accepts_type(self, source: Self) -> bool {
        self == Self::Json || self == source || (self == Self::Number && source == Self::Integer)
    }
}

#[derive(Debug, Clone)]
pub struct ConfigDeclaration {
    key: String,
    value_type: WorkflowValueType,
    required: bool,
    repeatable: bool,
    default: Option<Value>,
    help: String,
}

impl ConfigDeclaration {
    fn validate(key: String, raw: RawValueDeclaration) -> Result<Self, ProtocolError> {
        if !is_option_word(&key) {
            return Err(invalid_config(
                &key,
                "key must use lowercase ASCII letters, numbers, '-' or '_'",
            ));
        }
        if raw.required && raw.default.is_some() {
            return Err(invalid_config(
                &key,
                "required config cannot also declare a default",
            ));
        }
        validate_declared_default(&raw.default, raw.value_type, raw.repeatable, |message| {
            invalid_config(&key, &message)
        })?;
        Ok(Self {
            key,
            value_type: raw.value_type,
            required: raw.required,
            repeatable: raw.repeatable,
            default: raw.default,
            help: raw.help,
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value_type(&self) -> WorkflowValueType {
        self.value_type
    }

    pub fn required(&self) -> bool {
        self.required
    }

    pub fn repeatable(&self) -> bool {
        self.repeatable
    }

    pub fn default(&self) -> Option<&Value> {
        self.default.as_ref()
    }

    pub fn help(&self) -> &str {
        &self.help
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionKind {
    String,
    Integer,
    Number,
    Boolean,
    Flag,
    Json,
}

impl OptionKind {
    pub fn validate_value(self, value: &str) -> bool {
        match self {
            Self::String => true,
            Self::Integer => value.parse::<i64>().is_ok(),
            Self::Number => value.parse::<f64>().is_ok(),
            Self::Boolean => value.parse::<bool>().is_ok(),
            Self::Flag => value.is_empty(),
            Self::Json => serde_json::from_str::<Value>(value).is_ok(),
        }
    }

    pub fn type_name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Flag => "flag",
            Self::Json => "json",
        }
    }

    pub fn workflow_type(self) -> WorkflowValueType {
        match self {
            Self::String => WorkflowValueType::String,
            Self::Integer => WorkflowValueType::Integer,
            Self::Number => WorkflowValueType::Number,
            Self::Boolean | Self::Flag => WorkflowValueType::Boolean,
            Self::Json => WorkflowValueType::Json,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OptionPosition(NonZeroUsize);

impl OptionPosition {
    pub fn new(value: usize) -> Result<Self, String> {
        NonZeroUsize::new(value)
            .map(Self)
            .ok_or_else(|| "position must be at least 1".to_string())
    }

    pub fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Debug, Clone)]
pub struct OptionDeclaration {
    name: String,
    kind: OptionKind,
    short: Option<char>,
    long: Option<String>,
    position: Option<OptionPosition>,
    required: bool,
    repeatable: bool,
    help: String,
    values: Vec<String>,
}

impl OptionDeclaration {
    fn validate(
        name: String,
        raw: RawOption,
        command: &CommandPath,
    ) -> Result<Self, ProtocolError> {
        if !is_option_word(&name) {
            return Err(invalid_option(
                command,
                &name,
                "name must use lowercase ASCII letters, numbers, '-' or '_'",
            ));
        }
        if RESERVED_LONG_OPTIONS.contains(&name.as_str()) {
            return Err(invalid_option(
                command,
                &name,
                "name is reserved by the host",
            ));
        }

        let short = raw
            .short
            .map(|short| {
                let mut chars = short.chars();
                match (chars.next(), chars.next()) {
                    (Some(value), None) if value.is_ascii_alphanumeric() => Ok(value),
                    _ => Err(invalid_option(
                        command,
                        &name,
                        "short must be one ASCII letter or number without '-'",
                    )),
                }
            })
            .transpose()?;
        if short.is_some_and(|short| RESERVED_SHORT_OPTIONS.contains(&short)) {
            return Err(invalid_option(
                command,
                &name,
                "short name is reserved by the host",
            ));
        }

        let long = raw
            .long
            .map(|long| {
                if is_kebab_word(&long) {
                    Ok(long)
                } else {
                    Err(invalid_option(
                        command,
                        &name,
                        "long must use lowercase ASCII kebab-case without '--'",
                    ))
                }
            })
            .transpose()?;
        if long
            .as_deref()
            .is_some_and(|long| RESERVED_OPTION_KEYS.contains(&long))
        {
            return Err(invalid_option(
                command,
                &name,
                "long name is reserved by the host",
            ));
        }

        let position = raw
            .position
            .map(OptionPosition::new)
            .transpose()
            .map_err(|message| invalid_option(command, &name, &message))?;
        if position.is_some() {
            if short.is_some() || long.is_some() {
                return Err(invalid_option(
                    command,
                    &name,
                    "positional options cannot declare short or long names",
                ));
            }
            if raw.kind == OptionKind::Flag {
                return Err(invalid_option(
                    command,
                    &name,
                    "positional options cannot be flags",
                ));
            }
        } else if short.is_none() && long.is_none() {
            return Err(invalid_option(
                command,
                &name,
                "named options require a short or long name",
            ));
        }

        if raw.kind == OptionKind::Flag && !raw.values.is_empty() {
            return Err(invalid_option(
                command,
                &name,
                "flags cannot declare values",
            ));
        }
        for value in &raw.values {
            if !raw.kind.validate_value(value) {
                return Err(invalid_option(
                    command,
                    &name,
                    &format!(
                        "allowed value '{value}' is not a valid {}",
                        raw.kind.type_name()
                    ),
                ));
            }
        }

        Ok(Self {
            name,
            kind: raw.kind,
            short,
            long,
            position,
            required: raw.required,
            repeatable: raw.repeatable,
            help: raw.help,
            values: raw.values,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> OptionKind {
        self.kind
    }

    pub fn short(&self) -> Option<char> {
        self.short
    }

    pub fn long(&self) -> Option<&str> {
        self.long.as_deref()
    }

    pub fn positional(&self) -> bool {
        self.position.is_some()
    }

    pub fn position(&self) -> Option<OptionPosition> {
        self.position
    }

    pub fn required(&self) -> bool {
        self.required
    }

    pub fn repeatable(&self) -> bool {
        self.repeatable
    }

    pub fn help(&self) -> &str {
        &self.help
    }

    pub fn values(&self) -> &[String] {
        &self.values
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(default, rename = "$schema")]
    _schema: Option<String>,
    schema_version: u32,
    kind: ExtensionPackKind,
    name: String,
    version: String,
    requires_cli: String,
    protocol: Option<String>,
    executable: Option<String>,
    #[serde(default)]
    config: BTreeMap<String, RawValueDeclaration>,
    #[serde(default)]
    workflows: BTreeMap<String, RawWorkflow>,
    #[serde(default)]
    commands: BTreeMap<String, RawCommand>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    path: Vec<String>,
    #[serde(default)]
    arguments: Vec<String>,
    about: Option<String>,
    long_about: Option<String>,
    #[serde(default)]
    examples: Vec<String>,
    #[serde(default)]
    interactive: bool,
    #[serde(default)]
    options: BTreeMap<String, RawOption>,
    workflow: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflow {
    #[serde(default)]
    inputs: BTreeMap<String, RawWorkflowInput>,
    output: RawWorkflowOutput,
    #[serde(default)]
    steps: Vec<RawWorkflowStep>,
    #[serde(default)]
    capabilities: Vec<String>,
    result: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawWorkflowStep {
    Run {
        id: String,
        run: Vec<String>,
        #[serde(default, rename = "with")]
        bindings: BTreeMap<String, RawWorkflowBinding>,
        when: Option<String>,
    },
    Let {
        id: String,
        expr: String,
    },
    Assert {
        id: String,
        condition: String,
        message: String,
    },
    Call {
        id: String,
        call: String,
        #[serde(default, rename = "with")]
        bindings: BTreeMap<String, RawWorkflowBinding>,
        when: Option<String>,
    },
    ForEach {
        id: String,
        items: RawWorkflowBinding,
        #[serde(rename = "as")]
        item_name: String,
        call: String,
        #[serde(default, rename = "with")]
        bindings: BTreeMap<String, RawWorkflowBinding>,
        max_items: usize,
        when: Option<String>,
    },
}

impl RawWorkflowStep {
    fn id(&self) -> &str {
        match self {
            Self::Run { id, .. }
            | Self::Let { id, .. }
            | Self::Assert { id, .. }
            | Self::Call { id, .. }
            | Self::ForEach { id, .. } => id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawWorkflowBinding {
    Source(RawWorkflowBindingSource),
    Literal(Value),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflowBindingSource {
    input: Option<String>,
    config: Option<String>,
    step: Option<String>,
    literal: Option<Value>,
    select: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValueDeclaration {
    #[serde(rename = "type")]
    value_type: WorkflowValueType,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    repeatable: bool,
    default: Option<Value>,
    #[serde(default)]
    help: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflowInput {
    #[serde(rename = "type")]
    value_type: WorkflowValueType,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    repeatable: bool,
    default: Option<Value>,
    #[serde(default)]
    help: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflowOutput {
    shape: SemanticOutputShape,
    #[serde(default = "default_workflow_value_type", rename = "type")]
    value_type: WorkflowValueType,
    #[serde(default)]
    columns: Vec<String>,
}

fn default_workflow_value_type() -> WorkflowValueType {
    WorkflowValueType::Json
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOption {
    kind: OptionKind,
    short: Option<String>,
    long: Option<String>,
    position: Option<usize>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    repeatable: bool,
    #[serde(default)]
    help: String,
    #[serde(default)]
    values: Vec<String>,
}

fn invalid_option(command: &CommandPath, option: &str, message: &str) -> ProtocolError {
    ProtocolError::InvalidOption {
        command: command.display(),
        option: option.to_string(),
        message: message.to_string(),
    }
}

fn invalid_workflow(command: &CommandPath, message: &str) -> ProtocolError {
    ProtocolError::InvalidWorkflow {
        command: command.display(),
        message: message.to_string(),
    }
}

fn invalid_named_workflow(workflow: &str, message: &str) -> ProtocolError {
    ProtocolError::InvalidNamedWorkflow {
        workflow: workflow.to_string(),
        message: message.to_string(),
    }
}

fn invalid_config(key: &str, message: &str) -> ProtocolError {
    ProtocolError::InvalidConfig {
        key: key.to_string(),
        message: message.to_string(),
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn is_kebab_word(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_option_word(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        })
        && !value.ends_with(['-', '_'])
}

fn is_snake_word(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
        && !value.ends_with('_')
        && !value.contains("__")
}

fn workflow_value_contains_nul(value: &Value) -> bool {
    match value {
        Value::String(value) => value.contains('\0'),
        Value::Array(values) => values.iter().any(workflow_value_contains_nul),
        Value::Object(values) => values.values().any(workflow_value_contains_nul),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests;
