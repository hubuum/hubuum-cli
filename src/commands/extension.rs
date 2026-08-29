use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::{future::Future, mem};

use async_trait::async_trait;
use hubuum_extension_protocol::{
    CommandDeclaration, ExtensionManifest, ExtensionResponse, OptionDeclaration, OptionKind,
    SemanticOutput, SemanticOutputShape, PROTOCOL_V1,
};
use hubuum_filter::{evaluate_bounded_jq, OutputEnvelope, OutputShape};
use serde_json::{from_str, json, to_string, Map, Value};
use tokio::process::Command;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalogBuilder, CommandContext, CommandEffects, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction, WorkflowCardinality,
    WorkflowInputContract, WorkflowValueType,
};
use crate::command_line::shell_escape;
use crate::commands::{render_format, standard_options, table_headers, CliOption};
use crate::dispatch::{execute_offline_line, is_offline_builtin_command};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::extensions::{
    ExtensionPack, ExtensionRegistry, PlanBinding, PlanStep, WorkflowPlan, WorkflowProgram,
};
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
                register_workflow_command(builder, pack, &manifest, command, workflow.as_str());
            } else if let Some(executable) = pack.executable().map(PathBuf::from) {
                register_external_command(builder, pack, &manifest, executable, command);
            }
        }
    }
}

fn register_external_command(
    builder: &mut CommandCatalogBuilder,
    pack: &ExtensionPack,
    manifest: &ExtensionManifest,
    executable: PathBuf,
    command: &CommandDeclaration,
) {
    let handler = ExternalCommandHandler {
        pack: manifest.name().as_str().to_string(),
        executable,
        declaration: command.clone(),
        config: pack.config().clone(),
    };
    register_extension_command(
        builder,
        manifest,
        command,
        ReauthenticationRetry::Unsafe,
        CommandEffects::Mutating,
        handler,
    );
}

fn register_workflow_command(
    builder: &mut CommandCatalogBuilder,
    pack: &ExtensionPack,
    manifest: &ExtensionManifest,
    command: &CommandDeclaration,
    workflow: &str,
) {
    let Some(program) = pack.workflow_program_arc() else {
        return;
    };
    let Some(plan) = program.plan(workflow) else {
        return;
    };

    let handler = WorkflowCommandHandler {
        pack: manifest.name().as_str().to_string(),
        declaration: command.clone(),
        program,
        plan: plan.clone(),
        config: pack.config().clone(),
    };
    register_extension_command(
        builder,
        manifest,
        command,
        plan.reauthentication_retry(),
        plan.effects(),
        handler,
    );
}

fn register_extension_command<H>(
    builder: &mut CommandCatalogBuilder,
    manifest: &ExtensionManifest,
    command: &CommandDeclaration,
    reauthentication_retry: ReauthenticationRetry,
    effects: CommandEffects,
    handler: H,
) where
    H: AsyncCommandHandler + 'static,
{
    let (name, parents) = command.path().split_last();
    let mut path = vec![
        "extension".to_string(),
        manifest.name().as_str().to_string(),
    ];
    path.extend(parents.iter().cloned());
    let path_refs = path.iter().map(String::as_str).collect::<Vec<_>>();

    let mut spec = CommandSpec::new(
        name,
        extension_command_options(command),
        reauthentication_retry,
        effects,
        Arc::new(handler),
    );
    spec.about = command.about().map(str::to_string);
    spec.long_about = command.long_about().map(str::to_string);
    spec.examples = (!command.examples().is_empty()).then(|| command.examples().join("\n"));
    builder.add_command(&path_refs, spec);
}

fn extension_command_options(command: &CommandDeclaration) -> Vec<OptionSpec> {
    let mut options = command
        .options()
        .iter()
        .map(option_spec)
        .collect::<Vec<_>>();
    options.extend(standard_options().into_iter().map(standard_option_spec));
    options
}

fn standard_option_spec(option: CliOption) -> OptionSpec {
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
}

fn option_spec(option: &OptionDeclaration) -> OptionSpec {
    let field_type = match option.kind() {
        OptionKind::String => TypeId::of::<String>(),
        OptionKind::Integer => TypeId::of::<i64>(),
        OptionKind::Number => TypeId::of::<f64>(),
        OptionKind::Boolean | OptionKind::Flag => TypeId::of::<bool>(),
        OptionKind::Json => TypeId::of::<Value>(),
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
        if command.workflow_contract().input(name).is_none() {
            return Err(format!("command has no input named '{name}'"));
        }
    }

    let mut named_arguments = Vec::new();
    let mut positional_arguments = Vec::new();
    for input in command.workflow_contract().inputs() {
        let binding_name = input.id();
        if WORKFLOW_HOST_INPUTS.contains(&binding_name) {
            continue;
        }
        let option = command.workflow_input_option(input);
        let Some(value) = values.get(binding_name) else {
            if input.required() {
                return Err(format!("required input '{binding_name}' has no binding"));
            }
            continue;
        };
        let groups = workflow_binding_groups(value, input)?;
        if groups.is_empty() {
            if input.required() {
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

fn workflow_binding_groups(
    value: &Value,
    input: &WorkflowInputContract,
) -> Result<Vec<Vec<String>>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if input.flag() {
        return match value {
            Value::Bool(true) => Ok(vec![Vec::new()]),
            Value::Bool(false) | Value::Null => Ok(Vec::new()),
            _ => Err(format!(
                "flag input '{}' requires a boolean value",
                input.id()
            )),
        };
    }

    match input.cardinality() {
        WorkflowCardinality::RepeatedFixed(count) => {
            let occurrences = value.as_array().ok_or_else(|| {
                format!(
                    "repeatable input '{}' with {count} values per occurrence requires an array of arrays",
                    input.id()
                )
            })?;
            occurrences
                .iter()
                .map(|occurrence| workflow_fixed_group(occurrence, input, count))
                .collect()
        }
        WorkflowCardinality::Fixed(count) => Ok(vec![workflow_fixed_group(value, input, count)?]),
        WorkflowCardinality::Repeated => match value {
            Value::Array(values) => values
                .iter()
                .map(|value| workflow_scalar_argument(value, input).map(|value| vec![value]))
                .collect(),
            _ => Ok(vec![vec![workflow_scalar_argument(value, input)?]]),
        },
        WorkflowCardinality::One => Ok(vec![vec![workflow_scalar_argument(value, input)?]]),
    }
}

fn workflow_fixed_group(
    value: &Value,
    input: &WorkflowInputContract,
    count: usize,
) -> Result<Vec<String>, String> {
    let values = value.as_array().ok_or_else(|| {
        format!(
            "input '{}' requires an array containing exactly {count} values",
            input.id()
        )
    })?;
    if values.len() != count {
        return Err(format!(
            "input '{}' requires exactly {count} values per occurrence, got {}",
            input.id(),
            values.len()
        ));
    }
    values
        .iter()
        .map(|value| workflow_scalar_argument(value, input))
        .collect()
}

fn workflow_scalar_argument(
    value: &Value,
    input: &WorkflowInputContract,
) -> Result<String, String> {
    let valid_type = match input.value_type() {
        WorkflowValueType::Text => value.is_string(),
        WorkflowValueType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        WorkflowValueType::Number => value.is_number(),
        WorkflowValueType::Boolean => value.is_boolean(),
        WorkflowValueType::Json => true,
    };
    if !valid_type {
        return Err(format!(
            "input '{}' expects {:?}, got {}",
            input.id(),
            input.value_type(),
            workflow_json_type(value)
        ));
    }
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

fn workflow_json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
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
    program: Arc<WorkflowProgram>,
    plan: Arc<WorkflowPlan>,
    config: Value,
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
        let inputs = self
            .plan
            .resolve_inputs(&Value::Object(inputs))
            .map_err(|message| workflow_error(&self.pack, &self.declaration, message))?;
        let mut runtime = WorkflowRuntime::new(self.plan.limits());
        let execution = self
            .execute_plan(ctx.clone(), self.plan.clone(), inputs, &mut runtime)
            .await?;
        let output = self
            .plan
            .output()
            .semantic_output(execution.value)
            .map(convert_output)
            .map_err(|message| {
                workflow_error(
                    &self.pack,
                    &self.declaration,
                    format!("declared output contract failed: {message}"),
                )
            })?;

        reset_output()?;
        set_pipeline(invocation.pipeline)?;
        set_pipeline_suffix(invocation.pipeline_suffix)?;
        set_render_format(render_format(&tokens)?)?;
        set_table_headers(table_headers(&tokens)?)?;
        for warning in execution.warnings {
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
        self.plan.requires_authentication()
    }
}

impl WorkflowCommandHandler {
    fn execute_plan<'a>(
        &'a self,
        ctx: CommandContext,
        plan: Arc<WorkflowPlan>,
        inputs: Map<String, Value>,
        runtime: &'a mut WorkflowRuntime,
    ) -> Pin<Box<dyn Future<Output = Result<WorkflowExecution, AppError>> + Send + 'a>> {
        Box::pin(async move {
            runtime.enter_call(WorkflowLocation::new(
                &self.pack,
                &self.declaration,
                plan.name(),
                "<call>",
            ))?;
            let result = self.execute_plan_steps(ctx, plan, inputs, runtime).await;
            runtime.leave_call();
            result
        })
    }

    async fn execute_plan_steps(
        &self,
        ctx: CommandContext,
        plan: Arc<WorkflowPlan>,
        inputs: Map<String, Value>,
        runtime: &mut WorkflowRuntime,
    ) -> Result<WorkflowExecution, AppError> {
        let mut values = Map::new();
        let mut outputs = Map::new();
        let mut warnings = Vec::new();
        let mut completed_steps = Vec::new();

        for step in plan.steps() {
            let location =
                WorkflowLocation::new(&self.pack, &self.declaration, plan.name(), step.id());
            runtime.tick(location)?;
            let context = || workflow_context(&inputs, &self.config, &values, &outputs);
            let (value, output, step_warnings) = match step {
                PlanStep::Run(step) => {
                    if !self.evaluate_when(
                        step.when(),
                        context(),
                        runtime,
                        plan.name(),
                        step.id(),
                    )? {
                        (Value::Null, skipped_output(), Vec::new())
                    } else {
                        let bindings = resolve_plan_bindings(
                            step.bindings(),
                            &inputs,
                            &self.config,
                            &values,
                            runtime,
                            &self.pack,
                            &self.declaration,
                        )?;
                        self.execute_run_step(RunStepRequest {
                            ctx: &ctx,
                            workflow: plan.name(),
                            step: step.id(),
                            run: step.run().segments(),
                            binding_values: bindings,
                            warnings: &warnings,
                            completed_steps: &completed_steps,
                        })
                        .await?
                    }
                }
                PlanStep::Let(step) => {
                    let value = runtime.evaluate(location, "let", &context(), step.expr())?;
                    let output = json!({ "source": "expression", "value": value });
                    (value, output, Vec::new())
                }
                PlanStep::Assert(step) => {
                    let condition =
                        runtime.evaluate(location, "assert", &context(), step.condition())?;
                    match condition {
                        Value::Bool(true) => (
                            Value::Bool(true),
                            json!({ "source": "assert", "value": true }),
                            Vec::new(),
                        ),
                        Value::Bool(false) => {
                            return Err(workflow_error(
                                &self.pack,
                                &self.declaration,
                                format!(
                                    "workflow '{}' assert step '{}' failed: {}",
                                    plan.name(),
                                    step.id(),
                                    step.message()
                                ),
                            ));
                        }
                        value => {
                            return Err(workflow_error(
                                &self.pack,
                                &self.declaration,
                                format!(
                                    "workflow '{}' assert step '{}' returned {}, expected boolean",
                                    plan.name(),
                                    step.id(),
                                    workflow_json_type(&value)
                                ),
                            ));
                        }
                    }
                }
                PlanStep::Call(step) => {
                    if !self.evaluate_when(
                        step.when(),
                        context(),
                        runtime,
                        plan.name(),
                        step.id(),
                    )? {
                        (Value::Null, skipped_output(), Vec::new())
                    } else {
                        let bindings = resolve_plan_bindings(
                            step.bindings(),
                            &inputs,
                            &self.config,
                            &values,
                            runtime,
                            &self.pack,
                            &self.declaration,
                        )?;
                        let target = self.target_plan(step.call())?;
                        let target_inputs = target
                            .resolve_inputs(&Value::Object(bindings.into_iter().collect()))
                            .map_err(|message| {
                                workflow_error(
                                    &self.pack,
                                    &self.declaration,
                                    format!(
                                        "workflow '{}' call step '{}' inputs are invalid: {message}",
                                        plan.name(),
                                        step.id()
                                    ),
                                )
                            })?;
                        let execution = self
                            .execute_plan(ctx.clone(), target, target_inputs, runtime)
                            .await?;
                        (
                            execution.value.clone(),
                            json!({
                                "source": "workflow",
                                "workflow": step.call(),
                                "value": execution.value,
                                "output": execution.output,
                            }),
                            execution.warnings,
                        )
                    }
                }
                PlanStep::ForEach(step) => {
                    if !self.evaluate_when(
                        step.when(),
                        context(),
                        runtime,
                        plan.name(),
                        step.id(),
                    )? {
                        (Value::Null, skipped_output(), Vec::new())
                    } else {
                        let items = resolve_plan_binding(
                            step.items(),
                            &inputs,
                            &self.config,
                            &values,
                            runtime,
                            &self.pack,
                            &self.declaration,
                        )?
                        .ok_or_else(|| {
                            workflow_error(
                                &self.pack,
                                &self.declaration,
                                format!(
                                    "workflow '{}' for_each step '{}' items binding was absent",
                                    plan.name(),
                                    step.id()
                                ),
                            )
                        })?;
                        let items = items.as_array().ok_or_else(|| {
                            workflow_error(
                                &self.pack,
                                &self.declaration,
                                format!(
                                    "workflow '{}' for_each step '{}' expected an array, got {}",
                                    plan.name(),
                                    step.id(),
                                    workflow_json_type(&items)
                                ),
                            )
                        })?;
                        if items.len() > step.max_items()
                            || items.len() > runtime.limits.max_for_each_items()
                        {
                            return Err(workflow_error(
                                &self.pack,
                                &self.declaration,
                                format!(
                                    "workflow '{}' for_each step '{}' received {} items; declared max_items={}, host limit={}",
                                    plan.name(),
                                    step.id(),
                                    items.len(),
                                    step.max_items(),
                                    runtime.limits.max_for_each_items()
                                ),
                            ));
                        }
                        let target = self.target_plan(step.call())?;
                        let mut item_values = Vec::with_capacity(items.len());
                        let mut item_outputs = Vec::with_capacity(items.len());
                        let mut item_warnings = Vec::new();
                        for item in items {
                            let mut bindings = resolve_plan_bindings(
                                step.bindings(),
                                &inputs,
                                &self.config,
                                &values,
                                runtime,
                                &self.pack,
                                &self.declaration,
                            )?;
                            bindings.insert(step.item_name().to_string(), item.clone());
                            let target_inputs = target
                                .resolve_inputs(&Value::Object(bindings.into_iter().collect()))
                                .map_err(|message| {
                                    workflow_error(
                                        &self.pack,
                                        &self.declaration,
                                        format!(
                                            "workflow '{}' for_each step '{}' item inputs are invalid: {message}",
                                            plan.name(),
                                            step.id()
                                        ),
                                    )
                                })?;
                            let execution = self
                                .execute_plan(ctx.clone(), target.clone(), target_inputs, runtime)
                                .await?;
                            item_values.push(execution.value);
                            item_outputs.push(execution.output);
                            item_warnings.extend(execution.warnings);
                        }
                        let value = Value::Array(item_values);
                        (
                            value.clone(),
                            json!({
                                "source": "for_each",
                                "workflow": step.call(),
                                "value": value,
                                "outputs": item_outputs,
                            }),
                            item_warnings,
                        )
                    }
                }
            };

            runtime.charge_output(location, &value)?;
            warnings.extend(step_warnings);
            let id = step.id().to_string();
            completed_steps.push(id.clone());
            values.insert(id.clone(), value);
            outputs.insert(id, output);
        }

        let context = workflow_context(&inputs, &self.config, &values, &outputs);
        let result_location =
            WorkflowLocation::new(&self.pack, &self.declaration, plan.name(), "<result>");
        let value = runtime.evaluate(result_location, "result", &context, plan.result())?;
        runtime.charge_output(result_location, &value)?;
        let semantic = plan
            .output()
            .semantic_output(value.clone())
            .map_err(|message| {
                workflow_error(
                    &self.pack,
                    &self.declaration,
                    format!(
                        "workflow '{}' declared output contract failed: {message}",
                        plan.name()
                    ),
                )
            })?;
        Ok(WorkflowExecution {
            value,
            output: json!({
                "shape": workflow_semantic_shape_name(semantic.shape()),
                "value": semantic.value(),
                "columns": semantic.columns(),
            }),
            warnings,
        })
    }

    fn evaluate_when(
        &self,
        expression: Option<&str>,
        context: Value,
        runtime: &mut WorkflowRuntime,
        workflow: &str,
        step: &str,
    ) -> Result<bool, AppError> {
        let Some(expression) = expression else {
            return Ok(true);
        };
        let value = runtime.evaluate(
            WorkflowLocation::new(&self.pack, &self.declaration, workflow, step),
            "when",
            &context,
            expression,
        )?;
        value.as_bool().ok_or_else(|| {
            workflow_error(
                &self.pack,
                &self.declaration,
                format!(
                    "workflow '{workflow}' when expression on step '{step}' returned {}, expected boolean",
                    workflow_json_type(&value)
                ),
            )
        })
    }

    fn target_plan(&self, name: &str) -> Result<Arc<WorkflowPlan>, AppError> {
        self.program.plan(name).ok_or_else(|| {
            workflow_error(
                &self.pack,
                &self.declaration,
                format!("compiled workflow target '{name}' was unavailable"),
            )
        })
    }

    async fn execute_run_step(
        &self,
        request: RunStepRequest<'_>,
    ) -> Result<(Value, Value, Vec<String>), AppError> {
        let RunStepRequest {
            ctx,
            workflow,
            step,
            run,
            binding_values,
            warnings,
            completed_steps,
        } = request;
        let (handler, command_path, command_index, arguments) = {
            let catalog = ctx.catalog.snapshot();
            let resolved = catalog.resolve_command(&[], run)?;
            let arguments =
                workflow_step_arguments(resolved.command, &binding_values).map_err(|message| {
                    workflow_error(
                        &self.pack,
                        &self.declaration,
                        format!("workflow '{workflow}' step '{step}': {message}"),
                    )
                    .with_warnings(workflow_progress_warnings(warnings, completed_steps))
                })?;
            (
                resolved.command.handler.clone(),
                resolved.command_path,
                resolved.command_index,
                arguments,
            )
        };
        let mut step_tokens = run.to_vec();
        step_tokens.extend(["--output".to_string(), "json".to_string()]);
        step_tokens.extend(arguments);
        let raw_line = step_tokens
            .iter()
            .map(|token| shell_escape(token))
            .collect::<Vec<_>>()
            .join(" ");
        let step_result = if is_offline_builtin_command(run) {
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
        let outcome = step_result.map_err(|error| AppError::ExtensionWorkflowStep {
            pack: self.pack.clone(),
            workflow: workflow.to_string(),
            step: step.to_string(),
            command: run.join(" "),
            source: Box::new(error),
        })?;
        if outcome.redirect.is_some() || outcome.scope_action != ScopeAction::None {
            return Err(workflow_error(
                &self.pack,
                &self.declaration,
                format!(
                    "workflow '{workflow}' step '{step}' attempted an unsupported redirect or scope change"
                ),
            ));
        }
        if outcome.output.next_page_command.is_some() {
            return Err(workflow_error(
                &self.pack,
                &self.declaration,
                format!(
                    "workflow '{workflow}' step '{step}' returned a partial page; bind 'all = true' or an explicit pagination policy"
                ),
            ));
        }
        if !outcome.output.errors.is_empty() {
            return Err(workflow_error(
                &self.pack,
                &self.declaration,
                format!(
                    "workflow '{workflow}' step '{step}' returned errors: {}",
                    outcome.output.errors.join("; ")
                ),
            ));
        }
        let mut output = outcome.output;
        let step_warnings = mem::take(&mut output.warnings);
        let (value, metadata) = workflow_output_capture(output.semantic, output.lines);
        Ok((value, metadata, step_warnings))
    }
}

struct WorkflowExecution {
    value: Value,
    output: Value,
    warnings: Vec<String>,
}

struct RunStepRequest<'a> {
    ctx: &'a CommandContext,
    workflow: &'a str,
    step: &'a str,
    run: &'a [String],
    binding_values: BTreeMap<String, Value>,
    warnings: &'a [String],
    completed_steps: &'a [String],
}

#[derive(Clone, Copy)]
struct WorkflowLocation<'a> {
    pack: &'a str,
    declaration: &'a CommandDeclaration,
    workflow: &'a str,
    step: &'a str,
}

impl<'a> WorkflowLocation<'a> {
    fn new(
        pack: &'a str,
        declaration: &'a CommandDeclaration,
        workflow: &'a str,
        step: &'a str,
    ) -> Self {
        Self {
            pack,
            declaration,
            workflow,
            step,
        }
    }

    fn error(self, message: impl Into<String>) -> AppError {
        workflow_error(self.pack, self.declaration, message.into())
    }
}

struct WorkflowRuntime {
    limits: crate::extensions::WorkflowLimits,
    operations: usize,
    call_depth: usize,
    output_bytes: usize,
}

impl WorkflowRuntime {
    fn new(limits: crate::extensions::WorkflowLimits) -> Self {
        Self {
            limits,
            operations: 0,
            call_depth: 0,
            output_bytes: 0,
        }
    }

    fn enter_call(&mut self, location: WorkflowLocation<'_>) -> Result<(), AppError> {
        self.call_depth = self.call_depth.saturating_add(1);
        if self.call_depth > self.limits.max_call_depth() {
            return Err(location.error(format!(
                "workflow '{}' exceeded call depth limit {}",
                location.workflow,
                self.limits.max_call_depth()
            )));
        }
        Ok(())
    }

    fn leave_call(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn tick(&mut self, location: WorkflowLocation<'_>) -> Result<(), AppError> {
        self.operations = self.operations.saturating_add(1);
        if self.operations > self.limits.max_operations() {
            return Err(location.error(format!(
                "workflow '{}' step '{}' exceeded operation limit {}",
                location.workflow,
                location.step,
                self.limits.max_operations()
            )));
        }
        Ok(())
    }

    fn evaluate(
        &mut self,
        location: WorkflowLocation<'_>,
        label: &str,
        input: &Value,
        expression: &str,
    ) -> Result<Value, AppError> {
        self.tick(location)?;
        evaluate_bounded_jq(input, expression, self.limits.jq()).map_err(|error| {
            location.error(format!(
                "workflow '{}' step '{}' {label} expression failed: {error}",
                location.workflow, location.step
            ))
        })
    }

    fn charge_output(
        &mut self,
        location: WorkflowLocation<'_>,
        value: &Value,
    ) -> Result<(), AppError> {
        let bytes = serde_json::to_vec(value)
            .map_err(|error| location.error(error.to_string()))?
            .len();
        self.output_bytes = self.output_bytes.saturating_add(bytes);
        if self.output_bytes > self.limits.max_output_bytes() {
            return Err(location.error(format!(
                "workflow '{}' step '{}' exceeded cumulative output limit {} bytes",
                location.workflow,
                location.step,
                self.limits.max_output_bytes()
            )));
        }
        Ok(())
    }
}

fn workflow_output_capture(semantic: Vec<OutputEnvelope>, lines: Vec<String>) -> (Value, Value) {
    let has_semantic = !semantic.is_empty();
    let metadata = if !has_semantic {
        serde_json::json!({
            "source": "rendered",
            "shape": "lines",
            "columns": [],
            "lines": lines,
        })
    } else {
        Value::Array(
            semantic
                .iter()
                .map(|envelope| {
                    serde_json::json!({
                        "shape": workflow_output_shape_name(envelope.shape()),
                        "value": envelope.value(),
                        "columns": envelope.columns(),
                    })
                })
                .collect(),
        )
    };
    let value = match semantic.len() {
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
        1 => semantic
            .into_iter()
            .next()
            .expect("one semantic value was checked")
            .into_value(),
        _ => Value::Array(
            semantic
                .into_iter()
                .map(OutputEnvelope::into_value)
                .collect(),
        ),
    };
    let output = if !has_semantic {
        let mut metadata = metadata
            .as_object()
            .expect("rendered output metadata is an object")
            .clone();
        metadata.insert("value".to_string(), value.clone());
        Value::Object(metadata)
    } else {
        serde_json::json!({
            "source": "semantic",
            "value": value,
            "envelopes": metadata,
        })
    };
    (value, output)
}

fn workflow_output_shape_name(shape: OutputShape) -> &'static str {
    match shape {
        OutputShape::Empty => "empty",
        OutputShape::Lines => "lines",
        OutputShape::Rows => "rows",
        OutputShape::Detail => "detail",
        OutputShape::Message => "message",
        OutputShape::Values => "values",
        OutputShape::Groups => "groups",
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

fn resolve_plan_bindings(
    bindings: &BTreeMap<String, PlanBinding>,
    inputs: &Map<String, Value>,
    config: &Value,
    step_values: &Map<String, Value>,
    runtime: &mut WorkflowRuntime,
    pack: &str,
    declaration: &CommandDeclaration,
) -> Result<BTreeMap<String, Value>, AppError> {
    let mut resolved = BTreeMap::new();
    for (name, binding) in bindings {
        if let Some(value) = resolve_plan_binding(
            binding,
            inputs,
            config,
            step_values,
            runtime,
            pack,
            declaration,
        )? {
            resolved.insert(name.clone(), value);
        }
    }
    Ok(resolved)
}

fn resolve_plan_binding(
    binding: &PlanBinding,
    inputs: &Map<String, Value>,
    config: &Value,
    step_values: &Map<String, Value>,
    runtime: &mut WorkflowRuntime,
    pack: &str,
    declaration: &CommandDeclaration,
) -> Result<Option<Value>, AppError> {
    match binding {
        PlanBinding::Literal(value) => Ok(Some(value.clone())),
        PlanBinding::Input(name) => Ok(inputs.get(name).cloned()),
        PlanBinding::Config(key) => Ok(config
            .as_object()
            .and_then(|config| config.get(key))
            .filter(|value| !value.is_null())
            .cloned()),
        PlanBinding::Step { step, select } => {
            let source = step_values.get(step).ok_or_else(|| {
                workflow_error(
                    pack,
                    declaration,
                    format!("compiled step output '{step}' was unavailable"),
                )
            })?;
            let value = if let Some(select) = select {
                runtime.evaluate(
                    WorkflowLocation::new(pack, declaration, "<binding>", step),
                    "select",
                    source,
                    select,
                )?
            } else {
                source.clone()
            };
            Ok(Some(value))
        }
    }
}

fn workflow_context(
    inputs: &Map<String, Value>,
    config: &Value,
    steps: &Map<String, Value>,
    outputs: &Map<String, Value>,
) -> Value {
    json!({
        "input": inputs,
        "config": config,
        "steps": steps,
        "outputs": outputs,
    })
}

fn skipped_output() -> Value {
    json!({ "source": "skipped", "skipped": true, "value": null })
}

fn workflow_semantic_shape_name(shape: SemanticOutputShape) -> &'static str {
    match shape {
        SemanticOutputShape::Empty => "empty",
        SemanticOutputShape::Lines => "lines",
        SemanticOutputShape::Rows => "rows",
        SemanticOutputShape::Detail => "detail",
        SemanticOutputShape::Message => "message",
        SemanticOutputShape::Values => "values",
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
            Some(Value::Bool(!raw_values.is_empty()))
        } else if raw_values.is_empty() {
            None
        } else if option.repeatable() {
            Some(Value::Array(
                raw_values
                    .iter()
                    .map(|value| workflow_typed_input(option, value))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        } else {
            Some(
                raw_values
                    .first()
                    .map(|value| workflow_typed_input(option, value))
                    .transpose()?
                    .expect("non-empty raw values have a first value"),
            )
        };
        if let Some(value) = value {
            inputs.insert(option.name().to_string(), value);
        }
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
        OptionKind::Json => from_str(value)
            .map_err(|error| format!("input '{}' is not JSON: {error}", option.name())),
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
        CatalogStore, CommandCatalogBuilder, CommandContext, CommandEffects, CommandInvocation,
        CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
    };
    use crate::config::get_config;
    use crate::errors::{AppError, ReauthenticationRetry};
    use crate::extensions::WorkflowProgram;
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
            r#"{
  "schema_version": 1,
  "kind": "executable",
  "name": "demo",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "protocol": "hubuum-cli.extension/v1",
  "executable": "bin/demo",
  "commands": {
    "inventory_list": {
      "path": ["inventory", "list"],
      "options": {
        "target": { "kind": "string", "position": 1 },
        "state": { "kind": "string", "long": "state" }
      }
    }
  }
}"#,
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
            r#"{
  "schema_version": 1,
  "kind": "executable",
  "name": "demo",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "protocol": "hubuum-cli.extension/v1",
  "executable": "bin/demo",
  "commands": {
    "run": {
      "path": ["run"],
      "options": {
        "verbose": { "kind": "flag", "long": "verbose" },
        "tag": { "kind": "string", "long": "tag", "repeatable": true }
      }
    }
  }
}"#,
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
        let command = CommandSpec::new(
            "demo",
            vec![
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
            ReauthenticationRetry::Safe,
            CommandEffects::ReadOnly,
            Arc::new(EchoAction),
        );
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
            r#"{
  "schema_version": 1,
  "kind": "portable",
  "name": "demo",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "workflows": {
    "snapshot": {
      "inputs": {
        "target": { "type": "integer", "required": true }
      },
      "output": { "shape": "detail", "type": "json" },
      "steps": [
        { "id": "items", "kind": "run", "run": ["object", "list"] },
        {
          "id": "selected",
          "kind": "run",
          "run": ["object", "echo"],
          "with": { "id": { "step": "items", "select": ".[0].id" } }
        },
        {
          "id": "input_echo",
          "kind": "run",
          "run": ["object", "echo"],
          "with": { "id": { "input": "target" } }
        },
        { "id": "legacy", "kind": "run", "run": ["object", "legacy"] }
      ],
      "result": "{ requested: .input.target, items: .steps.items, selected: .steps.selected, input_echo: .steps.input_echo, legacy: .steps.legacy, items_output: .outputs.items, legacy_output: .outputs.legacy }"
    }
  },
  "commands": {
    "snapshot": {
      "path": ["snapshot"],
      "workflow": "snapshot",
      "options": {
        "target": { "kind": "integer", "position": 1, "required": true }
      }
    }
  }
}"#,
        )
        .expect("workflow manifest");
        let declaration = manifest.commands()[0].clone();
        let mut builder = CommandCatalogBuilder::new();
        builder.add_command(
            &["object"],
            CommandSpec::new(
                "list",
                Vec::new(),
                ReauthenticationRetry::Safe,
                CommandEffects::ReadOnly,
                Arc::new(SemanticAction),
            ),
        );
        let mut id_option = catalog_option("id", Some("--id"), true, false, false, None);
        id_option.field_type_help = "integer".to_string();
        id_option.field_type = TypeId::of::<i64>();
        builder.add_command(
            &["object"],
            CommandSpec::new(
                "echo",
                vec![id_option],
                ReauthenticationRetry::Safe,
                CommandEffects::ReadOnly,
                Arc::new(EchoAction),
            ),
        );
        builder.add_command(
            &["object"],
            CommandSpec::new(
                "legacy",
                Vec::new(),
                ReauthenticationRetry::Safe,
                CommandEffects::ReadOnly,
                Arc::new(RenderedJsonAction),
            ),
        );
        let program = Arc::new(
            WorkflowProgram::compile(&manifest, &json!({}), |path| builder.command(path))
                .expect("workflow plan"),
        );
        let handler = WorkflowCommandHandler {
            pack: "demo".to_string(),
            plan: program.plan("snapshot").expect("snapshot plan"),
            program,
            declaration,
            config: json!({}),
        };
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
            outcome.output.semantic[0].value(),
            &json!({
                "requested": 7,
                "items": [{"id": 1}],
                "selected": "object echo --output json --id=1",
                "input_echo": "object echo --output json --id=7",
                "legacy": {"legacy": true},
                "items_output": {
                    "source": "semantic",
                    "value": [{"id": 1}],
                    "envelopes": [{
                        "shape": "rows",
                        "value": [{"id": 1}],
                        "columns": ["id"]
                    }]
                },
                "legacy_output": {
                    "source": "rendered",
                    "shape": "lines",
                    "columns": [],
                    "lines": ["{", "  \"legacy\": true", "}"],
                    "value": {"legacy": true}
                }
            })
        );
    }
}
