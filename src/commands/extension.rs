use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use hubuum_extension_protocol::{
    CommandDeclaration, ExtensionManifest, ExtensionResponse, OptionDeclaration, OptionKind,
    SemanticOutput, SemanticOutputShape, WorkflowBinding, WorkflowDeclaration, PROTOCOL_V1,
};
use hubuum_filter::{apply_pipeline, OutputEnvelope, PipeStage};
use serde_json::{from_str, to_string, Map, Value};
use tokio::process::Command;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalogBuilder, CommandContext, CommandInvocation, CommandOutcome,
    CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::command_line::shell_escape;
use crate::commands::{render_format, standard_options, table_headers};
use crate::dispatch::{execute_offline_line, is_offline_builtin_command};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::extensions::{ExtensionPack, ExtensionRegistry};
use crate::output::{
    add_warning, reset_output, set_pipeline, set_pipeline_suffix, set_render_format,
    set_semantic_output, set_table_headers, take_output,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_extension_commands(
    builder: &mut CommandCatalogBuilder,
    registry: &Arc<ExtensionRegistry>,
) {
    for pack in registry.enabled_packs() {
        let Some(manifest) = pack.manifest_arc() else {
            continue;
        };
        for command in manifest.commands() {
            if let Some(workflow) = command.workflow() {
                register_workflow_command(
                    builder,
                    pack,
                    manifest.clone(),
                    command,
                    workflow.clone(),
                );
            } else if let Some(executable) = pack.executable().map(PathBuf::from) {
                register_external_command(builder, pack, manifest.clone(), executable, command);
            }
        }
    }
}

fn register_external_command(
    builder: &mut CommandCatalogBuilder,
    pack: &ExtensionPack,
    manifest: Arc<ExtensionManifest>,
    executable: PathBuf,
    command: &CommandDeclaration,
) {
    let segments = command.path().segments();
    let Some(name) = segments.last() else {
        return;
    };
    let mut path = vec![
        "extension".to_string(),
        manifest.name().as_str().to_string(),
    ];
    path.extend(segments[..segments.len() - 1].iter().cloned());
    let path_refs = path.iter().map(String::as_str).collect::<Vec<_>>();

    let mut options = command
        .options()
        .iter()
        .map(option_spec)
        .collect::<Vec<_>>();
    options.extend(standard_options().into_iter().map(|option| {
        OptionSpec {
            name: option.name,
            short: option.short,
            long: option.long,
            help: option.help,
            field_type_help: option.field_type_help,
            field_type: option.field_type,
            required: option.required,
            flag: option.flag,
            greedy: option.greedy,
            nargs: option.nargs,
            repeatable: option.repeatable,
            value_source: option.value_source,
            completion: option
                .autocomplete
                .map(CompletionSpec::Dynamic)
                .unwrap_or(CompletionSpec::None),
        }
    }));

    let handler = ExternalCommandHandler {
        pack: manifest.name().as_str().to_string(),
        executable,
        declaration: command.clone(),
        config: pack.config().clone(),
    };
    builder.add_command(
        &path_refs,
        CommandSpec {
            name: name.clone(),
            about: command.about().map(str::to_string),
            long_about: command.long_about().map(str::to_string),
            examples: (!command.examples().is_empty()).then(|| command.examples().join("\n")),
            options,
            reauthentication_retry: ReauthenticationRetry::Unsafe,
            handler: Arc::new(handler),
        },
    );
}

fn register_workflow_command(
    builder: &mut CommandCatalogBuilder,
    pack: &ExtensionPack,
    manifest: Arc<ExtensionManifest>,
    command: &CommandDeclaration,
    workflow: WorkflowDeclaration,
) {
    let segments = command.path().segments();
    let Some(name) = segments.last() else {
        return;
    };
    let mut path = vec![
        "extension".to_string(),
        manifest.name().as_str().to_string(),
    ];
    path.extend(segments[..segments.len() - 1].iter().cloned());
    let path_refs = path.iter().map(String::as_str).collect::<Vec<_>>();
    let reauthentication_retry = if workflow.steps().iter().any(|step| {
        builder
            .command(step.run().segments())
            .is_some_and(|command| command.reauthentication_retry == ReauthenticationRetry::Unsafe)
    }) {
        ReauthenticationRetry::Unsafe
    } else {
        ReauthenticationRetry::Safe
    };
    let requires_authentication = workflow.steps().iter().any(|step| {
        !is_offline_builtin_command(step.run().segments())
            && builder
                .command(step.run().segments())
                .is_some_and(|command| command.handler.requires_authentication())
    });

    let mut options = command
        .options()
        .iter()
        .map(option_spec)
        .collect::<Vec<_>>();
    options.extend(standard_options().into_iter().map(|option| {
        OptionSpec {
            name: option.name,
            short: option.short,
            long: option.long,
            help: option.help,
            field_type_help: option.field_type_help,
            field_type: option.field_type,
            required: option.required,
            flag: option.flag,
            greedy: option.greedy,
            nargs: option.nargs,
            repeatable: option.repeatable,
            value_source: option.value_source,
            completion: option
                .autocomplete
                .map(CompletionSpec::Dynamic)
                .unwrap_or(CompletionSpec::None),
        }
    }));

    builder.add_command(
        &path_refs,
        CommandSpec {
            name: name.clone(),
            about: command.about().map(str::to_string),
            long_about: command.long_about().map(str::to_string),
            examples: (!command.examples().is_empty()).then(|| command.examples().join("\n")),
            options,
            reauthentication_retry,
            handler: Arc::new(WorkflowCommandHandler {
                pack: manifest.name().as_str().to_string(),
                declaration: command.clone(),
                workflow,
                config: pack.config().clone(),
                requires_authentication,
            }),
        },
    );
}

fn option_spec(option: &OptionDeclaration) -> OptionSpec {
    let field_type = match option.kind() {
        OptionKind::String => TypeId::of::<String>(),
        OptionKind::Integer => TypeId::of::<i64>(),
        OptionKind::Number => TypeId::of::<f64>(),
        OptionKind::Boolean | OptionKind::Flag => TypeId::of::<bool>(),
    };
    OptionSpec {
        name: option.name().to_string(),
        short: option.short().map(|short| format!("-{short}")),
        long: option.long().map(|long| format!("--{long}")),
        help: option.help().to_string(),
        field_type_help: option.kind().type_name().to_string(),
        field_type,
        required: option.required(),
        flag: option.kind() == OptionKind::Flag,
        greedy: false,
        nargs: None,
        repeatable: option.repeatable(),
        value_source: false,
        completion: if option.values().is_empty() {
            CompletionSpec::None
        } else {
            CompletionSpec::Static(option.values().to_vec())
        },
    }
}

const WORKFLOW_HOST_INPUTS: &[&str] = &["help", "json", "output", "table-headers"];

pub(crate) fn workflow_binding_name(option: &OptionSpec) -> &str {
    option
        .long
        .as_deref()
        .map(|long| long.trim_start_matches('-'))
        .unwrap_or(&option.name)
}

pub(crate) fn workflow_step_arguments(
    command: &CommandSpec,
    values: &BTreeMap<String, Value>,
) -> Result<Vec<String>, String> {
    for name in values.keys() {
        if WORKFLOW_HOST_INPUTS.contains(&name.as_str()) {
            return Err(format!(
                "binding '{name}' is owned by the host and cannot be supplied by a workflow"
            ));
        }
        if !command
            .options
            .iter()
            .any(|option| workflow_binding_name(option) == name)
        {
            return Err(format!("command has no input named '{name}'"));
        }
    }

    let mut named_arguments = Vec::new();
    let mut positional_arguments = Vec::new();
    for option in &command.options {
        let binding_name = workflow_binding_name(option);
        if WORKFLOW_HOST_INPUTS.contains(&binding_name) {
            continue;
        }
        let Some(value) = values.get(binding_name) else {
            if option.required {
                return Err(format!("required input '{binding_name}' has no binding"));
            }
            continue;
        };
        let groups = workflow_binding_groups(value, option)?;
        if groups.is_empty() {
            if option.required {
                return Err(format!(
                    "required input '{binding_name}' resolved to no value"
                ));
            }
            continue;
        }
        let positional = option.short.is_none() && option.long.is_none();
        let prefix = option.long.as_ref().or(option.short.as_ref());
        if positional {
            positional_arguments.extend(groups.into_iter().flatten());
            continue;
        }
        let prefix =
            prefix.ok_or_else(|| format!("input '{}' has no CLI spelling", option.name))?;
        for mut group in groups {
            if group.is_empty() {
                named_arguments.push(prefix.clone());
            } else {
                let first = group.remove(0);
                named_arguments.push(format!("{prefix}={first}"));
                named_arguments.extend(group);
            }
        }
    }
    if !positional_arguments.is_empty() {
        named_arguments.push("--".to_string());
        named_arguments.extend(positional_arguments);
    }
    Ok(named_arguments)
}

fn workflow_binding_groups(value: &Value, option: &OptionSpec) -> Result<Vec<Vec<String>>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if option.flag {
        return match value {
            Value::Bool(true) => Ok(vec![Vec::new()]),
            Value::Bool(false) | Value::Null => Ok(Vec::new()),
            _ => Err(format!(
                "flag input '{}' requires a boolean value",
                option.name
            )),
        };
    }

    match (option.repeatable, option.nargs) {
        (true, Some(count)) => {
            let occurrences = value.as_array().ok_or_else(|| {
                format!(
                    "repeatable input '{}' with {count} values per occurrence requires an array of arrays",
                    option.name
                )
            })?;
            occurrences
                .iter()
                .map(|occurrence| workflow_fixed_group(occurrence, &option.name, count))
                .collect()
        }
        (false, Some(count)) => Ok(vec![workflow_fixed_group(value, &option.name, count)?]),
        (true, None) => match value {
            Value::Array(values) => values
                .iter()
                .map(|value| workflow_scalar_argument(value).map(|value| vec![value]))
                .collect(),
            _ => Ok(vec![vec![workflow_scalar_argument(value)?]]),
        },
        (false, None) => Ok(vec![vec![workflow_scalar_argument(value)?]]),
    }
}

fn workflow_fixed_group(value: &Value, name: &str, count: usize) -> Result<Vec<String>, String> {
    let values = value.as_array().ok_or_else(|| {
        format!("input '{name}' requires an array containing exactly {count} values")
    })?;
    if values.len() != count {
        return Err(format!(
            "input '{name}' requires exactly {count} values per occurrence, got {}",
            values.len()
        ));
    }
    values.iter().map(workflow_scalar_argument).collect()
}

fn workflow_scalar_argument(value: &Value) -> Result<String, String> {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => {
            to_string(value).map_err(|error| error.to_string())?
        }
        Value::Null => return Err("null cannot be passed as a command argument".to_string()),
    };
    if value.contains('\0') {
        Err("command arguments cannot contain NUL".to_string())
    } else {
        Ok(value)
    }
}

#[derive(Clone)]
struct ExternalCommandHandler {
    pack: String,
    executable: PathBuf,
    declaration: CommandDeclaration,
    config: Value,
}

#[async_trait]
impl AsyncCommandHandler for ExternalCommandHandler {
    async fn execute(
        &self,
        ctx: CommandContext,
        invocation: CommandInvocation,
    ) -> Result<CommandOutcome, AppError> {
        reset_output()?;
        set_pipeline(invocation.pipeline.clone())?;
        set_pipeline_suffix(invocation.pipeline_suffix.clone())?;
        let option_defs = extension_cli_options(&self.declaration);
        let tokens =
            CommandTokenizer::new_at(&invocation.raw_line, invocation.command_index, &option_defs)?;
        set_render_format(render_format(&tokens)?)?;
        set_table_headers(table_headers(&tokens)?)?;
        validate_invocation(&tokens, &self.declaration)?;

        let mut child = Command::new(&self.executable);
        child.args(self.declaration.arguments());
        child.args(forwarded_arguments(&tokens, invocation.command_index + 1));
        child.stdout(Stdio::piped()).stderr(Stdio::inherit());
        if self.declaration.interactive() && std::io::stdin().is_terminal() {
            child.stdin(Stdio::inherit());
        } else {
            child.stdin(Stdio::null());
        }
        configure_environment(&mut child, &ctx, &self.pack, &self.config)?;

        let process_output = child.output().await.map_err(|error| {
            extension_protocol_error(
                &self.pack,
                &invocation,
                format!("could not execute '{}': {error}", self.executable.display()),
            )
        })?;
        let stdout = String::from_utf8(process_output.stdout).map_err(|error| {
            extension_protocol_error(
                &self.pack,
                &invocation,
                format!("stdout was not valid UTF-8: {error}"),
            )
        })?;
        let response = ExtensionResponse::parse(&stdout).map_err(|error| {
            extension_protocol_error(&self.pack, &invocation, error.to_string())
        })?;
        let process_status = process_output.status;
        let exited_successfully = process_status.success();

        match response {
            ExtensionResponse::Ok {
                output, warnings, ..
            } => {
                if !exited_successfully {
                    return Err(extension_protocol_error(
                        &self.pack,
                        &invocation,
                        format!(
                            "success response used nonzero exit status {}",
                            output_status(&process_status)
                        ),
                    ));
                }
                for warning in warnings {
                    add_warning(warning)?;
                }
                set_semantic_output(convert_output(output))?;
                if let Some(services) = &ctx.services {
                    services.invalidate_completion();
                }
                Ok(CommandOutcome {
                    output: take_output()?,
                    scope_action: ScopeAction::None,
                    ..Default::default()
                })
            }
            ExtensionResponse::Error {
                error, warnings, ..
            } => {
                if exited_successfully {
                    return Err(extension_protocol_error(
                        &self.pack,
                        &invocation,
                        "error response used exit status 0".to_string(),
                    ));
                }
                let details = if error.details().is_null() {
                    String::new()
                } else {
                    format!("; details: {}", error.details())
                };
                Err(AppError::ExtensionCommand {
                    pack: self.pack.clone(),
                    command: self.declaration.path().display(),
                    code: error.code().to_string(),
                    message: error.message().to_string(),
                    details,
                }
                .with_warnings(warnings))
            }
        }
    }

    fn requires_authentication(&self) -> bool {
        false
    }
}

#[derive(Clone)]
struct WorkflowCommandHandler {
    pack: String,
    declaration: CommandDeclaration,
    workflow: WorkflowDeclaration,
    config: Value,
    requires_authentication: bool,
}

#[async_trait]
impl AsyncCommandHandler for WorkflowCommandHandler {
    async fn execute(
        &self,
        ctx: CommandContext,
        invocation: CommandInvocation,
    ) -> Result<CommandOutcome, AppError> {
        let option_defs = extension_cli_options(&self.declaration);
        let tokens =
            CommandTokenizer::new_at(&invocation.raw_line, invocation.command_index, &option_defs)?;
        validate_invocation(&tokens, &self.declaration)?;
        let inputs = workflow_input_values(&tokens, &self.declaration)
            .map_err(|message| workflow_error(&self.pack, &self.declaration, message))?;

        let mut values = Map::new();
        let mut columns = Vec::new();
        let mut warnings = Vec::new();
        let mut completed_steps = Vec::new();
        for step in self.workflow.steps() {
            let binding_values = step
                .bindings()
                .iter()
                .map(|(name, binding)| {
                    resolve_workflow_binding(binding, &inputs, &self.config, &self.pack, &values)
                        .map(|value| (name.as_str().to_string(), value))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map_err(|error| {
                    error.with_warnings(workflow_progress_warnings(&warnings, &completed_steps))
                })?;
            let (handler, command_path, command_index, arguments) = {
                let catalog = ctx.catalog.snapshot();
                let resolved = catalog.resolve_command(&[], step.run().segments())?;
                if resolved.command.reauthentication_retry == ReauthenticationRetry::Unsafe
                    && !self.workflow.allows_mutation()
                {
                    return Err(workflow_error(
                        &self.pack,
                        &self.declaration,
                        format!(
                            "step '{}' command '{}' may change state; declare capabilities = [\"mutate\"]",
                            step.id().as_str(),
                            step.run().display()
                        ),
                    )
                    .with_warnings(workflow_progress_warnings(
                        &warnings,
                        &completed_steps,
                    )));
                }
                let arguments = workflow_step_arguments(resolved.command, &binding_values)
                    .map_err(|message| {
                        workflow_error(
                            &self.pack,
                            &self.declaration,
                            format!("step '{}': {message}", step.id().as_str()),
                        )
                        .with_warnings(workflow_progress_warnings(&warnings, &completed_steps))
                    })?;
                (
                    resolved.command.handler.clone(),
                    resolved.command_path,
                    resolved.command_index,
                    arguments,
                )
            };
            let mut step_tokens = step.run().segments().to_vec();
            step_tokens.extend(["--output".to_string(), "json".to_string()]);
            step_tokens.extend(arguments);
            let raw_line = step_tokens
                .iter()
                .map(|token| shell_escape(token))
                .collect::<Vec<_>>()
                .join(" ");
            let step_result = if is_offline_builtin_command(step.run().segments()) {
                execute_offline_line(ctx.catalog.clone(), &raw_line).await
            } else {
                handler
                    .execute(
                        ctx.clone(),
                        CommandInvocation {
                            raw_line,
                            command_index,
                            command_path,
                            pipeline: Vec::new(),
                            pipeline_suffix: None,
                        },
                    )
                    .await
            };
            let outcome = step_result.map_err(|error| {
                let error = AppError::ExtensionWorkflowStep {
                    pack: self.pack.clone(),
                    workflow: self.declaration.path().display(),
                    step: step.id().as_str().to_string(),
                    command: step.run().display(),
                    source: Box::new(error),
                };
                error.with_warnings(workflow_progress_warnings(&warnings, &completed_steps))
            })?;
            warnings.extend(outcome.output.warnings);
            if outcome.redirect.is_some() || outcome.scope_action != ScopeAction::None {
                return Err(workflow_error(
                    &self.pack,
                    &self.declaration,
                    format!(
                        "step '{}' attempted an unsupported redirect or scope change",
                        step.id().as_str()
                    ),
                )
                .with_warnings(workflow_progress_warnings(&warnings, &completed_steps)));
            }
            if outcome.output.next_page_command.is_some() {
                return Err(workflow_error(
                    &self.pack,
                    &self.declaration,
                    format!(
                        "step '{}' returned a partial page; bind 'all = true' or an explicit pagination policy",
                        step.id().as_str()
                    ),
                )
                .with_warnings(workflow_progress_warnings(
                    &warnings,
                    &completed_steps,
                )));
            }
            if !outcome.output.errors.is_empty() {
                return Err(workflow_error(
                    &self.pack,
                    &self.declaration,
                    format!(
                        "step '{}' returned errors: {}",
                        step.id().as_str(),
                        outcome.output.errors.join("; ")
                    ),
                )
                .with_warnings(workflow_progress_warnings(&warnings, &completed_steps)));
            }
            let value = workflow_output_value(outcome.output.semantic, outcome.output.lines);
            let id = step.id().as_str().to_string();
            columns.push(id.clone());
            completed_steps.push(id.clone());
            values.insert(id, value);
        }

        let output = if let Some(result) = self.workflow.result() {
            let context = serde_json::json!({
                "input": inputs,
                "config": self.config.clone(),
                "steps": values,
            });
            apply_pipeline(
                workflow_value_envelope(context),
                &[PipeStage::Jq(result.to_string())],
            )
            .map_err(|error| {
                workflow_error(
                    &self.pack,
                    &self.declaration,
                    format!("result expression failed: {error}"),
                )
                .with_warnings(workflow_progress_warnings(&warnings, &completed_steps))
            })?
        } else {
            OutputEnvelope::detail(Value::Object(values), columns)
        };

        reset_output()?;
        set_pipeline(invocation.pipeline)?;
        set_pipeline_suffix(invocation.pipeline_suffix)?;
        set_render_format(render_format(&tokens)?)?;
        set_table_headers(table_headers(&tokens)?)?;
        for warning in warnings {
            add_warning(warning)?;
        }
        set_semantic_output(output)?;
        Ok(CommandOutcome {
            output: take_output()?,
            scope_action: ScopeAction::None,
            ..Default::default()
        })
    }

    fn requires_authentication(&self) -> bool {
        self.requires_authentication
    }
}

fn workflow_output_value(semantic: Vec<OutputEnvelope>, lines: Vec<String>) -> Value {
    match semantic.len() {
        0 => {
            if lines.is_empty() {
                Value::Null
            } else {
                let rendered = lines.join("\n");
                from_str(&rendered).unwrap_or_else(|_| {
                    if lines.len() == 1 {
                        Value::String(lines.into_iter().next().expect("one line was checked"))
                    } else {
                        Value::Array(lines.into_iter().map(Value::String).collect())
                    }
                })
            }
        }
        1 => {
            semantic
                .into_iter()
                .next()
                .expect("one semantic value was checked")
                .value
        }
        _ => Value::Array(
            semantic
                .into_iter()
                .map(|envelope| envelope.value)
                .collect(),
        ),
    }
}

fn workflow_progress_warnings(warnings: &[String], completed_steps: &[String]) -> Vec<String> {
    let mut warnings = warnings.to_vec();
    if !completed_steps.is_empty() {
        warnings.push(format!(
            "Workflow completed steps before the failure: {}",
            completed_steps.join(", ")
        ));
    }
    warnings
}

fn resolve_workflow_binding(
    binding: &WorkflowBinding,
    inputs: &Map<String, Value>,
    config: &Value,
    pack: &str,
    step_values: &Map<String, Value>,
) -> Result<Value, AppError> {
    match binding {
        WorkflowBinding::Literal(value) => Ok(value.clone()),
        WorkflowBinding::Input { name } => inputs.get(name).cloned().ok_or_else(|| {
            AppError::CommandExecutionError(format!(
                "extension workflow {pack} input '{name}' was unavailable"
            ))
        }),
        WorkflowBinding::Config { key, default } => Ok(config
            .as_object()
            .and_then(|config| config.get(key))
            .filter(|value| !value.is_null())
            .cloned()
            .or_else(|| default.clone())
            .unwrap_or(Value::Null)),
        WorkflowBinding::Step { step, select } => {
            let source = step_values.get(step.as_str()).ok_or_else(|| {
                AppError::CommandExecutionError(format!(
                    "extension workflow {pack} step output '{}' was unavailable",
                    step.as_str()
                ))
            })?;
            let value = if let Some(select) = select {
                apply_pipeline(
                    workflow_value_envelope(source.clone()),
                    &[PipeStage::Jq(select.to_string())],
                )
                .map_err(|error| {
                    AppError::CommandExecutionError(format!(
                        "extension workflow {pack} selector for step output '{}' failed: {error}",
                        step.as_str()
                    ))
                })?
                .value
            } else {
                source.clone()
            };
            Ok(value)
        }
    }
}

fn workflow_value_envelope(value: Value) -> OutputEnvelope {
    match value {
        Value::Object(_) => OutputEnvelope::detail(value, Vec::new()),
        Value::Array(values) => OutputEnvelope::values(values),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            OutputEnvelope::message(value)
        }
    }
}

fn workflow_input_values(
    tokens: &CommandTokenizer,
    declaration: &CommandDeclaration,
) -> Result<Map<String, Value>, String> {
    let mut inputs = Map::new();
    let mut positional_index = 0;
    for option in declaration.options() {
        let raw_values = if option.positional() {
            if option.repeatable() {
                let values = tokens.get_positionals()[positional_index..].to_vec();
                positional_index = tokens.get_positionals().len();
                values
            } else {
                let value = tokens.get_positionals().get(positional_index).cloned();
                positional_index += usize::from(value.is_some());
                value.into_iter().collect()
            }
        } else {
            tokens
                .get_option_occurrences()
                .iter()
                .filter(|occurrence| {
                    option
                        .short()
                        .is_some_and(|short| occurrence.key == short.to_string())
                        || option.long().is_some_and(|long| occurrence.key == long)
                })
                .map(|occurrence| occurrence.value.clone())
                .collect()
        };
        let value = if option.kind() == OptionKind::Flag {
            Value::Bool(!raw_values.is_empty())
        } else if option.repeatable() {
            Value::Array(
                raw_values
                    .iter()
                    .map(|value| workflow_typed_input(option, value))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            raw_values
                .first()
                .map(|value| workflow_typed_input(option, value))
                .transpose()?
                .unwrap_or(Value::Null)
        };
        inputs.insert(option.name().to_string(), value);
    }
    Ok(inputs)
}

fn workflow_typed_input(option: &OptionDeclaration, value: &str) -> Result<Value, String> {
    match option.kind() {
        OptionKind::String => Ok(Value::String(value.to_string())),
        OptionKind::Integer => value
            .parse::<i64>()
            .map(serde_json::Number::from)
            .map(Value::Number)
            .map_err(|error| format!("input '{}' is not an integer: {error}", option.name())),
        OptionKind::Number => value
            .parse::<f64>()
            .map_err(|error| format!("input '{}' is not a number: {error}", option.name()))
            .and_then(|number| {
                serde_json::Number::from_f64(number)
                    .map(Value::Number)
                    .ok_or_else(|| format!("input '{}' must be finite", option.name()))
            }),
        OptionKind::Boolean => value
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|error| format!("input '{}' is not a boolean: {error}", option.name())),
        OptionKind::Flag => Ok(Value::Bool(true)),
    }
}

fn workflow_error(pack: &str, declaration: &CommandDeclaration, message: String) -> AppError {
    AppError::ExtensionWorkflow {
        pack: pack.to_string(),
        command: declaration.path().display(),
        message,
    }
}

fn extension_cli_options(declaration: &CommandDeclaration) -> Vec<crate::commands::CliOption> {
    let mut options = declaration
        .options()
        .iter()
        .map(|option| option_spec(option).to_cli_option())
        .collect::<Vec<_>>();
    options.extend(standard_options());
    options
}

fn validate_invocation(
    tokens: &CommandTokenizer,
    declaration: &CommandDeclaration,
) -> Result<(), AppError> {
    let named = declaration
        .options()
        .iter()
        .filter(|option| !option.positional())
        .collect::<Vec<_>>();
    let mut by_key = HashMap::new();
    for option in &named {
        if let Some(short) = option.short() {
            by_key.insert(short.to_string(), *option);
        }
        if let Some(long) = option.long() {
            by_key.insert(long.to_string(), *option);
        }
    }
    let standard_keys = HashSet::from(["h", "help", "j", "json", "o", "output", "table-headers"]);
    let mut counts = HashMap::<&str, usize>::new();
    for occurrence in tokens.get_option_occurrences() {
        if standard_keys.contains(occurrence.key.as_str()) {
            continue;
        }
        let Some(option) = by_key.get(&occurrence.key).copied() else {
            return Err(AppError::InvalidOption(format!(
                "{} for extension command {}",
                display_option(&occurrence.key),
                declaration.path().display()
            )));
        };
        *counts.entry(option.name()).or_default() += 1;
        validate_value(option, &occurrence.value)?;
    }

    for option in named {
        let count = counts.get(option.name()).copied().unwrap_or(0);
        if option.required() && count == 0 {
            return Err(AppError::MissingOptions(vec![option.name().to_string()]));
        }
        if !option.repeatable() && count > 1 {
            return Err(AppError::DuplicateOptions(vec![option.name().to_string()]));
        }
    }

    let positionals = declaration
        .options()
        .iter()
        .filter(|option| option.positional())
        .collect::<Vec<_>>();
    let values = tokens.get_positionals();
    let mut value_index = 0;
    for (index, option) in positionals.iter().enumerate() {
        if option.repeatable() {
            let remaining = &values[value_index..];
            if option.required() && remaining.is_empty() {
                return Err(AppError::MissingOptions(vec![option.name().to_string()]));
            }
            for value in remaining {
                validate_value(option, value)?;
            }
            value_index = values.len();
        } else if let Some(value) = values.get(value_index) {
            validate_value(option, value)?;
            value_index += 1;
        } else if option.required() {
            return Err(AppError::MissingOptions(vec![option.name().to_string()]));
        }
        if index + 1 == positionals.len() && value_index < values.len() {
            return Err(AppError::ParseError(format!(
                "Unexpected positional argument '{}' for extension command {}",
                values[value_index],
                declaration.path().display()
            )));
        }
    }
    if positionals.is_empty() && !values.is_empty() {
        return Err(AppError::ParseError(format!(
            "Unexpected positional argument '{}' for extension command {}",
            values[0],
            declaration.path().display()
        )));
    }
    Ok(())
}

fn validate_value(option: &OptionDeclaration, value: &str) -> Result<(), AppError> {
    if !option.kind().validate_value(value) {
        return Err(AppError::ParseError(format!(
            "Option '{}' has value '{}' (expected type: {})",
            option.name(),
            value,
            option.kind().type_name()
        )));
    }
    if !option.values().is_empty() && !option.values().iter().any(|allowed| allowed == value) {
        return Err(AppError::ParseError(format!(
            "Option '{}' has unsupported value '{}'; use one of: {}",
            option.name(),
            value,
            option.values().join(", ")
        )));
    }
    Ok(())
}

fn forwarded_arguments(tokens: &CommandTokenizer, argument_start: usize) -> Vec<String> {
    let raw = tokens.raw_tokens();
    let mut forwarded = Vec::new();
    let mut index = argument_start;
    while let Some(argument) = raw.get(index) {
        let name = argument.split('=').next().unwrap_or(argument);
        match name {
            "-h" | "--help" | "-j" | "--json" => index += 1,
            "-o" | "--output" | "--table-headers" => {
                index += if argument.contains('=') { 1 } else { 2 };
            }
            _ => {
                forwarded.push(argument.clone());
                index += 1;
            }
        }
    }
    forwarded
}

fn configure_environment(
    child: &mut Command,
    ctx: &CommandContext,
    pack: &str,
    config: &Value,
) -> Result<(), AppError> {
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("HUBUUM_CLI__") {
            child.env_remove(name);
        }
    }
    child.env("HUBUUM_EXTENSION_PROTOCOL", PROTOCOL_V1);
    child.env("HUBUUM_EXTENSION_PACK", pack);
    child.env(
        "HUBUUM_EXTENSION_CONFIG_JSON",
        to_string(config).map_err(AppError::ParseJsonError)?,
    );
    child.env(
        "HUBUUM_CLI_BIN",
        std::env::current_exe().map_err(AppError::IoError)?,
    );
    child.env("HUBUUM_CLI__SERVER__HOSTNAME", &ctx.config.server.hostname);
    child.env(
        "HUBUUM_CLI__SERVER__PORT",
        ctx.config.server.port.to_string(),
    );
    child.env(
        "HUBUUM_CLI__SERVER__PROTOCOL",
        ctx.config.server.protocol.to_string(),
    );
    child.env(
        "HUBUUM_CLI__SERVER__SSL_VALIDATION",
        ctx.config.server.ssl_validation.to_string(),
    );
    child.env("HUBUUM_CLI__SERVER__USERNAME", &ctx.config.server.username);
    if let Some(scope) = &ctx.config.server.identity_scope {
        child.env("HUBUUM_CLI__SERVER__IDENTITY_SCOPE", scope);
    }
    if let Some(token_file) = &ctx.config.server.token_file {
        child.env("HUBUUM_CLI__SERVER__TOKEN_FILE", token_file);
    }
    Ok(())
}

fn convert_output(output: SemanticOutput) -> OutputEnvelope {
    let (shape, value, columns) = output.into_parts();
    match shape {
        SemanticOutputShape::Empty => OutputEnvelope::empty(),
        SemanticOutputShape::Lines => OutputEnvelope::lines(
            value
                .as_array()
                .expect("protocol output was validated")
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .expect("protocol lines were validated")
                        .to_string()
                })
                .collect(),
        ),
        SemanticOutputShape::Rows => OutputEnvelope::rows(
            value
                .as_array()
                .expect("protocol rows were validated")
                .clone(),
            columns,
        ),
        SemanticOutputShape::Detail => OutputEnvelope::detail(value, columns),
        SemanticOutputShape::Message => OutputEnvelope::message(value),
        SemanticOutputShape::Values => OutputEnvelope::values(
            value
                .as_array()
                .expect("protocol values were validated")
                .clone(),
        ),
    }
}

fn extension_protocol_error(
    pack: &str,
    invocation: &CommandInvocation,
    message: String,
) -> AppError {
    AppError::ExtensionProtocol {
        pack: pack.to_string(),
        command: invocation.command_path.join(" "),
        message,
    }
}

fn display_option(key: &str) -> String {
    if key.len() == 1 {
        format!("-{key}")
    } else {
        format!("--{key}")
    }
}

fn output_status(status: &std::process::ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated by signal".to_string(),
        |code| code.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use hubuum_extension_protocol::ExtensionManifest;
    use hubuum_filter::OutputEnvelope;
    use serde_json::json;
    use serial_test::serial;

    use super::{
        extension_cli_options, forwarded_arguments, workflow_input_values, workflow_step_arguments,
        AsyncCommandHandler, WorkflowCommandHandler,
    };
    use crate::catalog::{
        CatalogStore, CommandCatalogBuilder, CommandContext, CommandInvocation, CommandOutcome,
        CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
    };
    use crate::config::get_config;
    use crate::errors::{AppError, ReauthenticationRetry};
    use crate::output::OutputSnapshot;
    use crate::tokenizer::CommandTokenizer;

    struct SemanticAction;
    struct EchoAction;
    struct RenderedJsonAction;

    fn catalog_option(
        name: &str,
        long: Option<&str>,
        required: bool,
        flag: bool,
        repeatable: bool,
        nargs: Option<usize>,
    ) -> OptionSpec {
        OptionSpec {
            name: name.to_string(),
            short: None,
            long: long.map(str::to_string),
            help: String::new(),
            field_type_help: "string".to_string(),
            field_type: TypeId::of::<String>(),
            required,
            flag,
            greedy: false,
            nargs,
            repeatable,
            value_source: false,
            completion: CompletionSpec::None,
        }
    }

    #[async_trait]
    impl AsyncCommandHandler for SemanticAction {
        async fn execute(
            &self,
            _ctx: CommandContext,
            _invocation: CommandInvocation,
        ) -> Result<CommandOutcome, AppError> {
            Ok(CommandOutcome {
                output: OutputSnapshot {
                    semantic: vec![OutputEnvelope::rows(
                        vec![json!({"id": 1})],
                        vec!["id".to_string()],
                    )],
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl AsyncCommandHandler for EchoAction {
        async fn execute(
            &self,
            _ctx: CommandContext,
            invocation: CommandInvocation,
        ) -> Result<CommandOutcome, AppError> {
            Ok(CommandOutcome {
                output: OutputSnapshot {
                    semantic: vec![OutputEnvelope::message(json!(invocation.raw_line))],
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl AsyncCommandHandler for RenderedJsonAction {
        async fn execute(
            &self,
            _ctx: CommandContext,
            _invocation: CommandInvocation,
        ) -> Result<CommandOutcome, AppError> {
            Ok(CommandOutcome {
                output: OutputSnapshot {
                    lines: vec![
                        "{".to_string(),
                        "  \"legacy\": true".to_string(),
                        "}".to_string(),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    #[test]
    fn scoped_invocations_forward_every_entered_extension_argument() {
        let manifest = ExtensionManifest::parse(
            r#"
schema_version = 1
name = "demo"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/demo"

[[commands]]
path = ["inventory", "list"]

[[commands.options]]
name = "target"
kind = "string"
positional = true

[[commands.options]]
name = "state"
kind = "string"
long = "state"
"#,
        )
        .expect("manifest");
        let declaration = &manifest.commands()[0];
        let tokens = CommandTokenizer::new_at(
            "list target-1 --state active --output json",
            0,
            &extension_cli_options(declaration),
        )
        .expect("scoped invocation should tokenize");

        assert_eq!(
            forwarded_arguments(&tokens, 1),
            ["target-1", "--state", "active"]
        );
    }

    #[test]
    fn workflow_inputs_preserve_optional_flags_and_repeated_values() {
        let manifest = ExtensionManifest::parse(
            r#"
schema_version = 1
name = "demo"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/demo"

[[commands]]
path = ["run"]

[[commands.options]]
name = "verbose"
kind = "flag"
long = "verbose"

[[commands.options]]
name = "tag"
kind = "string"
long = "tag"
repeatable = true
"#,
        )
        .expect("manifest");
        let declaration = &manifest.commands()[0];
        let tokens = CommandTokenizer::new_at(
            "run --verbose --tag alpha --tag beta",
            0,
            &extension_cli_options(declaration),
        )
        .expect("workflow inputs should tokenize");

        assert_eq!(
            workflow_input_values(&tokens, declaration).expect("typed workflow inputs"),
            json!({ "verbose": true, "tag": ["alpha", "beta"] })
                .as_object()
                .expect("object")
                .clone()
        );
    }

    #[test]
    fn workflow_bindings_compile_named_flags_positionals_and_fixed_arity_values() {
        let command = CommandSpec {
            name: "demo".to_string(),
            about: None,
            long_about: None,
            examples: None,
            options: vec![
                catalog_option("target", None, true, false, false, None),
                catalog_option("class", Some("--class"), false, false, false, None),
                catalog_option("all", Some("--all"), false, true, false, None),
                catalog_option(
                    "where_clauses",
                    Some("--where"),
                    false,
                    false,
                    true,
                    Some(3),
                ),
            ],
            reauthentication_retry: ReauthenticationRetry::Safe,
            handler: Arc::new(EchoAction),
        };
        let values = BTreeMap::from([
            ("target".to_string(), json!("server-01")),
            ("class".to_string(), json!("Hosts")),
            ("all".to_string(), json!(true)),
            (
                "where".to_string(),
                json!([["state", "eq", "active"], ["kind", "eq", "host"]]),
            ),
        ]);

        assert_eq!(
            workflow_step_arguments(&command, &values).expect("compiled arguments"),
            [
                "--class=Hosts",
                "--all",
                "--where=state",
                "eq",
                "active",
                "--where=kind",
                "eq",
                "host",
                "--",
                "server-01",
            ]
        );
    }

    #[tokio::test]
    #[serial]
    async fn workflow_composes_semantic_step_values_in_process() {
        let manifest = ExtensionManifest::parse(
            r#"
schema_version = 1
name = "demo"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]

[[commands.options]]
name = "target"
kind = "integer"
positional = true
required = true

[commands.workflow]
result = "{ requested: .input.target, items: .steps.items, selected: .steps.selected, input_echo: .steps.input_echo, legacy: .steps.legacy }"

[[commands.workflow.steps]]
id = "items"
run = ["object", "list"]

[[commands.workflow.steps]]
id = "selected"
run = ["object", "echo"]
with = { id = { step = "items", select = ".[0].id" } }

[[commands.workflow.steps]]
id = "input_echo"
run = ["object", "echo"]
with = { id = { input = "target" } }

[[commands.workflow.steps]]
id = "legacy"
run = ["object", "legacy"]
"#,
        )
        .expect("workflow manifest");
        let declaration = manifest.commands()[0].clone();
        let handler = WorkflowCommandHandler {
            pack: "demo".to_string(),
            workflow: declaration.workflow().expect("workflow").clone(),
            declaration,
            config: json!({}),
            requires_authentication: false,
        };
        let mut builder = CommandCatalogBuilder::new();
        builder.add_command(
            &["object"],
            CommandSpec {
                name: "list".to_string(),
                about: None,
                long_about: None,
                examples: None,
                options: Vec::new(),
                reauthentication_retry: ReauthenticationRetry::Safe,
                handler: Arc::new(SemanticAction),
            },
        );
        builder.add_command(
            &["object"],
            CommandSpec {
                name: "echo".to_string(),
                about: None,
                long_about: None,
                examples: None,
                options: vec![OptionSpec {
                    name: "id".to_string(),
                    short: None,
                    long: Some("--id".to_string()),
                    help: String::new(),
                    field_type_help: "string".to_string(),
                    field_type: TypeId::of::<String>(),
                    required: true,
                    flag: false,
                    greedy: false,
                    nargs: None,
                    repeatable: false,
                    value_source: false,
                    completion: CompletionSpec::None,
                }],
                reauthentication_retry: ReauthenticationRetry::Safe,
                handler: Arc::new(EchoAction),
            },
        );
        builder.add_command(
            &["object"],
            CommandSpec {
                name: "legacy".to_string(),
                about: None,
                long_about: None,
                examples: None,
                options: Vec::new(),
                reauthentication_retry: ReauthenticationRetry::Safe,
                handler: Arc::new(RenderedJsonAction),
            },
        );
        let context = CommandContext {
            config: get_config(),
            services: None,
            catalog: Arc::new(CatalogStore::new(builder.build())),
        };

        let outcome = handler
            .execute(
                context,
                CommandInvocation {
                    raw_line: "extension demo snapshot 7 --output json".to_string(),
                    command_index: 2,
                    command_path: vec![
                        "extension".to_string(),
                        "demo".to_string(),
                        "snapshot".to_string(),
                    ],
                    pipeline: Vec::new(),
                    pipeline_suffix: None,
                },
            )
            .await
            .expect("workflow should execute");

        assert_eq!(outcome.scope_action, ScopeAction::None);
        assert_eq!(
            outcome.output.semantic[0].value,
            json!({
                "requested": 7,
                "items": [{"id": 1}],
                "selected": "object echo --output json --id=1",
                "input_echo": "object echo --output json --id=7",
                "legacy": {"legacy": true}
            })
        );
    }
}
