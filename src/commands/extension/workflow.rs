use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::{future::Future, mem};

use async_trait::async_trait;
use hubuum_extension_protocol::{CommandDeclaration, SemanticOutputShape};
use hubuum_filter::{evaluate_bounded_jq, OutputEnvelope, OutputShape};
use serde_json::{from_str, json, Map, Value};

use crate::catalog::{
    AsyncCommandHandler, CommandContext, CommandInvocation, CommandOutcome, ScopeAction,
};
use crate::command_line::shell_escape;
use crate::commands::{render_format, table_headers};
use crate::errors::AppError;
use crate::extensions::{
    PlanAssertStep, PlanBinding, PlanCallStep, PlanForEachStep, PlanStep, WorkflowLimits,
    WorkflowPlan, WorkflowProgram,
};
use crate::output::{
    add_warning, reset_output, set_pipeline, set_pipeline_suffix, set_render_format,
    set_semantic_output, set_table_headers, take_output,
};
use crate::tokenizer::CommandTokenizer;

use super::{
    convert_output, extension_cli_options, validate_invocation, workflow_error,
    workflow_input_values, workflow_json_type, workflow_step_arguments,
};
#[derive(Clone)]
pub(super) struct WorkflowCommandHandler {
    pub(super) pack: String,
    pub(super) declaration: CommandDeclaration,
    pub(super) program: Arc<WorkflowProgram>,
    pub(super) plan: Arc<WorkflowPlan>,
    pub(super) config: Value,
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
        let mut frame = WorkflowFrame::new(inputs);
        for step in plan.steps() {
            let location =
                WorkflowLocation::new(&self.pack, &self.declaration, plan.name(), step.id());
            runtime.tick(location)?;
            let context = StepContext {
                command: &ctx,
                location,
                frame: &frame,
            };
            let execution = self.execute_step(step, context, runtime).await?;
            runtime.charge_output(location, &execution.value)?;
            frame.record(step.id(), execution);
        }
        let location =
            WorkflowLocation::new(&self.pack, &self.declaration, plan.name(), "<result>");
        let value = runtime.evaluate(
            location,
            "result",
            &frame.context(&self.config),
            plan.result(),
        )?;
        runtime.charge_output(location, &value)?;
        let semantic = plan
            .output()
            .semantic_output(value.clone())
            .map_err(|message| {
                location.error(format!(
                    "workflow '{}' declared output contract failed: {message}",
                    plan.name()
                ))
            })?;
        Ok(WorkflowExecution {
            value,
            output: json!({
                "shape": workflow_semantic_shape_name(semantic.shape()),
                "value": semantic.value(),
                "columns": semantic.columns(),
            }),
            warnings: frame.warnings,
        })
    }

    async fn execute_step(
        &self,
        step: &PlanStep,
        context: StepContext<'_>,
        runtime: &mut WorkflowRuntime,
    ) -> Result<WorkflowExecution, AppError> {
        let when = match step {
            PlanStep::Run(step) => step.when(),
            PlanStep::Call(step) => step.when(),
            PlanStep::ForEach(step) => step.when(),
            PlanStep::Let(_) | PlanStep::Assert(_) => None,
        };
        if !self.evaluate_when(when, context, runtime)? {
            return Ok(WorkflowExecution::new(Value::Null, skipped_output()));
        }
        let location = context.location;
        let frame = context.frame;
        match step {
            PlanStep::Run(step) => {
                let bindings =
                    frame.resolve_bindings(step.bindings(), &self.config, runtime, location)?;
                self.execute_run_step(RunStepRequest {
                    ctx: context.command,
                    workflow: location.workflow,
                    step: step.id(),
                    run: step.run().segments(),
                    binding_values: bindings,
                    warnings: &frame.warnings,
                    completed_steps: &frame.completed_steps,
                })
                .await
            }
            PlanStep::Let(step) => {
                let value =
                    runtime.evaluate(location, "let", &frame.context(&self.config), step.expr())?;
                let output = json!({ "source": "expression", "value": value });
                Ok(WorkflowExecution::new(value, output))
            }
            PlanStep::Assert(step) => self.execute_assert_step(step, context, runtime),
            PlanStep::Call(step) => self.execute_call_step(step, context, runtime).await,
            PlanStep::ForEach(step) => self.execute_for_each_step(step, context, runtime).await,
        }
    }

    fn execute_assert_step(
        &self,
        step: &PlanAssertStep,
        context: StepContext<'_>,
        runtime: &mut WorkflowRuntime,
    ) -> Result<WorkflowExecution, AppError> {
        let location = context.location;
        let condition = runtime.evaluate(
            location,
            "assert",
            &context.frame.context(&self.config),
            step.condition(),
        )?;
        match condition {
            Value::Bool(true) => Ok(WorkflowExecution::new(
                Value::Bool(true),
                json!({ "source": "assert", "value": true }),
            )),
            Value::Bool(false) => Err(location.error(format!(
                "workflow '{}' assert step '{}' failed: {}",
                location.workflow,
                step.id(),
                step.message()
            ))),
            value => Err(location.error(format!(
                "workflow '{}' assert step '{}' returned {}, expected boolean",
                location.workflow,
                step.id(),
                workflow_json_type(&value)
            ))),
        }
    }

    async fn execute_call_step(
        &self,
        step: &PlanCallStep,
        context: StepContext<'_>,
        runtime: &mut WorkflowRuntime,
    ) -> Result<WorkflowExecution, AppError> {
        let bindings = context.frame.resolve_bindings(
            step.bindings(),
            &self.config,
            runtime,
            context.location,
        )?;
        let target = self.target_plan(step.call())?;
        let inputs = target
            .resolve_inputs(&Value::Object(bindings.into_iter().collect()))
            .map_err(|message| {
                context.location.error(format!(
                    "workflow '{}' call step '{}' inputs are invalid: {message}",
                    context.location.workflow,
                    step.id()
                ))
            })?;
        let execution = self
            .execute_plan(context.command.clone(), target, inputs, runtime)
            .await?;
        Ok(WorkflowExecution {
            value: execution.value.clone(),
            output: json!({ "source": "workflow", "workflow": step.call(), "value": execution.value, "output": execution.output }),
            warnings: execution.warnings,
        })
    }

    async fn execute_for_each_step(
        &self,
        step: &PlanForEachStep,
        context: StepContext<'_>,
        runtime: &mut WorkflowRuntime,
    ) -> Result<WorkflowExecution, AppError> {
        let location = context.location;
        let frame = context.frame;
        let items = frame
            .resolve_binding(step.items(), &self.config, runtime, location)?
            .ok_or_else(|| {
                location.error(format!(
                    "workflow '{}' for_each step '{}' items binding was absent",
                    location.workflow,
                    step.id()
                ))
            })?;
        let items = items.as_array().ok_or_else(|| {
            location.error(format!(
                "workflow '{}' for_each step '{}' expected an array, got {}",
                location.workflow,
                step.id(),
                workflow_json_type(&items)
            ))
        })?;
        if items.len() > step.max_items() || items.len() > runtime.limits.max_for_each_items() {
            return Err(location.error(format!(
                "workflow '{}' for_each step '{}' received {} items; declared max_items={}, host limit={}",
                location.workflow, step.id(), items.len(), step.max_items(), runtime.limits.max_for_each_items()
            )));
        }
        let target = self.target_plan(step.call())?;
        let mut values = Vec::with_capacity(items.len());
        let mut outputs = Vec::with_capacity(items.len());
        let mut warnings = Vec::new();
        for item in items {
            let mut bindings =
                frame.resolve_bindings(step.bindings(), &self.config, runtime, location)?;
            bindings.insert(step.item_name().to_string(), item.clone());
            let inputs = target
                .resolve_inputs(&Value::Object(bindings.into_iter().collect()))
                .map_err(|message| {
                    location.error(format!(
                        "workflow '{}' for_each step '{}' item inputs are invalid: {message}",
                        location.workflow,
                        step.id()
                    ))
                })?;
            let execution = self
                .execute_plan(context.command.clone(), target.clone(), inputs, runtime)
                .await?;
            values.push(execution.value);
            outputs.push(execution.output);
            warnings.extend(execution.warnings);
        }
        let value = Value::Array(values);
        Ok(WorkflowExecution {
            value: value.clone(),
            output: json!({ "source": "for_each", "workflow": step.call(), "value": value, "outputs": outputs }),
            warnings,
        })
    }

    fn evaluate_when(
        &self,
        expression: Option<&str>,
        context: StepContext<'_>,
        runtime: &mut WorkflowRuntime,
    ) -> Result<bool, AppError> {
        let Some(expression) = expression else {
            return Ok(true);
        };
        let location = context.location;
        let value = runtime.evaluate(
            location,
            "when",
            &context.frame.context(&self.config),
            expression,
        )?;
        value.as_bool().ok_or_else(|| {
            location.error(format!(
                "workflow '{}' when expression on step '{}' returned {}, expected boolean",
                location.workflow,
                location.step,
                workflow_json_type(&value)
            ))
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
    ) -> Result<WorkflowExecution, AppError> {
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
        let mut step_ctx = ctx.clone();
        if !handler.requires_authentication() {
            step_ctx.services = None;
        }
        let step_result = handler
            .execute(
                step_ctx,
                CommandInvocation {
                    stream_output: false,
                    raw_line,
                    command_index,
                    command_path,
                    pipeline: Vec::new(),
                    pipeline_suffix: None,
                },
            )
            .await;
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
        Ok(WorkflowExecution {
            value,
            output: metadata,
            warnings: step_warnings,
        })
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
    limits: WorkflowLimits,
    operations: usize,
    call_depth: usize,
    output_bytes: usize,
}

impl WorkflowRuntime {
    fn new(limits: WorkflowLimits) -> Self {
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

#[derive(Clone, Copy)]
struct StepContext<'a> {
    command: &'a CommandContext,
    location: WorkflowLocation<'a>,
    frame: &'a WorkflowFrame,
}

struct WorkflowFrame {
    inputs: Map<String, Value>,
    values: Map<String, Value>,
    outputs: Map<String, Value>,
    warnings: Vec<String>,
    completed_steps: Vec<String>,
}

impl WorkflowFrame {
    fn new(inputs: Map<String, Value>) -> Self {
        Self {
            inputs,
            values: Map::new(),
            outputs: Map::new(),
            warnings: Vec::new(),
            completed_steps: Vec::new(),
        }
    }

    fn record(&mut self, step: &str, execution: WorkflowExecution) {
        self.warnings.extend(execution.warnings);
        self.completed_steps.push(step.to_string());
        self.values.insert(step.to_string(), execution.value);
        self.outputs.insert(step.to_string(), execution.output);
    }

    fn context(&self, config: &Value) -> Value {
        json!({ "input": self.inputs, "config": config, "steps": self.values, "outputs": self.outputs })
    }

    fn resolve_bindings(
        &self,
        bindings: &BTreeMap<String, PlanBinding>,
        config: &Value,
        runtime: &mut WorkflowRuntime,
        location: WorkflowLocation<'_>,
    ) -> Result<BTreeMap<String, Value>, AppError> {
        let mut resolved = BTreeMap::new();
        for (name, binding) in bindings {
            if let Some(value) = self.resolve_binding(binding, config, runtime, location)? {
                resolved.insert(name.clone(), value);
            }
        }
        Ok(resolved)
    }

    fn resolve_binding(
        &self,
        binding: &PlanBinding,
        config: &Value,
        runtime: &mut WorkflowRuntime,
        location: WorkflowLocation<'_>,
    ) -> Result<Option<Value>, AppError> {
        match binding {
            PlanBinding::Literal(value) => Ok(Some(value.clone())),
            PlanBinding::Input(name) => Ok(self.inputs.get(name).cloned()),
            PlanBinding::Config(key) => Ok(config
                .as_object()
                .and_then(|config| config.get(key))
                .filter(|value| !value.is_null())
                .cloned()),
            PlanBinding::Step { step, select } => {
                let source = self.values.get(step).ok_or_else(|| {
                    location.error(format!("compiled step output '{step}' was unavailable"))
                })?;
                let value = if let Some(select) = select {
                    runtime.evaluate(
                        WorkflowLocation {
                            workflow: "<binding>",
                            step,
                            ..location
                        },
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
}

impl WorkflowExecution {
    fn new(value: Value, output: Value) -> Self {
        Self {
            value,
            output,
            warnings: Vec::new(),
        }
    }
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
