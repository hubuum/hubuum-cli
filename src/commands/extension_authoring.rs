use std::fs::{self, create_dir_all, rename, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hubuum_extension_protocol::{ExtensionManifest, PackName, MANIFEST_FILENAME};
use hubuum_filter::OutputEnvelope;
use semver::Version;
use serde_json::{json, Value};

use super::required_positional;
use crate::catalog::{CommandContext, CommandSpec, ScopeSpec, WorkflowCardinality};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::extensions::{validate_package_source, WorkflowProgram};
use crate::output::set_semantic_output;
use crate::tokenizer::CommandTokenizer;

pub(super) fn contract_command(
    ctx: &CommandContext,
    tokens: &CommandTokenizer,
) -> Result<(), AppError> {
    let catalog = ctx.catalog.snapshot();
    let root = catalog
        .scope(&[])
        .expect("the command catalog always has a root scope");
    let mut commands = Vec::new();
    collect_builtin_commands(root, &mut Vec::new(), &mut commands);

    if tokens.get_options().contains_key("list") {
        if !tokens.get_positionals().is_empty() {
            return Err(AppError::ParseError(
                "--list cannot be combined with a command path".to_string(),
            ));
        }
        let rows = commands.into_iter().map(contract_summary).collect();
        return set_semantic_output(OutputEnvelope::rows(
            rows,
            vec![
                "command".to_string(),
                "effects".to_string(),
                "authentication".to_string(),
                "inputs".to_string(),
            ],
        ));
    }

    if tokens.get_positionals().is_empty() {
        return Err(AppError::ParseError(
            "provide a built-in command path, or use --list".to_string(),
        ));
    }
    let path = tokens.get_positionals();
    if path.first().is_some_and(|segment| segment == "extension") {
        return Err(AppError::CommandExecutionError(
            "extension workflows may call built-in commands only; the 'extension' scope is not callable"
                .to_string(),
        ));
    }
    let command = catalog.command(path).ok_or_else(|| {
        AppError::CommandExecutionError(format!(
            "built-in command '{}' was not found; use 'extension contract --list'",
            path.join(" ")
        ))
    })?;
    set_semantic_output(OutputEnvelope::detail(contract_detail(command), Vec::new()))
}

fn collect_builtin_commands<'a>(
    scope: &'a ScopeSpec,
    path: &mut Vec<String>,
    commands: &mut Vec<&'a CommandSpec>,
) {
    for command in scope.commands.values() {
        if path.first().is_none_or(|segment| segment != "extension") {
            commands.push(command);
        }
    }
    for (name, nested) in &scope.scopes {
        path.push(name.clone());
        collect_builtin_commands(nested, path, commands);
        path.pop();
    }
}

fn contract_summary(command: &CommandSpec) -> Value {
    let contract = command.workflow_contract();
    json!({
        "command": contract.command_id(),
        "about": command.about,
        "effects": contract.effects().as_str(),
        "authentication": command.handler.requires_authentication(),
        "inputs": contract.inputs().count(),
    })
}

fn contract_detail(command: &CommandSpec) -> Value {
    let contract = command.workflow_contract();
    json!({
        "command": contract.command_id(),
        "about": command.about,
        "effects": contract.effects().as_str(),
        "authentication": command.handler.requires_authentication(),
        "reauthentication_retry": match command.reauthentication_retry {
            ReauthenticationRetry::Safe => "safe",
            ReauthenticationRetry::Unsafe => "unsafe",
        },
        "inputs": contract.inputs().map(|input| {
            let option = command.workflow_input_option(input);
            json!({
                "id": input.id(),
                "type": input.value_type().as_str(),
                "cardinality": cardinality_value(input.cardinality()),
                "required": input.required(),
                "flag": input.flag(),
                "cli": {
                    "name": option.name,
                    "short": option.short,
                    "long": option.long,
                    "help": option.help,
                },
            })
        }).collect::<Vec<_>>(),
        "step_output": {
            "type": "json",
            "shape": "runtime",
            "value": ".steps.<step-id>",
            "metadata": ".outputs.<step-id>",
        },
    })
}

fn cardinality_value(cardinality: WorkflowCardinality) -> Value {
    let mut value = json!({ "kind": cardinality.kind() });
    if let Some(count) = cardinality.group_size() {
        value["count"] = json!(count);
    }
    value
}

#[derive(Debug, Clone, Copy)]
enum InitTemplate {
    Minimal,
    ReadOnly,
    Executable,
}

impl InitTemplate {
    fn parse(value: Option<&String>) -> Result<Self, AppError> {
        match value.map(String::as_str).unwrap_or("minimal") {
            "minimal" => Ok(Self::Minimal),
            "read-only" => Ok(Self::ReadOnly),
            "executable" => Ok(Self::Executable),
            value => Err(AppError::ParseError(format!(
                "unknown extension template '{value}'; expected minimal, read-only, or executable"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::ReadOnly => "read-only",
            Self::Executable => "executable",
        }
    }
}

pub(super) fn init_command(
    ctx: &CommandContext,
    tokens: &CommandTokenizer,
) -> Result<(), AppError> {
    let target = PathBuf::from(required_positional(tokens, "target")?);
    if target.exists() {
        return Err(AppError::CommandExecutionError(format!(
            "extension target '{}' already exists",
            target.display()
        )));
    }
    let name = tokens
        .get_options()
        .get("name")
        .map(String::as_str)
        .or_else(|| target.file_name().and_then(|name| name.to_str()))
        .ok_or_else(|| {
            AppError::ParseError(
                "could not derive a pack name from the target; pass --name".to_string(),
            )
        })?;
    let name = PackName::new(name).map_err(|error| AppError::ParseError(error.to_string()))?;
    let template = InitTemplate::parse(tokens.get_options().get("template"))?;
    let manifest_source = render_init_manifest(&name, template);
    let manifest = ExtensionManifest::parse(&manifest_source)
        .map_err(|error| AppError::CommandExecutionError(error.to_string()))?;

    if manifest.is_portable() {
        let catalog = ctx.catalog.snapshot();
        let config = manifest
            .resolve_config(&Value::Object(Default::default()))
            .map_err(|error| AppError::CommandExecutionError(error.to_string()))?;
        WorkflowProgram::compile(&manifest, &config, |path| catalog.command(path)).map_err(
            |message| {
                AppError::CommandExecutionError(format!(
                    "generated extension workflow compilation failed: {message}"
                ))
            },
        )?;
    }

    create_init_package(&target, &manifest_source, template)?;
    let manifest_path = target.join(MANIFEST_FILENAME);
    let mut files = vec![manifest_path.clone()];
    if matches!(template, InitTemplate::Executable) {
        files.push(target.join(init_executable_path()));
    }
    set_semantic_output(OutputEnvelope::detail(
        json!({
            "status": "created",
            "name": name.as_str(),
            "template": template.as_str(),
            "target": target,
            "files": files,
            "next": format!("hubuum-cli extension validate {}", manifest_path.parent().unwrap_or(Path::new(".")).display()),
        }),
        Vec::new(),
    ))
}

fn create_init_package(
    target: &Path,
    manifest_source: &str,
    template: InitTemplate,
) -> Result<(), AppError> {
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_dir_all(parent)?;
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("extension");
    let staging = parent.join(format!(".{target_name}.init-{}", init_suffix()));
    let create = || -> Result<(), AppError> {
        create_dir_all(&staging)?;
        write(staging.join(MANIFEST_FILENAME), manifest_source)?;
        if matches!(template, InitTemplate::Executable) {
            write_init_executable(&staging)?;
        }
        validate_package_source(&staging)?;
        rename(&staging, target)?;
        Ok(())
    };
    create().inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })
}

fn init_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

const EXTENSION_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/hubuum/hubuum-cli/main/schemas/hubuum-extension.schema.json";

fn cli_version_requirement() -> String {
    let version = Version::parse(env!("CARGO_PKG_VERSION")).expect("valid package version");
    format!(
        ">={}.{}.{},<{}.{}.0",
        version.major,
        version.minor,
        version.patch,
        version.major,
        version.minor + 1
    )
}

fn render_init_manifest(name: &PackName, template: InitTemplate) -> String {
    let requires_cli = cli_version_requirement();
    let value = match template {
        InitTemplate::Minimal => json!({
            "$schema": EXTENSION_SCHEMA_URL,
            "schema_version": 1,
            "kind": "portable",
            "name": name.as_str(),
            "version": "0.1.0",
            "requires_cli": requires_cli,
            "workflows": {
                "list_classes": {
                    "output": { "shape": "rows", "type": "json" },
                    "steps": [{
                        "id": "classes",
                        "kind": "run",
                        "run": ["class", "list"],
                        "with": { "all": true }
                    }],
                    "result": ".steps.classes"
                }
            },
            "commands": {
                "list_classes": {
                    "path": ["list-classes"],
                    "workflow": "list_classes",
                    "about": "List every Hubuum class"
                }
            }
        }),
        InitTemplate::ReadOnly => json!({
            "$schema": EXTENSION_SCHEMA_URL,
            "schema_version": 1,
            "kind": "portable",
            "name": name.as_str(),
            "version": "0.1.0",
            "requires_cli": requires_cli,
            "config": {
                "objects_class": {
                    "type": "string",
                    "default": "Hosts",
                    "help": "Class to include in the snapshot"
                }
            },
            "workflows": {
                "snapshot": {
                    "output": { "shape": "rows", "type": "json" },
                    "steps": [{
                        "id": "objects",
                        "kind": "run",
                        "run": ["object", "list"],
                        "with": {
                            "class": { "config": "objects_class" },
                            "all": true
                        }
                    }],
                    "result": ".steps.objects"
                }
            },
            "commands": {
                "snapshot": {
                    "path": ["snapshot"],
                    "workflow": "snapshot",
                    "about": "List configured inventory objects"
                }
            }
        }),
        InitTemplate::Executable => json!({
            "$schema": EXTENSION_SCHEMA_URL,
            "schema_version": 1,
            "kind": "executable",
            "name": name.as_str(),
            "version": "0.1.0",
            "requires_cli": requires_cli,
            "protocol": "hubuum-cli.extension/v1",
            "executable": init_executable_path(),
            "commands": {
                "hello": {
                    "path": ["hello"],
                    "arguments": ["hello"],
                    "about": "Return a protocol response from an external program"
                }
            }
        }),
    };
    format!(
        "// Generated by hubuum-cli extension init --template {}.\n{}\n",
        template.as_str(),
        serde_json::to_string_pretty(&value).expect("template is serializable")
    )
}

#[cfg(unix)]
fn init_executable_path() -> &'static str {
    "bin/extension"
}

#[cfg(windows)]
fn init_executable_path() -> &'static str {
    "bin/extension.cmd"
}

#[cfg(unix)]
fn write_init_executable(target: &Path) -> Result<PathBuf, AppError> {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let executable = target.join(init_executable_path());
    create_dir_all(executable.parent().expect("executable has a parent"))?;
    write(
        &executable,
        "#!/bin/sh\nset -eu\nprintf '%s\\n' '{\"protocol\":\"hubuum-cli.extension/v1\",\"status\":\"ok\",\"output\":{\"shape\":\"message\",\"value\":\"Hello from the extension\"},\"warnings\":[]}'\n",
    )?;
    fs::set_permissions(&executable, Permissions::from_mode(0o755))?;
    Ok(executable)
}

#[cfg(windows)]
fn write_init_executable(target: &Path) -> Result<PathBuf, AppError> {
    let executable = target.join(init_executable_path());
    create_dir_all(executable.parent().expect("executable has a parent"))?;
    write(
        &executable,
        "@echo off\r\necho {\"protocol\":\"hubuum-cli.extension/v1\",\"status\":\"ok\",\"output\":{\"shape\":\"message\",\"value\":\"Hello from the extension\"},\"warnings\":[]}\r\n",
    )?;
    Ok(executable)
}
