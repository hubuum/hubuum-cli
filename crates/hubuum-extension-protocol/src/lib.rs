use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_V1: &str = "hubuum-cli.extension/v1";
pub const MANIFEST_FILENAME: &str = "hubuum-extension.toml";

const RESERVED_PACK_NAMES: &[&str] = &[
    "disable", "doctor", "enable", "explain", "install", "list", "reload", "remove", "show",
    "upgrade", "validate",
];
const RESERVED_LONG_OPTIONS: &[&str] = &["help", "json", "output", "table-headers"];
const RESERVED_SHORT_OPTIONS: &[char] = &['h', 'j', 'o'];
const RESERVED_OPTION_KEYS: &[&str] = &["h", "help", "j", "json", "o", "output", "table-headers"];

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid TOML manifest: {0}")]
    InvalidManifestToml(#[from] toml::de::Error),
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

#[derive(Debug, Clone)]
pub struct ExtensionManifest {
    kind: ExtensionPackKind,
    name: PackName,
    version: Version,
    requires_cli: VersionReq,
    protocol: Option<ProtocolVersion>,
    executable: Option<ExecutablePath>,
    config: BTreeMap<String, ConfigDeclaration>,
    workflows: BTreeMap<WorkflowName, WorkflowDeclaration>,
    commands: Vec<CommandDeclaration>,
}

impl ExtensionManifest {
    pub fn parse(input: &str) -> Result<Self, ProtocolError> {
        let raw: RawManifest = toml::from_str(input)?;
        if raw.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchemaVersion(raw.schema_version));
        }

        let name = PackName::new(raw.name)?;
        let version =
            Version::parse(&raw.version).map_err(|source| ProtocolError::InvalidVersion {
                value: raw.version,
                source,
            })?;
        let requires_cli = VersionReq::parse(&raw.requires_cli).map_err(|source| {
            ProtocolError::InvalidVersionRequirement {
                value: raw.requires_cli,
                source,
            }
        })?;
        let protocol = raw.protocol.map(ProtocolVersion::new).transpose()?;
        let executable = raw.executable.map(ExecutablePath::new).transpose()?;
        validate_pack_kind(
            raw.kind,
            protocol.as_ref(),
            executable.as_ref(),
            &raw.workflows,
        )?;

        let config = raw
            .config
            .into_iter()
            .map(|(key, declaration)| {
                ConfigDeclaration::validate(key.clone(), declaration)
                    .map(|declaration| (key, declaration))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let mut workflows = BTreeMap::new();
        for (name, workflow) in raw.workflows {
            let name = WorkflowName::new(name.clone())
                .map_err(|message| invalid_named_workflow(&name, &message))?;
            let workflow = WorkflowDeclaration::validate(&name, workflow, &config)?;
            workflows.insert(name, workflow);
        }
        validate_workflow_calls(&workflows)?;

        let mut command_paths = HashSet::new();
        let mut commands: Vec<CommandDeclaration> = Vec::with_capacity(raw.commands.len());
        for command in raw.commands {
            let command = CommandDeclaration::validate(command, raw.kind, &workflows)?;
            if !command_paths.insert(command.path.clone()) {
                return Err(ProtocolError::DuplicateCommandPath(command.path.display()));
            }
            for existing in &commands {
                let existing_path = existing.path().segments();
                let command_path = command.path().segments();
                let conflict = if existing_path.len() < command_path.len()
                    && command_path.starts_with(existing_path)
                {
                    Some((existing.path().display(), command.path().display()))
                } else if command_path.len() < existing_path.len()
                    && existing_path.starts_with(command_path)
                {
                    Some((command.path().display(), existing.path().display()))
                } else {
                    None
                };
                if let Some((prefix, command)) = conflict {
                    return Err(ProtocolError::CommandPathPrefix { prefix, command });
                }
            }
            commands.push(command);
        }
        if commands.is_empty() {
            return Err(ProtocolError::InvalidCommandPath(
                "at least one command is required".to_string(),
            ));
        }

        Ok(Self {
            kind: raw.kind,
            name,
            version,
            requires_cli,
            protocol,
            executable,
            config,
            workflows,
            commands,
        })
    }

    pub fn kind(&self) -> ExtensionPackKind {
        self.kind
    }

    pub fn is_portable(&self) -> bool {
        self.kind == ExtensionPackKind::Portable
    }

    pub fn name(&self) -> &PackName {
        &self.name
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn requires_cli(&self) -> &VersionReq {
        &self.requires_cli
    }

    pub fn supports_cli(&self, version: &Version) -> bool {
        self.requires_cli.matches(version)
    }

    pub fn protocol(&self) -> Option<&ProtocolVersion> {
        self.protocol.as_ref()
    }

    pub fn executable(&self) -> Option<&ExecutablePath> {
        self.executable.as_ref()
    }

    pub fn config(&self) -> &BTreeMap<String, ConfigDeclaration> {
        &self.config
    }

    pub fn workflows(&self) -> &BTreeMap<WorkflowName, WorkflowDeclaration> {
        &self.workflows
    }

    pub fn workflow(&self, name: &WorkflowName) -> Option<&WorkflowDeclaration> {
        self.workflows.get(name)
    }

    pub fn commands(&self) -> &[CommandDeclaration] {
        &self.commands
    }

    pub fn resolve_config(&self, value: &Value) -> Result<Value, ProtocolError> {
        let supplied = value.as_object().ok_or_else(|| {
            invalid_config("<root>", "extension configuration must be a TOML table")
        })?;
        if let Some(key) = supplied.keys().find(|key| !self.config.contains_key(*key)) {
            return Err(invalid_config(key, "unknown configuration key"));
        }

        let mut resolved = serde_json::Map::new();
        for declaration in self.config.values() {
            let value = supplied
                .get(declaration.key())
                .cloned()
                .or_else(|| declaration.default().cloned());
            match value {
                Some(value) => {
                    validate_declared_value(
                        &value,
                        declaration.value_type(),
                        declaration.repeatable(),
                    )
                    .map_err(|message| invalid_config(declaration.key(), &message))?;
                    resolved.insert(declaration.key().to_string(), value);
                }
                None if declaration.required() => {
                    return Err(invalid_config(
                        declaration.key(),
                        "required configuration value is missing",
                    ));
                }
                None => {}
            }
        }
        Ok(Value::Object(resolved))
    }
}

fn validate_pack_kind(
    kind: ExtensionPackKind,
    protocol: Option<&ProtocolVersion>,
    executable: Option<&ExecutablePath>,
    workflows: &BTreeMap<String, RawWorkflow>,
) -> Result<(), ProtocolError> {
    let invalid = |message: &str| ProtocolError::InvalidPackKind {
        kind: kind.as_str().to_string(),
        message: message.to_string(),
    };
    match kind {
        ExtensionPackKind::Portable => {
            if protocol.is_some() || executable.is_some() {
                return Err(invalid(
                    "portable packs cannot declare protocol or executable; they run only through hubuum-cli workflows",
                ));
            }
            if workflows.is_empty() {
                return Err(invalid("portable packs must declare at least one workflow"));
            }
        }
        ExtensionPackKind::Executable => {
            if protocol.is_none() || executable.is_none() {
                return Err(invalid(
                    "executable packs must declare both protocol and executable",
                ));
            }
            if !workflows.is_empty() {
                return Err(invalid("executable packs cannot declare workflows"));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CommandDeclaration {
    path: CommandPath,
    about: Option<String>,
    long_about: Option<String>,
    examples: Vec<String>,
    options: Vec<OptionDeclaration>,
    implementation: CommandImplementation,
}

#[derive(Debug, Clone)]
pub enum CommandImplementation {
    Executable {
        arguments: Vec<String>,
        interactive: bool,
    },
    Workflow(WorkflowName),
}

impl CommandDeclaration {
    fn validate(
        raw: RawCommand,
        kind: ExtensionPackKind,
        workflows: &BTreeMap<WorkflowName, WorkflowDeclaration>,
    ) -> Result<Self, ProtocolError> {
        let path = CommandPath::new(raw.path)?;
        if raw.arguments.iter().any(|argument| argument.contains('\0')) {
            return Err(ProtocolError::InvalidCommandPath(format!(
                "command '{}' contains a NUL argument",
                path.display()
            )));
        }

        let mut names = HashSet::new();
        let mut aliases = HashSet::new();
        let mut options = Vec::with_capacity(raw.options.len());
        let mut optional_positional_seen = false;
        let positional_count = raw
            .options
            .iter()
            .filter(|option| option.positional)
            .count();
        let mut positional_index = 0;

        for option in raw.options {
            let declaration = OptionDeclaration::validate(option, &path)?;
            if !names.insert(declaration.name.clone()) {
                return Err(invalid_option(
                    &path,
                    &declaration.name,
                    "duplicate option name",
                ));
            }
            if let Some(short) = declaration.short {
                if !aliases.insert(short.to_string()) {
                    return Err(invalid_option(
                        &path,
                        &declaration.name,
                        "option aliases must be unique after removing dashes",
                    ));
                }
            }
            if let Some(long) = &declaration.long {
                if !aliases.insert(long.clone()) {
                    return Err(invalid_option(
                        &path,
                        &declaration.name,
                        "option aliases must be unique after removing dashes",
                    ));
                }
            }
            if declaration.positional {
                positional_index += 1;
                if optional_positional_seen && declaration.required {
                    return Err(invalid_option(
                        &path,
                        &declaration.name,
                        "required positionals cannot follow optional positionals",
                    ));
                }
                optional_positional_seen |= !declaration.required;
                if declaration.repeatable && positional_index != positional_count {
                    return Err(invalid_option(
                        &path,
                        &declaration.name,
                        "only the final positional may be repeatable",
                    ));
                }
            }
            options.push(declaration);
        }

        let implementation = match (kind, raw.workflow) {
            (ExtensionPackKind::Portable, Some(name)) => {
                if !raw.arguments.is_empty() {
                    return Err(invalid_workflow(
                        &path,
                        "portable workflow commands cannot declare executable arguments",
                    ));
                }
                if raw.interactive {
                    return Err(invalid_workflow(
                        &path,
                        "portable workflow commands cannot be interactive",
                    ));
                }
                let name = WorkflowName::new(name.clone())
                    .map_err(|message| invalid_workflow(&path, &message))?;
                let workflow = workflows.get(&name).ok_or_else(|| {
                    invalid_workflow(
                        &path,
                        &format!("references unknown workflow '{}'", name.as_str()),
                    )
                })?;
                validate_command_inputs(&path, &options, workflow.inputs())?;
                CommandImplementation::Workflow(name)
            }
            (ExtensionPackKind::Portable, None) => {
                return Err(invalid_workflow(
                    &path,
                    "portable commands must reference a workflow",
                ));
            }
            (ExtensionPackKind::Executable, Some(_)) => {
                return Err(invalid_workflow(
                    &path,
                    "executable commands cannot reference workflows",
                ));
            }
            (ExtensionPackKind::Executable, None) => CommandImplementation::Executable {
                arguments: raw.arguments,
                interactive: raw.interactive,
            },
        };
        Ok(Self {
            path,
            about: nonempty(raw.about),
            long_about: nonempty(raw.long_about),
            examples: raw.examples,
            options,
            implementation,
        })
    }

    pub fn path(&self) -> &CommandPath {
        &self.path
    }

    pub fn arguments(&self) -> &[String] {
        match &self.implementation {
            CommandImplementation::Executable { arguments, .. } => arguments,
            CommandImplementation::Workflow(_) => &[],
        }
    }

    pub fn about(&self) -> Option<&str> {
        self.about.as_deref()
    }

    pub fn long_about(&self) -> Option<&str> {
        self.long_about.as_deref()
    }

    pub fn examples(&self) -> &[String] {
        &self.examples
    }

    pub fn interactive(&self) -> bool {
        matches!(
            self.implementation,
            CommandImplementation::Executable {
                interactive: true,
                ..
            }
        )
    }

    pub fn options(&self) -> &[OptionDeclaration] {
        &self.options
    }

    pub fn workflow(&self) -> Option<&WorkflowName> {
        match &self.implementation {
            CommandImplementation::Executable { .. } => None,
            CommandImplementation::Workflow(workflow) => Some(workflow),
        }
    }

    pub fn implementation(&self) -> &CommandImplementation {
        &self.implementation
    }
}

fn validate_command_inputs(
    command: &CommandPath,
    options: &[OptionDeclaration],
    inputs: &[WorkflowInputDeclaration],
) -> Result<(), ProtocolError> {
    if options.len() != inputs.len() {
        return Err(invalid_workflow(
            command,
            "command options must exactly match the referenced workflow inputs",
        ));
    }
    for input in inputs {
        let option = options
            .iter()
            .find(|option| option.name() == input.name())
            .ok_or_else(|| {
                invalid_workflow(
                    command,
                    &format!(
                        "missing command option for workflow input '{}'",
                        input.name()
                    ),
                )
            })?;
        if input.value_type() != option.kind().workflow_type()
            || input.required() != option.required()
            || input.repeatable() != option.repeatable()
        {
            return Err(invalid_workflow(
                command,
                &format!(
                    "option '{}' must match workflow input type={}, required={}, repeatable={}",
                    input.name(),
                    input.value_type().as_str(),
                    input.required(),
                    input.repeatable()
                ),
            ));
        }
        if option.kind() == OptionKind::Flag && input.repeatable() {
            return Err(invalid_workflow(
                command,
                &format!(
                    "option '{}' cannot use flag kind for a repeatable workflow input",
                    input.name()
                ),
            ));
        }
        if option.kind() == OptionKind::Flag
            && input
                .default()
                .is_some_and(|default| default != &Value::Bool(false))
        {
            return Err(invalid_workflow(
                command,
                &format!(
                    "option '{}' cannot use flag kind when the workflow input defaults to true",
                    input.name()
                ),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct WorkflowInputDeclaration {
    name: String,
    value_type: WorkflowValueType,
    required: bool,
    repeatable: bool,
    default: Option<Value>,
    help: String,
}

impl WorkflowInputDeclaration {
    fn validate(workflow: &WorkflowName, raw: RawWorkflowInput) -> Result<Self, ProtocolError> {
        if !is_option_word(&raw.name) {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                &format!(
                    "input '{}' must use lowercase ASCII letters, numbers, '-' or '_'",
                    raw.name
                ),
            ));
        }
        if raw.required && raw.default.is_some() {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                &format!(
                    "input '{}' cannot be required and also declare a default",
                    raw.name
                ),
            ));
        }
        validate_declared_default(&raw.default, raw.value_type, raw.repeatable, |message| {
            invalid_named_workflow(workflow.as_str(), &message)
        })?;
        Ok(Self {
            name: raw.name,
            value_type: raw.value_type,
            required: raw.required,
            repeatable: raw.repeatable,
            default: raw.default,
            help: raw.help,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
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

#[derive(Debug, Clone)]
pub struct WorkflowOutputDeclaration {
    shape: SemanticOutputShape,
    value_type: WorkflowValueType,
    columns: Vec<String>,
}

impl WorkflowOutputDeclaration {
    fn validate(workflow: &WorkflowName, raw: RawWorkflowOutput) -> Result<Self, ProtocolError> {
        if raw.columns.iter().any(|column| column.trim().is_empty()) {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                "output columns cannot be empty",
            ));
        }
        let mut unique = HashSet::new();
        if raw.columns.iter().any(|column| !unique.insert(column)) {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                "output columns must be unique",
            ));
        }
        if !raw.columns.is_empty()
            && !matches!(
                raw.shape,
                SemanticOutputShape::Rows | SemanticOutputShape::Detail
            )
        {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                "output columns are only valid for rows and detail shapes",
            ));
        }
        Ok(Self {
            shape: raw.shape,
            value_type: raw.value_type,
            columns: raw.columns,
        })
    }

    pub fn shape(&self) -> SemanticOutputShape {
        self.shape
    }

    pub fn value_type(&self) -> WorkflowValueType {
        self.value_type
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn validate_value(&self, value: &Value) -> Result<(), ProtocolError> {
        SemanticOutput::new(self.shape, value.clone(), self.columns.clone())?;
        let typed_values: Vec<&Value> = match self.shape {
            SemanticOutputShape::Values | SemanticOutputShape::Lines => {
                value.as_array().into_iter().flatten().collect()
            }
            SemanticOutputShape::Empty => Vec::new(),
            SemanticOutputShape::Rows
            | SemanticOutputShape::Detail
            | SemanticOutputShape::Message => vec![value],
        };
        if typed_values
            .into_iter()
            .any(|item| !self.value_type.accepts(item))
        {
            return Err(ProtocolError::InvalidOutput(format!(
                "workflow output contains a value incompatible with declared type '{}'",
                self.value_type.as_str()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowDeclaration {
    name: WorkflowName,
    inputs: Vec<WorkflowInputDeclaration>,
    output: WorkflowOutputDeclaration,
    steps: Vec<WorkflowStep>,
    capabilities: Vec<WorkflowCapability>,
    result: String,
}

impl WorkflowDeclaration {
    fn validate(
        name: &WorkflowName,
        raw: RawWorkflow,
        config: &BTreeMap<String, ConfigDeclaration>,
    ) -> Result<Self, ProtocolError> {
        if raw.steps.is_empty() {
            return Err(invalid_named_workflow(
                name.as_str(),
                "at least one step is required",
            ));
        }
        if raw.result.trim().is_empty() {
            return Err(invalid_named_workflow(
                name.as_str(),
                "result expression cannot be empty",
            ));
        }

        let mut input_names = HashSet::new();
        let mut inputs = Vec::with_capacity(raw.inputs.len());
        for input in raw.inputs {
            let input = WorkflowInputDeclaration::validate(name, input)?;
            if !input_names.insert(input.name.clone()) {
                return Err(invalid_named_workflow(
                    name.as_str(),
                    &format!("duplicate input '{}'", input.name()),
                ));
            }
            inputs.push(input);
        }

        let mut capabilities = Vec::with_capacity(raw.capabilities.len());
        for capability in raw.capabilities {
            let capability = WorkflowCapability::new(&capability)
                .map_err(|message| invalid_named_workflow(name.as_str(), &message))?;
            if capabilities.contains(&capability) {
                return Err(invalid_named_workflow(
                    name.as_str(),
                    &format!("duplicate workflow capability '{}'", capability.as_str()),
                ));
            }
            capabilities.push(capability);
        }

        let mut ids = HashSet::new();
        let mut steps = Vec::with_capacity(raw.steps.len());
        for raw_step in raw.steps {
            let step = WorkflowStep::validate(name, raw_step, &inputs, config, &ids)?;
            if !ids.insert(step.id().clone()) {
                return Err(invalid_named_workflow(
                    name.as_str(),
                    &format!("duplicate step id '{}'", step.id().as_str()),
                ));
            }
            steps.push(step);
        }
        Ok(Self {
            name: name.clone(),
            inputs,
            output: WorkflowOutputDeclaration::validate(name, raw.output)?,
            steps,
            capabilities,
            result: raw.result,
        })
    }

    pub fn name(&self) -> &WorkflowName {
        &self.name
    }

    pub fn inputs(&self) -> &[WorkflowInputDeclaration] {
        &self.inputs
    }

    pub fn output(&self) -> &WorkflowOutputDeclaration {
        &self.output
    }

    pub fn steps(&self) -> &[WorkflowStep] {
        &self.steps
    }

    pub fn capabilities(&self) -> &[WorkflowCapability] {
        &self.capabilities
    }

    pub fn allows_mutation(&self) -> bool {
        self.capabilities.contains(&WorkflowCapability::Mutate)
    }

    pub fn result(&self) -> &str {
        &self.result
    }

    pub fn resolve_inputs(&self, value: &Value) -> Result<Value, ProtocolError> {
        let supplied = value.as_object().ok_or_else(|| {
            invalid_named_workflow(self.name.as_str(), "workflow inputs must be an object")
        })?;
        if let Some(key) = supplied
            .keys()
            .find(|key| !self.inputs.iter().any(|input| input.name() == *key))
        {
            return Err(invalid_named_workflow(
                self.name.as_str(),
                &format!("unknown workflow input '{key}'"),
            ));
        }
        let mut resolved = serde_json::Map::new();
        for declaration in &self.inputs {
            let value = supplied
                .get(declaration.name())
                .cloned()
                .or_else(|| declaration.default().cloned());
            match value {
                Some(value) => {
                    validate_declared_value(
                        &value,
                        declaration.value_type(),
                        declaration.repeatable(),
                    )
                    .map_err(|message| {
                        invalid_named_workflow(
                            self.name.as_str(),
                            &format!("input '{}': {message}", declaration.name()),
                        )
                    })?;
                    resolved.insert(declaration.name().to_string(), value);
                }
                None if declaration.required() => {
                    return Err(invalid_named_workflow(
                        self.name.as_str(),
                        &format!("required input '{}' is missing", declaration.name()),
                    ));
                }
                None => {}
            }
        }
        Ok(Value::Object(resolved))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowCapability {
    Mutate,
}

impl WorkflowCapability {
    fn new(value: &str) -> Result<Self, String> {
        match value {
            "mutate" => Ok(Self::Mutate),
            _ => Err(format!(
                "unknown workflow capability '{value}'; supported capabilities: mutate"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mutate => "mutate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkflowStepId(String);

impl WorkflowStepId {
    fn new(value: String) -> Result<Self, String> {
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

#[derive(Debug, Clone)]
pub enum WorkflowStep {
    Run(WorkflowRunStep),
    Let(WorkflowLetStep),
    Assert(WorkflowAssertStep),
    Call(WorkflowCallStep),
    ForEach(WorkflowForEachStep),
}

impl WorkflowStep {
    fn validate(
        workflow: &WorkflowName,
        raw: RawWorkflowStep,
        inputs: &[WorkflowInputDeclaration],
        config: &BTreeMap<String, ConfigDeclaration>,
        prior_step_ids: &HashSet<WorkflowStepId>,
    ) -> Result<Self, ProtocolError> {
        let invalid = |message: String| invalid_named_workflow(workflow.as_str(), &message);
        match raw {
            RawWorkflowStep::Run {
                id,
                run,
                bindings,
                when,
            } => {
                let id = validate_step_id(workflow, id)?;
                let run = CommandPath::new(run)
                    .map_err(|error| invalid(format!("step '{}': {error}", id.as_str())))?;
                if run
                    .segments()
                    .first()
                    .is_some_and(|part| part == "extension")
                {
                    return Err(invalid(format!(
                        "step '{}' cannot invoke extension commands",
                        id.as_str()
                    )));
                }
                Ok(Self::Run(WorkflowRunStep {
                    id,
                    run,
                    bindings: validate_bindings(
                        workflow,
                        bindings,
                        inputs,
                        config,
                        prior_step_ids,
                    )?,
                    when: validate_optional_expression(workflow, "when", when)?,
                }))
            }
            RawWorkflowStep::Let { id, expr } => Ok(Self::Let(WorkflowLetStep {
                id: validate_step_id(workflow, id)?,
                expr: validate_expression(workflow, "let expression", expr)?,
            })),
            RawWorkflowStep::Assert {
                id,
                condition,
                message,
            } => {
                if message.trim().is_empty() || message.contains('\0') {
                    return Err(invalid(format!(
                        "assert step '{id}' message must be non-empty and contain no NUL"
                    )));
                }
                Ok(Self::Assert(WorkflowAssertStep {
                    id: validate_step_id(workflow, id)?,
                    condition: validate_expression(workflow, "assert condition", condition)?,
                    message,
                }))
            }
            RawWorkflowStep::Call {
                id,
                call,
                bindings,
                when,
            } => Ok(Self::Call(WorkflowCallStep {
                id: validate_step_id(workflow, id)?,
                call: WorkflowName::new(call.clone())
                    .map_err(|message| invalid(format!("call target {message}")))?,
                bindings: validate_bindings(workflow, bindings, inputs, config, prior_step_ids)?,
                when: validate_optional_expression(workflow, "when", when)?,
            })),
            RawWorkflowStep::ForEach {
                id,
                items,
                item_name,
                call,
                bindings,
                max_items,
                when,
            } => {
                let id = validate_step_id(workflow, id)?;
                if max_items == 0 {
                    return Err(invalid(format!(
                        "for_each step '{}' max_items must be greater than zero",
                        id.as_str()
                    )));
                }
                if !is_option_word(&item_name) {
                    return Err(invalid(format!(
                        "for_each step '{}' as value '{}' is not a valid input name",
                        id.as_str(),
                        item_name
                    )));
                }
                Ok(Self::ForEach(WorkflowForEachStep {
                    id,
                    items: WorkflowBinding::validate(
                        items,
                        workflow,
                        inputs,
                        config,
                        prior_step_ids,
                    )?,
                    item_name,
                    call: WorkflowName::new(call.clone())
                        .map_err(|message| invalid(format!("call target {message}")))?,
                    bindings: validate_bindings(
                        workflow,
                        bindings,
                        inputs,
                        config,
                        prior_step_ids,
                    )?,
                    max_items,
                    when: validate_optional_expression(workflow, "when", when)?,
                }))
            }
        }
    }

    pub fn id(&self) -> &WorkflowStepId {
        match self {
            Self::Run(step) => step.id(),
            Self::Let(step) => step.id(),
            Self::Assert(step) => step.id(),
            Self::Call(step) => step.id(),
            Self::ForEach(step) => step.id(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowRunStep {
    id: WorkflowStepId,
    run: CommandPath,
    bindings: BTreeMap<WorkflowBindingName, WorkflowBinding>,
    when: Option<String>,
}

impl WorkflowRunStep {
    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }

    pub fn run(&self) -> &CommandPath {
        &self.run
    }

    pub fn bindings(&self) -> &BTreeMap<WorkflowBindingName, WorkflowBinding> {
        &self.bindings
    }

    pub fn when(&self) -> Option<&str> {
        self.when.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowLetStep {
    id: WorkflowStepId,
    expr: String,
}

impl WorkflowLetStep {
    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }

    pub fn expr(&self) -> &str {
        &self.expr
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowAssertStep {
    id: WorkflowStepId,
    condition: String,
    message: String,
}

impl WorkflowAssertStep {
    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }

    pub fn condition(&self) -> &str {
        &self.condition
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowCallStep {
    id: WorkflowStepId,
    call: WorkflowName,
    bindings: BTreeMap<WorkflowBindingName, WorkflowBinding>,
    when: Option<String>,
}

impl WorkflowCallStep {
    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }

    pub fn call(&self) -> &WorkflowName {
        &self.call
    }

    pub fn bindings(&self) -> &BTreeMap<WorkflowBindingName, WorkflowBinding> {
        &self.bindings
    }

    pub fn when(&self) -> Option<&str> {
        self.when.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowForEachStep {
    id: WorkflowStepId,
    items: WorkflowBinding,
    item_name: String,
    call: WorkflowName,
    bindings: BTreeMap<WorkflowBindingName, WorkflowBinding>,
    max_items: usize,
    when: Option<String>,
}

impl WorkflowForEachStep {
    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }

    pub fn items(&self) -> &WorkflowBinding {
        &self.items
    }

    pub fn item_name(&self) -> &str {
        &self.item_name
    }

    pub fn call(&self) -> &WorkflowName {
        &self.call
    }

    pub fn bindings(&self) -> &BTreeMap<WorkflowBindingName, WorkflowBinding> {
        &self.bindings
    }

    pub fn max_items(&self) -> usize {
        self.max_items
    }

    pub fn when(&self) -> Option<&str> {
        self.when.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkflowBindingName(String);

impl WorkflowBindingName {
    fn new(value: String) -> Result<Self, String> {
        if is_option_word(&value) {
            Ok(Self(value))
        } else {
            Err(format!(
                "binding name '{value}' must use lowercase ASCII letters, numbers, '-' or '_'"
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub enum WorkflowBinding {
    Literal(Value),
    Input {
        name: String,
    },
    Config {
        key: String,
    },
    Step {
        step: WorkflowStepId,
        select: Option<String>,
    },
}

impl WorkflowBinding {
    fn validate(
        raw: RawWorkflowBinding,
        workflow: &WorkflowName,
        inputs: &[WorkflowInputDeclaration],
        config: &BTreeMap<String, ConfigDeclaration>,
        prior_step_ids: &HashSet<WorkflowStepId>,
    ) -> Result<Self, ProtocolError> {
        let invalid = |message: &str| invalid_named_workflow(workflow.as_str(), message);
        let raw = match raw {
            RawWorkflowBinding::Literal(value) => {
                if value.is_object() {
                    return Err(invalid(
                        "workflow binding tables must declare exactly one of input, config, or step",
                    ));
                }
                if workflow_value_contains_nul(&value) {
                    return Err(invalid("workflow binding literals cannot contain NUL"));
                }
                return Ok(Self::Literal(value));
            }
            RawWorkflowBinding::Source(raw) => raw,
        };
        let sources = [
            raw.input.is_some(),
            raw.config.is_some(),
            raw.step.is_some(),
            raw.literal.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        if sources != 1 {
            return Err(invalid(
                "each workflow binding source must declare exactly one of input, config, step, or literal",
            ));
        }
        if raw.select.is_some() && raw.step.is_none() {
            return Err(invalid(
                "a workflow binding select expression is only valid with step",
            ));
        }
        if raw
            .select
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(invalid(
                "a workflow binding select expression cannot be empty",
            ));
        }

        if let Some(input) = raw.input {
            if !inputs.iter().any(|candidate| candidate.name() == input) {
                return Err(invalid(&format!(
                    "workflow binding references unknown input '{input}'"
                )));
            }
            Ok(Self::Input { name: input })
        } else if let Some(key) = raw.config {
            if !config.contains_key(&key) {
                return Err(invalid(&format!(
                    "workflow binding references undeclared config key '{key}'"
                )));
            }
            Ok(Self::Config { key })
        } else if let Some(literal) = raw.literal {
            if workflow_value_contains_nul(&literal) {
                return Err(invalid("workflow binding literals cannot contain NUL"));
            }
            Ok(Self::Literal(literal))
        } else {
            let step = WorkflowStepId::new(raw.step.expect("one binding source was present"))
                .map_err(|message| invalid(&format!("invalid step reference: {message}")))?;
            if !prior_step_ids.contains(&step) {
                return Err(invalid(&format!(
                    "step output reference '{}' must name an earlier step",
                    step.as_str()
                )));
            }
            Ok(Self::Step {
                step,
                select: raw.select,
            })
        }
    }
}

fn validate_workflow_calls(
    workflows: &BTreeMap<WorkflowName, WorkflowDeclaration>,
) -> Result<(), ProtocolError> {
    for workflow in workflows.values() {
        for step in workflow.steps() {
            let (target, item_name, bindings) = match step {
                WorkflowStep::Call(step) => (step.call(), None, step.bindings()),
                WorkflowStep::ForEach(step) => {
                    (step.call(), Some(step.item_name()), step.bindings())
                }
                WorkflowStep::Run(_) | WorkflowStep::Let(_) | WorkflowStep::Assert(_) => continue,
            };
            let target_workflow = workflows.get(target).ok_or_else(|| {
                invalid_named_workflow(
                    workflow.name().as_str(),
                    &format!(
                        "step '{}' calls unknown workflow '{}'",
                        step.id().as_str(),
                        target.as_str()
                    ),
                )
            })?;
            for binding in bindings.keys() {
                if !target_workflow
                    .inputs()
                    .iter()
                    .any(|input| input.name() == binding.as_str())
                {
                    return Err(invalid_named_workflow(
                        workflow.name().as_str(),
                        &format!(
                            "step '{}' binds unknown input '{}' on workflow '{}'",
                            step.id().as_str(),
                            binding.as_str(),
                            target.as_str()
                        ),
                    ));
                }
            }
            if let Some(item_name) = item_name {
                if bindings.keys().any(|binding| binding.as_str() == item_name) {
                    return Err(invalid_named_workflow(
                        workflow.name().as_str(),
                        &format!(
                            "for_each step '{}' cannot bind '{}' in both as and with",
                            step.id().as_str(),
                            item_name
                        ),
                    ));
                }
                if !target_workflow
                    .inputs()
                    .iter()
                    .any(|input| input.name() == item_name)
                {
                    return Err(invalid_named_workflow(
                        workflow.name().as_str(),
                        &format!(
                            "for_each step '{}' as value '{}' is not an input of workflow '{}'",
                            step.id().as_str(),
                            item_name,
                            target.as_str()
                        ),
                    ));
                }
            }
            for input in target_workflow.inputs() {
                let supplied = bindings
                    .keys()
                    .any(|binding| binding.as_str() == input.name())
                    || item_name == Some(input.name())
                    || input.default().is_some();
                if input.required() && !supplied {
                    return Err(invalid_named_workflow(
                        workflow.name().as_str(),
                        &format!(
                            "step '{}' does not bind required input '{}' on workflow '{}'",
                            step.id().as_str(),
                            input.name(),
                            target.as_str()
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_step_id(
    workflow: &WorkflowName,
    value: String,
) -> Result<WorkflowStepId, ProtocolError> {
    WorkflowStepId::new(value).map_err(|message| {
        invalid_named_workflow(workflow.as_str(), &format!("invalid step id: {message}"))
    })
}

fn validate_bindings(
    workflow: &WorkflowName,
    raw: BTreeMap<String, RawWorkflowBinding>,
    inputs: &[WorkflowInputDeclaration],
    config: &BTreeMap<String, ConfigDeclaration>,
    prior_step_ids: &HashSet<WorkflowStepId>,
) -> Result<BTreeMap<WorkflowBindingName, WorkflowBinding>, ProtocolError> {
    raw.into_iter()
        .map(|(name, binding)| {
            let name = WorkflowBindingName::new(name).map_err(|message| {
                invalid_named_workflow(workflow.as_str(), &format!("binding: {message}"))
            })?;
            let binding =
                WorkflowBinding::validate(binding, workflow, inputs, config, prior_step_ids)?;
            Ok((name, binding))
        })
        .collect()
}

fn validate_expression(
    workflow: &WorkflowName,
    label: &str,
    expression: String,
) -> Result<String, ProtocolError> {
    if expression.trim().is_empty() || expression.contains('\0') {
        Err(invalid_named_workflow(
            workflow.as_str(),
            &format!("{label} must be non-empty and contain no NUL"),
        ))
    } else {
        Ok(expression)
    }
}

fn validate_optional_expression(
    workflow: &WorkflowName,
    label: &str,
    expression: Option<String>,
) -> Result<Option<String>, ProtocolError> {
    expression
        .map(|expression| validate_expression(workflow, label, expression))
        .transpose()
}

fn validate_declared_default<F>(
    default: &Option<Value>,
    value_type: WorkflowValueType,
    repeatable: bool,
    invalid: F,
) -> Result<(), ProtocolError>
where
    F: Fn(String) -> ProtocolError,
{
    if let Some(value) = default {
        if workflow_value_contains_nul(value) {
            return Err(invalid("default value cannot contain NUL".to_string()));
        }
        validate_declared_value(value, value_type, repeatable)
            .map_err(|message| invalid(format!("invalid default: {message}")))?;
    }
    Ok(())
}

fn validate_declared_value(
    value: &Value,
    value_type: WorkflowValueType,
    repeatable: bool,
) -> Result<(), String> {
    if repeatable {
        let values = value
            .as_array()
            .ok_or_else(|| "repeatable value must be an array".to_string())?;
        if values.iter().all(|value| value_type.accepts(value)) {
            Ok(())
        } else {
            Err(format!(
                "array items must have type '{}'",
                value_type.as_str()
            ))
        }
    } else if value_type.accepts(value) {
        Ok(())
    } else {
        Err(format!("value must have type '{}'", value_type.as_str()))
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

#[derive(Debug, Clone)]
pub struct OptionDeclaration {
    name: String,
    kind: OptionKind,
    short: Option<char>,
    long: Option<String>,
    positional: bool,
    required: bool,
    repeatable: bool,
    help: String,
    values: Vec<String>,
}

impl OptionDeclaration {
    fn validate(raw: RawOption, command: &CommandPath) -> Result<Self, ProtocolError> {
        if !is_option_word(&raw.name) {
            return Err(invalid_option(
                command,
                &raw.name,
                "name must use lowercase ASCII letters, numbers, '-' or '_'",
            ));
        }
        if RESERVED_LONG_OPTIONS.contains(&raw.name.as_str()) {
            return Err(invalid_option(
                command,
                &raw.name,
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
                        &raw.name,
                        "short must be one ASCII letter or number without '-'",
                    )),
                }
            })
            .transpose()?;
        if short.is_some_and(|short| RESERVED_SHORT_OPTIONS.contains(&short)) {
            return Err(invalid_option(
                command,
                &raw.name,
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
                        &raw.name,
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
                &raw.name,
                "long name is reserved by the host",
            ));
        }

        if raw.positional {
            if short.is_some() || long.is_some() {
                return Err(invalid_option(
                    command,
                    &raw.name,
                    "positional options cannot declare short or long names",
                ));
            }
            if raw.kind == OptionKind::Flag {
                return Err(invalid_option(
                    command,
                    &raw.name,
                    "positional options cannot be flags",
                ));
            }
        } else if short.is_none() && long.is_none() {
            return Err(invalid_option(
                command,
                &raw.name,
                "named options require a short or long name",
            ));
        }

        if raw.kind == OptionKind::Flag && !raw.values.is_empty() {
            return Err(invalid_option(
                command,
                &raw.name,
                "flags cannot declare values",
            ));
        }
        for value in &raw.values {
            if !raw.kind.validate_value(value) {
                return Err(invalid_option(
                    command,
                    &raw.name,
                    &format!(
                        "allowed value '{value}' is not a valid {}",
                        raw.kind.type_name()
                    ),
                ));
            }
        }

        Ok(Self {
            name: raw.name,
            kind: raw.kind,
            short,
            long,
            positional: raw.positional,
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
        self.positional
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
    commands: Vec<RawCommand>,
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
    options: Vec<RawOption>,
    workflow: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflow {
    #[serde(default)]
    inputs: Vec<RawWorkflowInput>,
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
    name: String,
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
    name: String,
    kind: OptionKind,
    short: Option<String>,
    long: Option<String>,
    #[serde(default)]
    positional: bool,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    repeatable: bool,
    #[serde(default)]
    help: String,
    #[serde(default)]
    values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOutputShape {
    Empty,
    Lines,
    Rows,
    Detail,
    Message,
    Values,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SemanticOutput {
    shape: SemanticOutputShape,
    value: Value,
    #[serde(default)]
    columns: Vec<String>,
}

impl SemanticOutput {
    pub fn new(
        shape: SemanticOutputShape,
        value: Value,
        columns: Vec<String>,
    ) -> Result<Self, ProtocolError> {
        let output = Self {
            shape,
            value,
            columns,
        };
        output.validate()?;
        Ok(output)
    }

    pub fn shape(&self) -> SemanticOutputShape {
        self.shape
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn into_parts(self) -> (SemanticOutputShape, Value, Vec<String>) {
        (self.shape, self.value, self.columns)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        let valid_value = match self.shape {
            SemanticOutputShape::Empty => {
                self.value.is_null() || self.value.as_array().is_some_and(Vec::is_empty)
            }
            SemanticOutputShape::Lines => self
                .value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            SemanticOutputShape::Rows => self
                .value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_object)),
            SemanticOutputShape::Detail => self.value.is_object(),
            SemanticOutputShape::Message => !self.value.is_array() && !self.value.is_object(),
            SemanticOutputShape::Values => self.value.is_array(),
        };
        if !valid_value {
            return Err(ProtocolError::InvalidOutput(format!(
                "shape '{:?}' has an incompatible JSON value",
                self.shape
            )));
        }
        if self.columns.iter().any(|column| column.trim().is_empty()) {
            return Err(ProtocolError::InvalidOutput(
                "columns must not be empty".to_string(),
            ));
        }
        let mut unique = HashSet::new();
        if self.columns.iter().any(|column| !unique.insert(column)) {
            return Err(ProtocolError::InvalidOutput(
                "columns must be unique".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExtensionFailure {
    code: String,
    message: String,
    #[serde(default)]
    details: Value,
}

impl ExtensionFailure {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        DiagnosticCode::new(self.code.clone())?;
        if self.message.trim().is_empty() {
            return Err(ProtocolError::InvalidOutput(
                "error message must not be empty".to_string(),
            ));
        }
        Ok(())
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn details(&self) -> &Value {
        &self.details
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ExtensionResponse {
    Ok {
        protocol: String,
        output: SemanticOutput,
        #[serde(default)]
        warnings: Vec<String>,
    },
    Error {
        protocol: String,
        error: ExtensionFailure,
        #[serde(default)]
        warnings: Vec<String>,
    },
}

impl ExtensionResponse {
    pub fn parse(input: &str) -> Result<Self, ProtocolError> {
        let response: Self = serde_json::from_str(input)?;
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        let protocol = match self {
            Self::Ok { protocol, .. } | Self::Error { protocol, .. } => protocol,
        };
        ProtocolVersion::new(protocol.clone())?;
        match self {
            Self::Ok { output, .. } => output.validate(),
            Self::Error { error, .. } => error.validate(),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }
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
mod workflow_language_tests {
    use semver::Version;
    use serde_json::json;

    use super::{
        CommandImplementation, ExtensionManifest, ExtensionPackKind, ExtensionResponse,
        ProtocolError, SemanticOutput, SemanticOutputShape, WorkflowStep, PROTOCOL_V1,
    };

    const EXECUTABLE: &str = r#"
schema_version = 1
kind = "executable"
name = "site-inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/site-inventory"

[config.site]
type = "string"
required = true

[[commands]]
path = ["host", "show"]
arguments = ["host", "show"]

[[commands.options]]
name = "identifier"
kind = "string"
positional = true
"#;

    const PORTABLE_BODY: &str = r#"
[config.hosts_class]
type = "string"
default = "Hosts"

[workflows.item]
result = "[.input.item]"

[[workflows.item.inputs]]
name = "item"
type = "json"
required = true

[workflows.item.output]
shape = "values"
type = "json"

[[workflows.item.steps]]
id = "keep"
kind = "let"
expr = ".input.item"

[workflows.snapshot]
result = "{ hosts: .steps.hosts, selected: .steps.selected, one: .steps.one, many: .steps.many }"

[[workflows.snapshot.inputs]]
name = "enabled"
type = "boolean"
default = true

[[workflows.snapshot.inputs]]
name = "items"
type = "json"
default = [1, 2]

[workflows.snapshot.output]
shape = "detail"
type = "json"

[[workflows.snapshot.steps]]
id = "hosts"
kind = "run"
run = ["object", "list"]
when = ".input.enabled"

[workflows.snapshot.steps.with]
class = { config = "hosts_class" }
all = true

[[workflows.snapshot.steps]]
id = "selected"
kind = "let"
expr = ".steps.hosts"

[[workflows.snapshot.steps]]
id = "valid"
kind = "assert"
condition = ".input.enabled == true"
message = "snapshot must be enabled"

[[workflows.snapshot.steps]]
id = "one"
kind = "call"
call = "item"
when = ".input.enabled"
with = { item = "one" }

[[workflows.snapshot.steps]]
id = "many"
kind = "for_each"
items = { input = "items" }
as = "item"
call = "item"
max_items = 10
when = ".input.enabled"
"#;

    #[test]
    fn parses_executable_pack_and_typed_config() {
        let manifest = ExtensionManifest::parse(EXECUTABLE).expect("manifest");
        assert_eq!(manifest.kind(), ExtensionPackKind::Executable);
        assert_eq!(manifest.protocol().expect("protocol").as_str(), PROTOCOL_V1);
        assert!(manifest.supports_cli(&Version::new(0, 0, 9)));
        assert!(matches!(
            manifest.commands()[0].implementation(),
            CommandImplementation::Executable { .. }
        ));
        assert_eq!(
            manifest
                .resolve_config(&json!({"site": "oslo"}))
                .expect("config")["site"],
            "oslo"
        );
        assert!(manifest
            .resolve_config(&json!({"site": "oslo", "extra": true}))
            .expect_err("unknown key")
            .to_string()
            .contains("unknown configuration key"));
    }

    #[test]
    fn parses_every_tagged_step_and_named_workflow() {
        let manifest =
            ExtensionManifest::parse(&portable_manifest(PORTABLE_BODY)).expect("portable manifest");
        assert_eq!(manifest.kind(), ExtensionPackKind::Portable);
        assert!(manifest.protocol().is_none());
        let name = manifest.commands()[0].workflow().expect("workflow name");
        let workflow = manifest.workflow(name).expect("workflow");
        assert_eq!(workflow.steps().len(), 5);
        assert!(matches!(workflow.steps()[0], WorkflowStep::Run(_)));
        assert!(matches!(workflow.steps()[1], WorkflowStep::Let(_)));
        assert!(matches!(workflow.steps()[2], WorkflowStep::Assert(_)));
        assert!(matches!(workflow.steps()[3], WorkflowStep::Call(_)));
        assert!(matches!(workflow.steps()[4], WorkflowStep::ForEach(_)));
    }

    #[test]
    fn rejects_forward_references_cross_pack_calls_and_unbounded_iteration() {
        let forward = portable_manifest(
            r#"
[workflows.snapshot]
result = ".steps.first"
[workflows.snapshot.output]
shape = "detail"
type = "json"
[[workflows.snapshot.steps]]
id = "first"
kind = "run"
run = ["object", "show"]
with = { id = { step = "later" } }
[[workflows.snapshot.steps]]
id = "later"
kind = "run"
run = ["object", "list"]
"#,
        );
        assert!(ExtensionManifest::parse(&forward)
            .expect_err("forward reference")
            .to_string()
            .contains("earlier step"));

        let cross_pack = PORTABLE_BODY.replace("call = \"item\"", "call = \"other.pack\"");
        assert!(ExtensionManifest::parse(&portable_manifest(&cross_pack))
            .expect_err("cross-pack call")
            .to_string()
            .contains("snake_case"));

        let unbounded = PORTABLE_BODY.replace("max_items = 10\n", "");
        assert!(ExtensionManifest::parse(&portable_manifest(&unbounded))
            .expect_err("missing max_items")
            .to_string()
            .contains("max_items"));
    }

    #[test]
    fn rejects_pack_kind_mixing_and_extension_run_steps() {
        assert!(ExtensionManifest::parse(
            &EXECUTABLE.replace("kind = \"executable\"", "kind = \"portable\"")
        )
        .expect_err("mixed kind")
        .to_string()
        .contains("cannot declare protocol or executable"));

        let recursion = PORTABLE_BODY.replace(
            "run = [\"object\", \"list\"]",
            "run = [\"extension\", \"inventory\", \"snapshot\"]",
        );
        assert!(ExtensionManifest::parse(&portable_manifest(&recursion))
            .expect_err("extension recursion")
            .to_string()
            .contains("cannot invoke extension commands"));
    }

    #[test]
    fn rejects_contradictory_defaults_and_lossy_flag_inputs() {
        let required_config = PORTABLE_BODY.replace(
            "type = \"string\"\ndefault = \"Hosts\"",
            "type = \"string\"\nrequired = true\ndefault = \"Hosts\"",
        );
        assert!(
            ExtensionManifest::parse(&portable_manifest(&required_config))
                .expect_err("required config default")
                .to_string()
                .contains("required config cannot also declare a default")
        );

        let required_input = PORTABLE_BODY.replace(
            "type = \"boolean\"\ndefault = true",
            "type = \"boolean\"\nrequired = true\ndefault = true",
        );
        assert!(
            ExtensionManifest::parse(&portable_manifest(&required_input))
                .expect_err("required input default")
                .to_string()
                .contains("cannot be required and also declare a default")
        );

        let true_flag = portable_manifest(PORTABLE_BODY).replace(
            "name = \"enabled\"\nkind = \"boolean\"",
            "name = \"enabled\"\nkind = \"flag\"",
        );
        assert!(ExtensionManifest::parse(&true_flag)
            .expect_err("true default flag")
            .to_string()
            .contains("defaults to true"));

        let repeatable_flag = portable_manifest(&PORTABLE_BODY.replace(
            "type = \"boolean\"\ndefault = true",
            "type = \"boolean\"\nrepeatable = true",
        ))
        .replace(
            "name = \"enabled\"\nkind = \"boolean\"",
            "name = \"enabled\"\nkind = \"flag\"\nrepeatable = true",
        );
        assert!(ExtensionManifest::parse(&repeatable_flag)
            .expect_err("repeatable flag")
            .to_string()
            .contains("repeatable workflow input"));
    }

    #[test]
    fn validates_semantic_shapes_and_protocol_responses() {
        SemanticOutput::new(
            SemanticOutputShape::Rows,
            json!([{"name": "host-1"}]),
            vec!["name".to_string()],
        )
        .expect("rows");
        assert!(SemanticOutput::new(
            SemanticOutputShape::Rows,
            json!(["not-an-object"]),
            Vec::new(),
        )
        .is_err());

        assert!(ExtensionResponse::parse(
            r#"{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"message","value":"done","columns":[]}}"#,
        )
        .expect("response")
        .is_ok());
    }

    #[test]
    fn reserves_management_names() {
        assert!(matches!(
            ExtensionManifest::parse(&EXECUTABLE.replace("site-inventory", "validate")),
            Err(ProtocolError::ReservedPackName(_))
        ));
    }

    fn portable_manifest(body: &str) -> String {
        format!(
            r#"schema_version = 1
kind = "portable"
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"

{body}

[[commands]]
path = ["snapshot"]
workflow = "snapshot"

[[commands.options]]
name = "enabled"
kind = "boolean"
long = "enabled"

[[commands.options]]
name = "items"
kind = "json"
long = "items"
"#
        )
    }
}
