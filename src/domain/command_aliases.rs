use std::collections::{BTreeMap, HashSet};

use serde::de::Error as _;
use serde::ser::SerializeStruct as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::catalog::CommandCatalog;
use crate::errors::AppError;

const RESERVED_COMMANDS: &[&str] = &["..", "?", "exit", "next", "quit"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CommandAliases(BTreeMap<String, CommandAliasEntry>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandAliasEntry {
    command: String,
    description: Option<String>,
}

impl Serialize for CommandAliasEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let Some(description) = &self.description else {
            return serializer.serialize_str(&self.command);
        };

        let mut value = serializer.serialize_struct("CommandAlias", 2)?;
        value.serialize_field("command", &self.command)?;
        value.serialize_field("description", description)?;
        value.end()
    }
}

impl<'de> Deserialize<'de> for CommandAliasEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StoredAlias {
            Command(String),
            Described {
                command: String,
                description: String,
            },
        }

        let (command, description) = match StoredAlias::deserialize(deserializer)? {
            StoredAlias::Command(command) => (command, None),
            StoredAlias::Described {
                command,
                description,
            } => (command, Some(description)),
        };
        let command = CommandAliasTarget::new(command).map_err(D::Error::custom)?;
        let description = description
            .map(CommandAliasDescription::new)
            .transpose()
            .map_err(D::Error::custom)?;
        Ok(Self {
            command: command.0,
            description: description.map(|description| description.0),
        })
    }
}

impl<'de> Deserialize<'de> for CommandAliases {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let aliases = BTreeMap::<String, CommandAliasEntry>::deserialize(deserializer)?;
        for name in aliases.keys() {
            CommandAliasName::new(name.clone()).map_err(D::Error::custom)?;
        }
        Ok(Self(aliases))
    }
}

impl CommandAliases {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|alias| alias.command.as_str())
    }

    pub fn description(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .and_then(|alias| alias.description.as_deref())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, alias)| (name.as_str(), alias.command.as_str()))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAliasName(String);

impl CommandAliasName {
    pub fn new(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        let mut chars = value.chars();
        let valid_start = chars.next().is_some_and(|ch| ch.is_ascii_alphabetic());
        let valid_rest = chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
        if !valid_start || !valid_rest {
            return Err(AppError::InvalidOption(format!(
                "alias name '{value}' must start with an ASCII letter and contain only letters, numbers, '-' or '_'"
            )));
        }
        if RESERVED_COMMANDS.contains(&value.as_str()) {
            return Err(AppError::InvalidOption(format!(
                "'{value}' is reserved and cannot be used as an alias name"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAliasTarget(String);

impl CommandAliasTarget {
    pub fn new(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(AppError::InvalidOption(
                "alias command cannot be empty".to_string(),
            ));
        }
        if value.contains(['\n', '\r']) {
            return Err(AppError::InvalidOption(
                "alias command must contain exactly one logical command line".to_string(),
            ));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAliasDescription(String);

impl CommandAliasDescription {
    pub fn new(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(AppError::InvalidOption(
                "alias description cannot be empty".to_string(),
            ));
        }
        if value.contains(['\n', '\r']) {
            return Err(AppError::InvalidOption(
                "alias description must fit on one logical line".to_string(),
            ));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasExpansion {
    line: String,
}

impl AliasExpansion {
    pub fn line(&self) -> &str {
        &self.line
    }
}

pub fn expand_command_aliases(
    catalog: &CommandCatalog,
    scope: &[String],
    aliases: &CommandAliases,
    line: &str,
) -> Result<AliasExpansion, AppError> {
    if !scope.is_empty() {
        return Ok(AliasExpansion {
            line: line.to_string(),
        });
    }
    let mut line = line.trim_start().to_string();
    let mut expanded_names = Vec::new();
    let mut seen = HashSet::new();
    let effective_scope = scope;

    while let Some((name, suffix)) = first_word_and_suffix(&line) {
        if is_catalog_or_shell_command(catalog, effective_scope, name) {
            break;
        }
        let Some(target) = aliases.get(name) else {
            break;
        };
        if !seen.insert(name.to_string()) {
            expanded_names.push(name.to_string());
            return Err(AppError::ParseError(format!(
                "Command alias cycle detected: {}",
                expanded_names.join(" -> ")
            )));
        }

        let target = CommandAliasTarget::new(target)?;
        expanded_names.push(name.to_string());
        line = format!("{}{}", target.as_str(), suffix);
    }

    Ok(AliasExpansion { line })
}

pub fn alias_name_shadows_command(catalog: &CommandCatalog, name: &str) -> bool {
    is_catalog_or_shell_command(catalog, &[], name)
}

fn is_catalog_or_shell_command(catalog: &CommandCatalog, scope: &[String], name: &str) -> bool {
    RESERVED_COMMANDS.contains(&name) || catalog.list_words(scope).iter().any(|word| word == name)
}

fn first_word_and_suffix(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    if line.is_empty() {
        return None;
    }
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    Some((&line[..end], &line[end..]))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        expand_command_aliases, CommandAliasDescription, CommandAliasEntry, CommandAliasName,
        CommandAliasTarget, CommandAliases,
    };
    use crate::commands::build_command_catalog;

    fn aliases(values: &[(&str, &str)]) -> CommandAliases {
        CommandAliases(BTreeMap::from_iter(values.iter().map(|(name, command)| {
            (
                (*name).to_string(),
                CommandAliasEntry {
                    command: (*command).to_string(),
                    description: None,
                },
            )
        })))
    }

    #[test]
    fn alias_names_are_single_safe_command_words() {
        assert!(CommandAliasName::new("outdated-kernels").is_ok());
        assert!(CommandAliasName::new("old_kernels2").is_ok());
        assert!(CommandAliasName::new("2old").is_err());
        assert!(CommandAliasName::new("old kernels").is_err());
        assert!(CommandAliasName::new("next").is_err());
    }

    #[test]
    fn alias_targets_are_one_nonempty_logical_line() {
        assert_eq!(
            CommandAliasTarget::new("  object list  ")
                .expect("target should validate")
                .as_str(),
            "object list"
        );
        assert!(CommandAliasTarget::new(" ").is_err());
        assert!(CommandAliasTarget::new("object list\nobject show").is_err());
    }

    #[test]
    fn alias_descriptions_are_trimmed_nonempty_logical_lines() {
        assert_eq!(
            CommandAliasDescription::new("  Find outdated kernels  ")
                .expect("description should validate")
                .as_str(),
            "Find outdated kernels"
        );
        assert!(CommandAliasDescription::new(" ").is_err());
        assert!(CommandAliasDescription::new("first\nsecond").is_err());
    }

    #[test]
    fn deserialization_preserves_alias_invariants() {
        assert!(serde_json::from_value::<CommandAliases>(serde_json::json!({
            "outdated-kernels": "object list | C"
        }))
        .is_ok());
        assert!(serde_json::from_value::<CommandAliases>(serde_json::json!({
            "invalid name": "help"
        }))
        .is_err());
        assert!(serde_json::from_value::<CommandAliases>(serde_json::json!({
            "valid": "help\nversion"
        }))
        .is_err());
    }

    #[test]
    fn described_aliases_round_trip_with_legacy_string_aliases() {
        let aliases = serde_json::from_value::<CommandAliases>(serde_json::json!({
            "hosts": "object list --class Hosts",
            "outdated-kernels": {
                "command": "object list --class Hosts | JQ '...'",
                "description": "Find hosts running an outdated kernel"
            }
        }))
        .expect("mixed alias formats should deserialize");

        assert_eq!(aliases.get("hosts"), Some("object list --class Hosts"));
        assert_eq!(aliases.description("hosts"), None);
        assert_eq!(
            aliases.description("outdated-kernels"),
            Some("Find hosts running an outdated kernel")
        );
        assert_eq!(
            serde_json::to_value(&aliases).expect("aliases should serialize"),
            serde_json::json!({
                "hosts": "object list --class Hosts",
                "outdated-kernels": {
                    "command": "object list --class Hosts | JQ '...'",
                    "description": "Find hosts running an outdated kernel"
                }
            })
        );
    }

    #[test]
    fn expansion_preserves_appended_pipes_and_redirects() {
        let catalog = build_command_catalog();
        let aliases = aliases(&[("hosts", "object list --class Hosts | P Name")]);
        let expanded =
            expand_command_aliases(&catalog, &[], &aliases, "hosts | S Name > hosts.txt")
                .expect("alias should expand");

        assert_eq!(
            expanded.line(),
            "object list --class Hosts | P Name | S Name > hosts.txt"
        );
    }

    #[test]
    fn aliases_can_chain_and_cycles_are_rejected() {
        let catalog = build_command_catalog();
        let chained = aliases(&[
            ("hosts", "all-hosts | P Name"),
            ("all-hosts", "object list"),
        ]);
        let expanded = expand_command_aliases(&catalog, &[], &chained, "hosts")
            .expect("alias chain should expand");
        assert_eq!(expanded.line(), "object list | P Name");

        let cyclic = aliases(&[("one", "two"), ("two", "one")]);
        let error =
            expand_command_aliases(&catalog, &[], &cyclic, "one").expect_err("cycle should fail");
        assert!(error.to_string().contains("one -> two -> one"));
    }

    #[test]
    fn catalog_commands_take_precedence_over_aliases() {
        let catalog = build_command_catalog();
        let aliases = aliases(&[("object", "help")]);
        let expanded = expand_command_aliases(&catalog, &[], &aliases, "object list")
            .expect("catalog command should remain unchanged");

        assert_eq!(expanded.line(), "object list");
    }
}
