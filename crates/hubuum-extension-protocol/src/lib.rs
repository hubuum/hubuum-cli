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
    "disable", "doctor", "enable", "install", "list", "reload", "remove", "show", "upgrade",
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
    #[error("command '{0}' requires an executable or a workflow")]
    MissingCommandImplementation(String),
    #[error("command '{command}' has an invalid workflow: {message}")]
    InvalidWorkflow { command: String, message: String },
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

#[derive(Debug, Clone)]
pub struct ExtensionManifest {
    name: PackName,
    version: Version,
    requires_cli: VersionReq,
    protocol: ProtocolVersion,
    executable: Option<ExecutablePath>,
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
        let protocol = ProtocolVersion::new(raw.protocol)?;
        let executable = raw.executable.map(ExecutablePath::new).transpose()?;

        let mut command_paths = HashSet::new();
        let mut commands: Vec<CommandDeclaration> = Vec::with_capacity(raw.commands.len());
        for command in raw.commands {
            let command = CommandDeclaration::try_from(command)?;
            if command.workflow().is_none() && executable.is_none() {
                return Err(ProtocolError::MissingCommandImplementation(
                    command.path().display(),
                ));
            }
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
            name,
            version,
            requires_cli,
            protocol,
            executable,
            commands,
        })
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

    pub fn protocol(&self) -> &ProtocolVersion {
        &self.protocol
    }

    pub fn executable(&self) -> Option<&ExecutablePath> {
        self.executable.as_ref()
    }

    pub fn commands(&self) -> &[CommandDeclaration] {
        &self.commands
    }
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
    Workflow(WorkflowDeclaration),
}

impl CommandDeclaration {
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

    pub fn workflow(&self) -> Option<&WorkflowDeclaration> {
        match &self.implementation {
            CommandImplementation::Executable { .. } => None,
            CommandImplementation::Workflow(workflow) => Some(workflow),
        }
    }

    pub fn implementation(&self) -> &CommandImplementation {
        &self.implementation
    }
}

impl TryFrom<RawCommand> for CommandDeclaration {
    type Error = ProtocolError;

    fn try_from(raw: RawCommand) -> Result<Self, Self::Error> {
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

        let workflow = raw
            .workflow
            .map(|workflow| WorkflowDeclaration::validate(workflow, &path, &options))
            .transpose()?;
        if workflow.is_some() && !raw.arguments.is_empty() {
            return Err(invalid_workflow(
                &path,
                "workflow commands cannot declare executable arguments",
            ));
        }
        if workflow.is_some() && raw.interactive {
            return Err(invalid_workflow(
                &path,
                "workflow commands cannot be interactive",
            ));
        }
        let implementation = workflow.map_or_else(
            || CommandImplementation::Executable {
                arguments: raw.arguments,
                interactive: raw.interactive,
            },
            CommandImplementation::Workflow,
        );
        Ok(Self {
            path,
            about: nonempty(raw.about),
            long_about: nonempty(raw.long_about),
            examples: raw.examples,
            options,
            implementation,
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowDeclaration {
    steps: Vec<WorkflowStep>,
    capabilities: Vec<WorkflowCapability>,
    result: Option<String>,
}

impl WorkflowDeclaration {
    fn validate(
        raw: RawWorkflow,
        command: &CommandPath,
        options: &[OptionDeclaration],
    ) -> Result<Self, ProtocolError> {
        if raw.steps.is_empty() {
            return Err(invalid_workflow(command, "at least one step is required"));
        }

        let mut capabilities = Vec::with_capacity(raw.capabilities.len());
        for capability in raw.capabilities {
            let capability = WorkflowCapability::new(&capability)
                .map_err(|message| invalid_workflow(command, &message))?;
            if capabilities.contains(&capability) {
                return Err(invalid_workflow(
                    command,
                    &format!("duplicate workflow capability '{}'", capability.as_str()),
                ));
            }
            capabilities.push(capability);
        }
        let result = raw
            .result
            .map(|result| {
                if result.trim().is_empty() {
                    Err(invalid_workflow(
                        command,
                        "workflow result expression cannot be empty",
                    ))
                } else {
                    Ok(result)
                }
            })
            .transpose()?;
        let mut ids = HashSet::new();
        let mut steps = Vec::with_capacity(raw.steps.len());
        for raw_step in raw.steps {
            let id = WorkflowStepId::new(raw_step.id).map_err(|message| {
                invalid_workflow(command, &format!("invalid step id: {message}"))
            })?;
            if ids.contains(&id) {
                return Err(invalid_workflow(
                    command,
                    &format!("duplicate step id '{}'", id.as_str()),
                ));
            }
            let run = CommandPath::new(raw_step.run).map_err(|error| {
                invalid_workflow(command, &format!("step '{}': {error}", id.as_str()))
            })?;
            if run
                .segments()
                .first()
                .is_some_and(|part| part == "extension")
            {
                return Err(invalid_workflow(
                    command,
                    &format!("step '{}' cannot invoke extension commands", id.as_str()),
                ));
            }
            let bindings = raw_step
                .with
                .into_iter()
                .map(|(name, binding)| {
                    let name = WorkflowBindingName::new(name).map_err(|message| {
                        invalid_workflow(command, &format!("step '{}': {message}", id.as_str()))
                    })?;
                    let binding = WorkflowBinding::validate(binding, command, options, &ids)?;
                    Ok::<_, ProtocolError>((name, binding))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            steps.push(WorkflowStep {
                id: id.clone(),
                run,
                bindings,
            });
            ids.insert(id);
        }
        Ok(Self {
            steps,
            capabilities,
            result,
        })
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

    pub fn result(&self) -> Option<&str> {
        self.result.as_deref()
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
pub struct WorkflowStep {
    id: WorkflowStepId,
    run: CommandPath,
    bindings: BTreeMap<WorkflowBindingName, WorkflowBinding>,
}

impl WorkflowStep {
    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }

    pub fn run(&self) -> &CommandPath {
        &self.run
    }

    pub fn bindings(&self) -> &BTreeMap<WorkflowBindingName, WorkflowBinding> {
        &self.bindings
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
        default: Option<Value>,
    },
    Step {
        step: WorkflowStepId,
        select: Option<String>,
    },
}

impl WorkflowBinding {
    fn validate(
        raw: RawWorkflowBinding,
        command: &CommandPath,
        options: &[OptionDeclaration],
        prior_step_ids: &HashSet<WorkflowStepId>,
    ) -> Result<Self, ProtocolError> {
        let raw = match raw {
            RawWorkflowBinding::Literal(value) => {
                if value.is_object() {
                    return Err(invalid_workflow(
                        command,
                        "workflow binding tables must declare exactly one of input, config, or step",
                    ));
                }
                if workflow_value_contains_nul(&value) {
                    return Err(invalid_workflow(
                        command,
                        "workflow binding literals cannot contain NUL",
                    ));
                }
                return Ok(Self::Literal(value));
            }
            RawWorkflowBinding::Source(raw) => raw,
        };
        let sources = [
            raw.input.is_some(),
            raw.config.is_some(),
            raw.step.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        if sources != 1 {
            return Err(invalid_workflow(
                command,
                "each workflow binding source must declare exactly one of input, config, or step",
            ));
        }
        if raw.default.is_some() && raw.config.is_none() {
            return Err(invalid_workflow(
                command,
                "a workflow binding default is only valid with config",
            ));
        }
        if raw
            .default
            .as_ref()
            .is_some_and(workflow_value_contains_nul)
        {
            return Err(invalid_workflow(
                command,
                "workflow binding defaults cannot contain NUL",
            ));
        }
        if raw.select.is_some() && raw.step.is_none() {
            return Err(invalid_workflow(
                command,
                "a workflow binding select expression is only valid with step",
            ));
        }
        if raw
            .select
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(invalid_workflow(
                command,
                "a workflow binding select expression cannot be empty",
            ));
        }

        if let Some(input) = raw.input {
            if !options.iter().any(|candidate| candidate.name() == input) {
                return Err(invalid_workflow(
                    command,
                    &format!("workflow binding references unknown input '{input}'"),
                ));
            }
            Ok(Self::Input { name: input })
        } else if let Some(config) = raw.config {
            if !is_option_word(&config) {
                return Err(invalid_workflow(
                    command,
                    &format!(
                        "config key '{config}' must use lowercase ASCII letters, numbers, '-' or '_'"
                    ),
                ));
            }
            Ok(Self::Config {
                key: config,
                default: raw.default,
            })
        } else {
            let step = WorkflowStepId::new(raw.step.expect("one binding source was present"))
                .map_err(|message| {
                    invalid_workflow(command, &format!("invalid step reference: {message}"))
                })?;
            if !prior_step_ids.contains(&step) {
                return Err(invalid_workflow(
                    command,
                    &format!(
                        "step output reference '{}' must name an earlier step",
                        step.as_str()
                    ),
                ));
            }
            Ok(Self::Step {
                step,
                select: raw.select,
            })
        }
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
}

impl OptionKind {
    pub fn validate_value(self, value: &str) -> bool {
        match self {
            Self::String => true,
            Self::Integer => value.parse::<i64>().is_ok(),
            Self::Number => value.parse::<f64>().is_ok(),
            Self::Boolean => value.parse::<bool>().is_ok(),
            Self::Flag => value.is_empty(),
        }
    }

    pub fn type_name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Flag => "flag",
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
    name: String,
    version: String,
    requires_cli: String,
    protocol: String,
    executable: Option<String>,
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
    workflow: Option<RawWorkflow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflow {
    #[serde(default)]
    steps: Vec<RawWorkflowStep>,
    #[serde(default)]
    capabilities: Vec<String>,
    result: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflowStep {
    id: String,
    run: Vec<String>,
    #[serde(default)]
    with: BTreeMap<String, RawWorkflowBinding>,
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
    default: Option<Value>,
    select: Option<String>,
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
mod tests {
    use semver::Version;
    use serde_json::json;

    use super::{
        CommandImplementation, ExtensionManifest, ExtensionResponse, ProtocolError, SemanticOutput,
        SemanticOutputShape, WorkflowBinding,
    };

    const MANIFEST: &str = r#"
schema_version = 1
name = "site-inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/site-inventory"

[[commands]]
path = ["host", "show"]
arguments = ["host", "show"]
about = "Show a Host"

[[commands.options]]
name = "identifier"
kind = "string"
positional = true

[[commands.options]]
name = "target-type"
kind = "string"
long = "target-type"
values = ["auto", "host", "jack", "room"]

[[commands.options]]
name = "verbose"
kind = "flag"
short = "v"
long = "verbose"
"#;

    #[test]
    fn parses_and_validates_a_manifest() {
        let manifest = ExtensionManifest::parse(MANIFEST).expect("manifest should parse");
        assert_eq!(manifest.name().as_str(), "site-inventory");
        assert!(manifest.supports_cli(&Version::new(0, 0, 9)));
        assert_eq!(manifest.commands()[0].path().display(), "host show");
        assert_eq!(manifest.commands()[0].options()[1].values().len(), 4);
        assert!(matches!(
            manifest.commands()[0].implementation(),
            CommandImplementation::Executable { .. }
        ));
    }

    #[test]
    fn rejects_unknown_manifest_command_and_option_fields() {
        let manifest_error = ExtensionManifest::parse(&MANIFEST.replace(
            "name = \"site-inventory\"",
            "name = \"site-inventory\"\nowner = \"ops\"",
        ))
        .expect_err("unknown manifest field should fail");
        assert!(manifest_error.to_string().contains("owner"));

        let command_error = ExtensionManifest::parse(&MANIFEST.replace(
            "about = \"Show a Host\"",
            "about = \"Show a Host\"\ninteractiv = true",
        ))
        .expect_err("unknown command field should fail");
        assert!(command_error.to_string().contains("interactiv"));

        let option_error = ExtensionManifest::parse(
            &MANIFEST.replace("positional = true", "positional = true\nrequried = true"),
        )
        .expect_err("unknown option field should fail");
        assert!(option_error.to_string().contains("requried"));
    }

    #[test]
    fn parses_manifest_only_workflows() {
        let manifest = ExtensionManifest::parse(
            r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[commands.workflow]
capabilities = ["mutate"]
result = "{ hosts: .steps.hosts, details: .steps.details }"

[[commands.workflow.steps]]
id = "hosts"
run = ["object", "list"]

[commands.workflow.steps.with]
class = { config = "hosts_class", default = "Hosts" }
all = true

[[commands.workflow.steps]]
id = "details"
run = ["object", "show"]
with = { id = { step = "hosts", select = ".[0].id" } }
"#,
        )
        .expect("workflow manifest should parse");

        assert!(manifest.executable().is_none());
        let workflow = manifest.commands()[0].workflow().expect("workflow");
        let step = &workflow.steps()[0];
        assert_eq!(step.id().as_str(), "hosts");
        assert_eq!(step.run().display(), "object list");
        assert!(workflow.allows_mutation());
        assert_eq!(
            workflow.result(),
            Some("{ hosts: .steps.hosts, details: .steps.details }")
        );
        assert!(matches!(
            workflow.steps()[1]
                .bindings()
                .values()
                .next()
                .expect("step binding"),
            WorkflowBinding::Step { step, select }
                if step.as_str() == "hosts" && select.as_deref() == Some(".[0].id")
        ));
        assert!(matches!(
            manifest.commands()[0].implementation(),
            CommandImplementation::Workflow(_)
        ));
    }

    #[test]
    fn rejects_interactive_workflows() {
        let manifest = r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]
interactive = true

[commands.workflow]

[[commands.workflow.steps]]
id = "classes"
run = ["class", "list"]
"#;
        let error = ExtensionManifest::parse(manifest).expect_err("workflow should fail");

        assert!(error
            .to_string()
            .contains("workflow commands cannot be interactive"));
    }

    #[test]
    fn rejects_forward_step_output_references() {
        let manifest = r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[commands.workflow]

[[commands.workflow.steps]]
id = "first"
run = ["object", "show"]
with = { id = { step = "later" } }

[[commands.workflow.steps]]
id = "later"
run = ["object", "list"]
"#;
        let error = ExtensionManifest::parse(manifest).expect_err("manifest should fail");

        assert!(error.to_string().contains("must name an earlier step"));
    }

    #[test]
    fn rejects_retired_action_workflow_syntax() {
        let manifest = r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[commands.workflow]

[[commands.workflow.actions]]
id = "legacy"
command = ["class", "list"]
"#;
        let error = ExtensionManifest::parse(manifest).expect_err("legacy syntax should fail");

        let message = error.to_string();
        assert!(message.contains("unknown field"));
        assert!(message.contains("actions"));
    }

    #[test]
    fn rejects_unknown_workflow_capabilities() {
        let manifest = r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[commands.workflow]
capabilities = ["network"]

[[commands.workflow.steps]]
id = "classes"
run = ["class", "list"]
"#;
        let error = ExtensionManifest::parse(manifest).expect_err("capability should fail");

        assert!(error
            .to_string()
            .contains("unknown workflow capability 'network'"));
    }

    #[test]
    fn rejects_misspelled_workflow_binding_sources() {
        let manifest = r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[commands.workflow]

[[commands.workflow.steps]]
id = "classes"
run = ["class", "show"]
with = { name = { stpe = "classes" } }
"#;
        let error = ExtensionManifest::parse(manifest).expect_err("binding typo should fail");

        assert!(error
            .to_string()
            .contains("binding tables must declare exactly one"));
    }

    #[test]
    fn requires_an_implementation_for_each_command() {
        let manifest = MANIFEST
            .replace("executable = \"bin/site-inventory\"\n", "")
            .replace("arguments = [\"host\", \"show\"]\n", "");
        let error = ExtensionManifest::parse(&manifest).expect_err("manifest should fail");

        assert!(matches!(
            error,
            ProtocolError::MissingCommandImplementation(_)
        ));
    }

    #[test]
    fn rejects_extension_recursion_in_workflows() {
        let manifest = r#"
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[commands.workflow]

[[commands.workflow.steps]]
id = "again"
run = ["extension", "inventory", "snapshot"]
"#;
        let error = ExtensionManifest::parse(manifest).expect_err("manifest should fail");

        assert!(error
            .to_string()
            .contains("cannot invoke extension commands"));
    }

    #[test]
    fn rejects_reserved_pack_names() {
        let manifest = MANIFEST.replace("site-inventory", "doctor");
        assert!(matches!(
            ExtensionManifest::parse(&manifest),
            Err(ProtocolError::ReservedPackName(_))
        ));
    }

    #[test]
    fn rejects_nonfinal_repeatable_positionals() {
        let manifest = MANIFEST.replace(
            "positional = true",
            "positional = true\nrepeatable = true\n\n[[commands.options]]\nname = \"second\"\nkind = \"string\"\npositional = true",
        );
        assert!(ExtensionManifest::parse(&manifest)
            .expect_err("manifest should fail")
            .to_string()
            .contains("final positional"));
    }

    #[test]
    fn rejects_command_paths_that_prefix_other_commands() {
        let manifest =
            format!("{MANIFEST}\n[[commands]]\npath = [\"host\"]\nabout = \"Host commands\"\n");
        let error = ExtensionManifest::parse(&manifest).expect_err("manifest should fail");

        assert!(matches!(error, ProtocolError::CommandPathPrefix { .. }));
        assert!(error.to_string().contains("'host'"));
        assert!(error.to_string().contains("'host show'"));
    }

    #[test]
    fn rejects_aliases_that_collide_after_removing_dashes() {
        let manifest = format!(
            "{MANIFEST}\n[[commands.options]]\nname = \"view\"\nkind = \"string\"\nlong = \"v\"\n"
        );
        let error = ExtensionManifest::parse(&manifest).expect_err("manifest should fail");

        assert!(error.to_string().contains("unique after removing dashes"));
    }

    #[test]
    fn rejects_long_aliases_that_collide_with_host_short_options() {
        let manifest = MANIFEST.replace("long = \"target-type\"", "long = \"h\"");
        let error = ExtensionManifest::parse(&manifest).expect_err("manifest should fail");

        assert!(error.to_string().contains("reserved by the host"));
    }

    #[test]
    fn validates_semantic_shapes() {
        SemanticOutput::new(
            SemanticOutputShape::Rows,
            json!([{"name": "host-1"}]),
            vec!["name".to_string()],
        )
        .expect("rows should validate");
        assert!(SemanticOutput::new(
            SemanticOutputShape::Rows,
            json!(["not-an-object"]),
            Vec::new(),
        )
        .is_err());
    }

    #[test]
    fn parses_success_and_error_responses() {
        let success = ExtensionResponse::parse(
            r#"{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"message","value":"done","columns":[]}}"#,
        )
        .expect("success response");
        assert!(success.is_ok());

        let failure = ExtensionResponse::parse(
            r#"{"protocol":"hubuum-cli.extension/v1","status":"error","error":{"code":"move_failed","message":"move failed","details":{}}}"#,
        )
        .expect("error response");
        assert!(!failure.is_ok());
    }
}
