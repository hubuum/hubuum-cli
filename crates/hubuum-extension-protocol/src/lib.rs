use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use jsonc_parser::{parse_to_serde_value, ParseOptions};
use semver::{Version, VersionReq};
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
};

const RESERVED_PACK_NAMES: &[&str] = &[
    "disable", "doctor", "enable", "explain", "install", "list", "reload", "remove", "show",
    "upgrade", "validate",
];
const RESERVED_LONG_OPTIONS: &[&str] = &["help", "json", "output", "table-headers"];
const RESERVED_SHORT_OPTIONS: &[char] = &['h', 'j', 'o'];
const RESERVED_OPTION_KEYS: &[&str] = &["h", "help", "j", "json", "o", "output", "table-headers"];

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
        let raw: RawManifest = parse_to_serde_value(input, &MANIFEST_PARSE_OPTIONS)
            .map_err(|error| ProtocolError::InvalidManifestJsonc(error.to_string()))?;
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

        let config = normalize_config(raw.config)?;
        let workflows = normalize_workflows(raw.workflows, &config)?;
        validate_workflow_calls(&workflows, &config)?;
        let commands = normalize_commands(raw.commands, raw.kind, &workflows)?;

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

fn normalize_config(
    raw: BTreeMap<String, RawValueDeclaration>,
) -> Result<BTreeMap<String, ConfigDeclaration>, ProtocolError> {
    raw.into_iter()
        .map(|(key, declaration)| {
            ConfigDeclaration::validate(key.clone(), declaration)
                .map(|declaration| (key, declaration))
        })
        .collect()
}

fn normalize_workflows(
    raw: BTreeMap<String, RawWorkflow>,
    config: &BTreeMap<String, ConfigDeclaration>,
) -> Result<BTreeMap<WorkflowName, WorkflowDeclaration>, ProtocolError> {
    raw.into_iter()
        .map(|(name, workflow)| {
            let workflow_name = WorkflowName::new(name.clone())
                .map_err(|message| invalid_named_workflow(&name, &message))?;
            WorkflowDeclaration::validate(&workflow_name, workflow, config)
                .map(|workflow| (workflow_name, workflow))
        })
        .collect()
}

fn normalize_commands(
    raw: BTreeMap<String, RawCommand>,
    kind: ExtensionPackKind,
    workflows: &BTreeMap<WorkflowName, WorkflowDeclaration>,
) -> Result<Vec<CommandDeclaration>, ProtocolError> {
    if raw.is_empty() {
        return Err(ProtocolError::InvalidCommandPath(
            "at least one command is required".to_string(),
        ));
    }

    let mut paths = HashSet::new();
    let mut commands: Vec<CommandDeclaration> = Vec::with_capacity(raw.len());
    for (name, command) in raw {
        let command = CommandDeclaration::validate(
            CommandDeclarationName::new(name)?,
            command,
            kind,
            workflows,
        )?;
        if !paths.insert(command.path.clone()) {
            return Err(ProtocolError::DuplicateCommandPath(command.path.display()));
        }
        for existing in &commands {
            if let Some((prefix, command)) = command_path_conflict(existing.path(), command.path())
            {
                return Err(ProtocolError::CommandPathPrefix { prefix, command });
            }
        }
        commands.push(command);
    }
    Ok(commands)
}

fn command_path_conflict(left: &CommandPath, right: &CommandPath) -> Option<(String, String)> {
    if left.segments().len() < right.segments().len()
        && right.segments().starts_with(left.segments())
    {
        Some((left.display(), right.display()))
    } else if right.segments().len() < left.segments().len()
        && left.segments().starts_with(right.segments())
    {
        Some((right.display(), left.display()))
    } else {
        None
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
    declaration_name: CommandDeclarationName,
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
        declaration_name: CommandDeclarationName,
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

        let options = normalize_command_options(raw.options, &path)?;

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
            declaration_name,
            path,
            about: nonempty(raw.about),
            long_about: nonempty(raw.long_about),
            examples: raw.examples,
            options,
            implementation,
        })
    }

    pub fn declaration_name(&self) -> &CommandDeclarationName {
        &self.declaration_name
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

fn normalize_command_options(
    raw: BTreeMap<String, RawOption>,
    command: &CommandPath,
) -> Result<Vec<OptionDeclaration>, ProtocolError> {
    let mut options = raw
        .into_iter()
        .map(|(name, option)| OptionDeclaration::validate(name, option, command))
        .collect::<Result<Vec<_>, _>>()?;
    options.sort_by(|left, right| {
        left.position()
            .map_or((1, usize::MAX), |position| (0, position.get()))
            .cmp(
                &right
                    .position()
                    .map_or((1, usize::MAX), |position| (0, position.get())),
            )
            .then_with(|| left.name().cmp(right.name()))
    });

    let positional_count = options.iter().filter(|option| option.positional()).count();
    let mut positions = HashSet::new();
    let mut aliases = HashSet::new();
    let mut optional_positional_seen = false;
    for option in &options {
        if let Some(position) = option.position() {
            if !positions.insert(position.get()) {
                return Err(invalid_option(
                    command,
                    option.name(),
                    "positional option positions must be unique",
                ));
            }
            if optional_positional_seen && option.required() {
                return Err(invalid_option(
                    command,
                    option.name(),
                    "required positionals cannot follow optional positionals",
                ));
            }
            optional_positional_seen |= !option.required();
            if option.repeatable() && position.get() != positional_count {
                return Err(invalid_option(
                    command,
                    option.name(),
                    "only the final positional may be repeatable",
                ));
            }
        }
        if let Some(short) = option.short() {
            validate_unique_option_alias(command, option.name(), short.to_string(), &mut aliases)?;
        }
        if let Some(long) = option.long() {
            validate_unique_option_alias(command, option.name(), long.to_string(), &mut aliases)?;
        }
    }
    if (1..=positional_count).any(|position| !positions.contains(&position)) {
        return Err(ProtocolError::InvalidCommandPath(format!(
            "command '{}' positional option positions must be contiguous from 1",
            command.display()
        )));
    }
    Ok(options)
}

fn validate_unique_option_alias(
    command: &CommandPath,
    option: &str,
    alias: String,
    aliases: &mut HashSet<String>,
) -> Result<(), ProtocolError> {
    if aliases.insert(alias) {
        Ok(())
    } else {
        Err(invalid_option(
            command,
            option,
            "option aliases must be unique after removing dashes",
        ))
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
    fn validate(
        workflow: &WorkflowName,
        name: String,
        raw: RawWorkflowInput,
    ) -> Result<Self, ProtocolError> {
        if !is_option_word(&name) {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                &format!(
                    "input '{}' must use lowercase ASCII letters, numbers, '-' or '_'",
                    name
                ),
            ));
        }
        if raw.required && raw.default.is_some() {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                &format!(
                    "input '{}' cannot be required and also declare a default",
                    name
                ),
            ));
        }
        validate_declared_default(&raw.default, raw.value_type, raw.repeatable, |message| {
            invalid_named_workflow(workflow.as_str(), &message)
        })?;
        Ok(Self {
            name,
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
        let inputs = normalize_workflow_inputs(name, raw.inputs)?;
        let capabilities = normalize_workflow_capabilities(name, raw.capabilities)?;
        let steps = normalize_workflow_steps(name, raw.steps, &inputs, config)?;
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

fn normalize_workflow_inputs(
    workflow: &WorkflowName,
    raw: BTreeMap<String, RawWorkflowInput>,
) -> Result<Vec<WorkflowInputDeclaration>, ProtocolError> {
    raw.into_iter()
        .map(|(name, input)| WorkflowInputDeclaration::validate(workflow, name, input))
        .collect()
}

fn normalize_workflow_capabilities(
    workflow: &WorkflowName,
    raw: Vec<String>,
) -> Result<Vec<WorkflowCapability>, ProtocolError> {
    let mut capabilities = Vec::with_capacity(raw.len());
    for capability in raw {
        let capability = WorkflowCapability::new(&capability)
            .map_err(|message| invalid_named_workflow(workflow.as_str(), &message))?;
        if capabilities.contains(&capability) {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                &format!("duplicate workflow capability '{}'", capability.as_str()),
            ));
        }
        capabilities.push(capability);
    }
    Ok(capabilities)
}

fn normalize_workflow_steps(
    workflow: &WorkflowName,
    raw: Vec<RawWorkflowStep>,
    inputs: &[WorkflowInputDeclaration],
    config: &BTreeMap<String, ConfigDeclaration>,
) -> Result<Vec<WorkflowStep>, ProtocolError> {
    let mut prior_ids = HashSet::new();
    let mut steps = Vec::with_capacity(raw.len());
    for raw_step in raw {
        let step = WorkflowStep::validate(workflow, raw_step, inputs, config, &prior_ids)?;
        if !prior_ids.insert(step.id().clone()) {
            return Err(invalid_named_workflow(
                workflow.as_str(),
                &format!("duplicate step id '{}'", step.id().as_str()),
            ));
        }
        steps.push(step);
    }
    Ok(steps)
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
        let id = validate_step_id(workflow, raw.id().to_string())?;
        match raw {
            RawWorkflowStep::Run {
                run,
                bindings,
                when,
                ..
            } => {
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
            RawWorkflowStep::Let { expr, .. } => Ok(Self::Let(WorkflowLetStep {
                id,
                expr: validate_expression(workflow, "let expression", expr)?,
            })),
            RawWorkflowStep::Assert {
                condition, message, ..
            } => {
                if message.trim().is_empty() || message.contains('\0') {
                    return Err(invalid(format!(
                        "assert step '{}' message must be non-empty and contain no NUL",
                        id.as_str()
                    )));
                }
                Ok(Self::Assert(WorkflowAssertStep {
                    id,
                    condition: validate_expression(workflow, "assert condition", condition)?,
                    message,
                }))
            }
            RawWorkflowStep::Call {
                call,
                bindings,
                when,
                ..
            } => Ok(Self::Call(WorkflowCallStep {
                id,
                call: WorkflowName::new(call.clone())
                    .map_err(|message| invalid(format!("call target {message}")))?,
                bindings: validate_bindings(workflow, bindings, inputs, config, prior_step_ids)?,
                when: validate_optional_expression(workflow, "when", when)?,
            })),
            RawWorkflowStep::ForEach {
                items,
                item_name,
                call,
                bindings,
                max_items,
                when,
                ..
            } => {
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
    config: &BTreeMap<String, ConfigDeclaration>,
) -> Result<(), ProtocolError> {
    for workflow in workflows.values() {
        for step in workflow.steps() {
            let (target, item, bindings) = match step {
                WorkflowStep::Call(step) => (step.call(), None, step.bindings()),
                WorkflowStep::ForEach(step) => (
                    step.call(),
                    Some((step.item_name(), step.items())),
                    step.bindings(),
                ),
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
            for (binding_name, binding) in bindings {
                let target_input = target_workflow
                    .inputs()
                    .iter()
                    .find(|input| input.name() == binding_name.as_str())
                    .ok_or_else(|| {
                        invalid_named_workflow(
                            workflow.name().as_str(),
                            &format!(
                                "step '{}' binds unknown input '{}' on workflow '{}'",
                                step.id().as_str(),
                                binding_name.as_str(),
                                target.as_str()
                            ),
                        )
                    })?;
                validate_call_binding_type(
                    workflow,
                    step.id(),
                    binding_name,
                    binding,
                    target_input,
                    config,
                )?;
            }
            let item_name = item.map(|(name, _)| name);
            if let Some((item_name, items)) = item {
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
                let target_input = target_workflow
                    .inputs()
                    .iter()
                    .find(|input| input.name() == item_name)
                    .ok_or_else(|| {
                        invalid_named_workflow(
                            workflow.name().as_str(),
                            &format!(
                                "for_each step '{}' as value '{}' is not an input of workflow '{}'",
                                step.id().as_str(),
                                item_name,
                                target.as_str()
                            ),
                        )
                    })?;
                validate_for_each_item_type(workflow, step.id(), items, target_input, config)?;
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

fn validate_call_binding_type(
    workflow: &WorkflowDeclaration,
    step: &WorkflowStepId,
    binding_name: &WorkflowBindingName,
    binding: &WorkflowBinding,
    target: &WorkflowInputDeclaration,
    config: &BTreeMap<String, ConfigDeclaration>,
) -> Result<(), ProtocolError> {
    let result = match binding {
        WorkflowBinding::Literal(value) => validate_workflow_input_value(value, target),
        WorkflowBinding::Input { name } => {
            match workflow.inputs().iter().find(|input| input.name() == name) {
                Some(source) => validate_declared_types(
                    source.value_type(),
                    source.repeatable(),
                    target,
                    "workflow input",
                ),
                None => Err(format!("source input '{name}' is not declared")),
            }
        }
        WorkflowBinding::Config { key } => match config.get(key) {
            Some(source) => validate_declared_types(
                source.value_type(),
                source.repeatable(),
                target,
                "configuration value",
            ),
            None => Err(format!("configuration value '{key}' is not declared")),
        },
        WorkflowBinding::Step { .. } => Ok(()),
    };
    result.map_err(|message| {
        invalid_named_workflow(
            workflow.name().as_str(),
            &format!(
                "step '{}' binding '{}': {message}",
                step.as_str(),
                binding_name.as_str()
            ),
        )
    })
}

fn validate_declared_types(
    source_type: WorkflowValueType,
    source_repeatable: bool,
    target: &WorkflowInputDeclaration,
    source_label: &str,
) -> Result<(), String> {
    if source_repeatable != target.repeatable() {
        return Err(format!(
            "{source_label} repeatable={source_repeatable} does not match target input '{}' repeatable={}",
            target.name(),
            target.repeatable()
        ));
    }
    if !target.value_type().accepts_type(source_type) {
        return Err(format!(
            "{source_label} type '{}' is incompatible with target input '{}' type '{}'",
            source_type.as_str(),
            target.name(),
            target.value_type().as_str()
        ));
    }
    Ok(())
}

fn validate_workflow_input_value(
    value: &Value,
    target: &WorkflowInputDeclaration,
) -> Result<(), String> {
    let valid = if target.repeatable() {
        value
            .as_array()
            .is_some_and(|items| items.iter().all(|item| target.value_type().accepts(item)))
    } else {
        target.value_type().accepts(value)
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "literal does not satisfy target input '{}' type '{}'{}",
            target.name(),
            target.value_type().as_str(),
            if target.repeatable() { "[]" } else { "" }
        ))
    }
}

fn validate_for_each_item_type(
    workflow: &WorkflowDeclaration,
    step: &WorkflowStepId,
    binding: &WorkflowBinding,
    target: &WorkflowInputDeclaration,
    config: &BTreeMap<String, ConfigDeclaration>,
) -> Result<(), ProtocolError> {
    let invalid = |message: String| {
        invalid_named_workflow(
            workflow.name().as_str(),
            &format!("for_each step '{}' items: {message}", step.as_str()),
        )
    };
    match binding {
        WorkflowBinding::Literal(value) => {
            let items = value
                .as_array()
                .ok_or_else(|| invalid("literal must be an array".to_string()))?;
            for item in items {
                validate_workflow_input_value(item, target).map_err(&invalid)?;
            }
            Ok(())
        }
        WorkflowBinding::Input { name } => {
            let source = workflow
                .inputs()
                .iter()
                .find(|input| input.name() == name)
                .ok_or_else(|| invalid(format!("workflow input '{name}' is not declared")))?;
            validate_iteration_source(
                source.value_type(),
                source.repeatable(),
                target,
                &format!("workflow input '{name}'"),
            )
            .map_err(invalid)
        }
        WorkflowBinding::Config { key } => {
            let source = config
                .get(key)
                .ok_or_else(|| invalid(format!("configuration value '{key}' is not declared")))?;
            validate_iteration_source(
                source.value_type(),
                source.repeatable(),
                target,
                &format!("configuration value '{key}'"),
            )
            .map_err(invalid)
        }
        WorkflowBinding::Step { .. } => Ok(()),
    }
}

fn validate_iteration_source(
    source_type: WorkflowValueType,
    source_repeatable: bool,
    target: &WorkflowInputDeclaration,
    source_label: &str,
) -> Result<(), String> {
    if source_repeatable {
        validate_declared_types(source_type, false, target, "iteration item")
    } else if source_type == WorkflowValueType::Json {
        Ok(())
    } else {
        Err(format!("{source_label} is not repeatable or JSON"))
    }
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

    const EXECUTABLE: &str = r#"{
  // JSONC comments and trailing commas are supported.
  "schema_version": 1,
  "kind": "executable",
  "name": "site-inventory",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "protocol": "hubuum-cli.extension/v1",
  "executable": "bin/site-inventory",
  "config": {
    "site": {
      "type": "string",
      "required": true,
    },
  },
  "commands": {
    "host_show": {
      "path": ["host", "show"],
      "arguments": ["host", "show"],
      "options": {
        "identifier": {
          "kind": "string",
          "position": 1,
        },
      },
    },
  },
}"#;

    const PORTABLE_BODY: &str = r#"
  "config": {
    "hosts_class": {
      "type": "string",
      "default": "Hosts"
    }
  },
  "workflows": {
    "item": {
      "inputs": {
        "item": {
          "type": "json",
          "required": true
        }
      },
      "output": {
        "shape": "values",
        "type": "json"
      },
      "steps": [
        {
          "id": "keep",
          "kind": "let",
          "expr": ".input.item"
        }
      ],
      "result": "[.input.item]"
    },
    "snapshot": {
      "inputs": {
        "enabled": {
          "type": "boolean",
          "default": true
        },
        "items": {
          "type": "json",
          "default": [1, 2]
        }
      },
      "output": {
        "shape": "detail",
        "type": "json"
      },
      "steps": [
        {
          "id": "hosts",
          "kind": "run",
          "run": ["object", "list"],
          "when": ".input.enabled",
          "with": {
            "class": { "config": "hosts_class" },
            "all": true
          }
        },
        {
          "id": "selected",
          "kind": "let",
          "expr": ".steps.hosts"
        },
        {
          "id": "valid",
          "kind": "assert",
          "condition": ".input.enabled == true",
          "message": "snapshot must be enabled"
        },
        {
          "id": "one",
          "kind": "call",
          "call": "item",
          "when": ".input.enabled",
          "with": { "item": "one" }
        },
        {
          "id": "many",
          "kind": "for_each",
          "items": { "input": "items" },
          "as": "item",
          "call": "item",
          "max_items": 10,
          "when": ".input.enabled"
        }
      ],
      "result": "{ hosts: .steps.hosts, selected: .steps.selected, one: .steps.one, many: .steps.many }"
    }
  }
"#;

    #[test]
    fn parses_executable_pack_and_typed_config() {
        let manifest = ExtensionManifest::parse(EXECUTABLE).expect("manifest");
        assert_eq!(manifest.kind(), ExtensionPackKind::Executable);
        assert_eq!(manifest.protocol().expect("protocol").as_str(), PROTOCOL_V1);
        assert!(manifest.supports_cli(&Version::new(0, 0, 9)));
        assert_eq!(
            manifest.commands()[0].declaration_name().as_str(),
            "host_show"
        );
        assert_eq!(
            manifest.commands()[0].options()[0]
                .position()
                .map(|position| position.get()),
            Some(1)
        );
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
  "workflows": {
    "snapshot": {
      "output": { "shape": "detail", "type": "json" },
      "steps": [
        {
          "id": "first",
          "kind": "run",
          "run": ["object", "show"],
          "with": { "id": { "step": "later" } }
        },
        {
          "id": "later",
          "kind": "run",
          "run": ["object", "list"]
        }
      ],
      "result": ".steps.first"
    }
  }
"#,
        );
        assert!(ExtensionManifest::parse(&forward)
            .expect_err("forward reference")
            .to_string()
            .contains("earlier step"));

        let cross_pack = PORTABLE_BODY.replace("\"call\": \"item\"", "\"call\": \"other.pack\"");
        assert!(ExtensionManifest::parse(&portable_manifest(&cross_pack))
            .expect_err("cross-pack call")
            .to_string()
            .contains("snake_case"));

        let unbounded = PORTABLE_BODY.replace("          \"max_items\": 10,\n", "");
        assert!(ExtensionManifest::parse(&portable_manifest(&unbounded))
            .expect_err("missing max_items")
            .to_string()
            .contains("max_items"));
    }

    #[test]
    fn rejects_duplicate_steps_and_noncontiguous_positions() {
        let missing_step =
            PORTABLE_BODY.replace("          \"id\": \"many\",", "          \"id\": \"one\",");
        assert!(ExtensionManifest::parse(&portable_manifest(&missing_step))
            .expect_err("duplicate step id")
            .to_string()
            .contains("duplicate step id 'one'"));

        let skipped_position = EXECUTABLE.replace("\"position\": 1", "\"position\": 2");
        assert!(ExtensionManifest::parse(&skipped_position)
            .expect_err("non-contiguous positional order")
            .to_string()
            .contains("positions must be contiguous from 1"));
    }

    #[test]
    fn rejects_incompatible_same_pack_bindings() {
        let invalid_call_literal = PORTABLE_BODY.replace(
            "          \"type\": \"json\",\n          \"required\": true",
            "          \"type\": \"integer\",\n          \"required\": true",
        );
        assert!(
            ExtensionManifest::parse(&portable_manifest(&invalid_call_literal))
                .expect_err("incompatible call literal")
                .to_string()
                .contains("literal does not satisfy target input 'item' type 'integer'")
        );

        let invalid_iteration = PORTABLE_BODY.replace(
            "\"items\": { \"input\": \"items\" }",
            "\"items\": { \"input\": \"enabled\" }",
        );
        assert!(
            ExtensionManifest::parse(&portable_manifest(&invalid_iteration))
                .expect_err("non-iterable input")
                .to_string()
                .contains("workflow input 'enabled' is not repeatable or JSON")
        );
    }

    #[test]
    fn rejects_toml_json5_extensions_and_invalid_declaration_names() {
        let legacy = "schema_version = 1";
        assert!(matches!(
            ExtensionManifest::parse(&legacy),
            Err(ProtocolError::InvalidManifestJsonc(_))
        ));

        let json5_extensions = [
            EXECUTABLE.replace("\"schema_version\"", "schema_version"),
            EXECUTABLE.replace("\"site-inventory\"", "'site-inventory'"),
            EXECUTABLE.replace("\"schema_version\": 1", "\"schema_version\": 0x1"),
            EXECUTABLE.replace("\"schema_version\": 1", "\"schema_version\": +1"),
            EXECUTABLE.replace("\"schema_version\": 1,", "\"schema_version\": 1"),
        ];
        for manifest in json5_extensions {
            assert!(matches!(
                ExtensionManifest::parse(&manifest),
                Err(ProtocolError::InvalidManifestJsonc(_))
            ));
        }

        let invalid_name = EXECUTABLE.replace("\"host_show\"", "\"host-show\"");
        assert!(matches!(
            ExtensionManifest::parse(&invalid_name),
            Err(ProtocolError::InvalidCommandDeclarationName(name)) if name == "host-show"
        ));
    }

    #[test]
    fn rejects_pack_kind_mixing_and_extension_run_steps() {
        assert!(ExtensionManifest::parse(
            &EXECUTABLE.replace("\"kind\": \"executable\"", "\"kind\": \"portable\"")
        )
        .expect_err("mixed kind")
        .to_string()
        .contains("cannot declare protocol or executable"));

        let recursion = PORTABLE_BODY.replace(
            "\"run\": [\"object\", \"list\"]",
            "\"run\": [\"extension\", \"inventory\", \"snapshot\"]",
        );
        assert!(ExtensionManifest::parse(&portable_manifest(&recursion))
            .expect_err("extension recursion")
            .to_string()
            .contains("cannot invoke extension commands"));
    }

    #[test]
    fn rejects_contradictory_defaults_and_lossy_flag_inputs() {
        let required_config = PORTABLE_BODY.replace(
            "      \"type\": \"string\",\n      \"default\": \"Hosts\"",
            "      \"type\": \"string\",\n      \"required\": true,\n      \"default\": \"Hosts\"",
        );
        assert!(
            ExtensionManifest::parse(&portable_manifest(&required_config))
                .expect_err("required config default")
                .to_string()
                .contains("required config cannot also declare a default")
        );

        let required_input = PORTABLE_BODY.replace(
            "          \"type\": \"boolean\",\n          \"default\": true",
            "          \"type\": \"boolean\",\n          \"required\": true,\n          \"default\": true",
        );
        assert!(
            ExtensionManifest::parse(&portable_manifest(&required_input))
                .expect_err("required input default")
                .to_string()
                .contains("cannot be required and also declare a default")
        );

        let true_flag = portable_manifest(PORTABLE_BODY).replace(
            "\"enabled\": { \"kind\": \"boolean\", \"long\": \"enabled\" }",
            "\"enabled\": { \"kind\": \"flag\", \"long\": \"enabled\" }",
        );
        assert!(ExtensionManifest::parse(&true_flag)
            .expect_err("true default flag")
            .to_string()
            .contains("defaults to true"));

        let repeatable_flag = portable_manifest(&PORTABLE_BODY.replace(
            "          \"type\": \"boolean\",\n          \"default\": true",
            "          \"type\": \"boolean\",\n          \"repeatable\": true",
        ))
        .replace(
            "\"enabled\": { \"kind\": \"boolean\", \"long\": \"enabled\" }",
            "\"enabled\": { \"kind\": \"flag\", \"long\": \"enabled\", \"repeatable\": true }",
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
            r#"{{
  "schema_version": 1,
  "kind": "portable",
  "name": "inventory",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
{body},
  "commands": {{
    "snapshot": {{
      "path": ["snapshot"],
      "workflow": "snapshot",
      "options": {{
        "enabled": {{ "kind": "boolean", "long": "enabled" }},
        "items": {{ "kind": "json", "long": "items" }}
      }}
    }}
  }}
}}"#
        )
    }
}
