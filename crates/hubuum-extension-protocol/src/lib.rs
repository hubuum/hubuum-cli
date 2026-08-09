use std::collections::HashSet;
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
    executable: ExecutablePath,
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
        let executable = ExecutablePath::new(raw.executable)?;

        let mut command_paths = HashSet::new();
        let mut commands: Vec<CommandDeclaration> = Vec::with_capacity(raw.commands.len());
        for command in raw.commands {
            let command = CommandDeclaration::try_from(command)?;
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

    pub fn executable(&self) -> &ExecutablePath {
        &self.executable
    }

    pub fn commands(&self) -> &[CommandDeclaration] {
        &self.commands
    }
}

#[derive(Debug, Clone)]
pub struct CommandDeclaration {
    path: CommandPath,
    arguments: Vec<String>,
    about: Option<String>,
    long_about: Option<String>,
    examples: Vec<String>,
    interactive: bool,
    options: Vec<OptionDeclaration>,
}

impl CommandDeclaration {
    pub fn path(&self) -> &CommandPath {
        &self.path
    }

    pub fn arguments(&self) -> &[String] {
        &self.arguments
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
        self.interactive
    }

    pub fn options(&self) -> &[OptionDeclaration] {
        &self.options
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

        Ok(Self {
            path,
            arguments: raw.arguments,
            about: nonempty(raw.about),
            long_about: nonempty(raw.long_about),
            examples: raw.examples,
            interactive: raw.interactive,
            options,
        })
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
struct RawManifest {
    schema_version: u32,
    name: String,
    version: String,
    requires_cli: String,
    protocol: String,
    executable: String,
    #[serde(default)]
    commands: Vec<RawCommand>,
}

#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize)]
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

#[cfg(test)]
mod tests {
    use semver::Version;
    use serde_json::json;

    use super::{
        ExtensionManifest, ExtensionResponse, ProtocolError, SemanticOutput, SemanticOutputShape,
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
