use super::{validate_declared_value, WorkflowInputDeclaration};
use std::collections::{BTreeMap, HashSet};

use jsonc_parser::parse_to_serde_value;
use semver::{Version, VersionReq};
use serde_json::Value;

use super::{
    invalid_config, invalid_named_workflow, invalid_option, invalid_workflow, nonempty,
    validate_workflow_calls, CommandDeclarationName, CommandPath, ConfigDeclaration,
    ExecutablePath, ExtensionPackKind, OptionDeclaration, OptionKind, PackName, ProtocolError,
    ProtocolVersion, RawCommand, RawManifest, RawOption, RawValueDeclaration, RawWorkflow,
    WorkflowDeclaration, WorkflowName, MANIFEST_PARSE_OPTIONS, MANIFEST_SCHEMA_VERSION,
};

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
