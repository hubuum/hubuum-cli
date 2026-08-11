use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use hubuum_extension_protocol::{
    CommandPath, ConfigDeclaration, ExtensionManifest, SemanticOutput, SemanticOutputShape,
    WorkflowBinding, WorkflowBindingName, WorkflowDeclaration, WorkflowInputDeclaration,
    WorkflowName, WorkflowOutputDeclaration, WorkflowStep, WorkflowStepId,
    WorkflowValueType as DeclaredValueType,
};
use hubuum_filter::{validate_bounded_jq_expression, JqLimits};
use serde_json::{json, Map, Value};

use crate::catalog::{
    CommandEffects, CommandSpec, WorkflowCardinality, WorkflowInputContract,
    WorkflowValueType as CommandValueType,
};
use crate::dispatch::is_offline_builtin_command;
use crate::errors::ReauthenticationRetry;

pub const MAX_WORKFLOW_CALL_DEPTH: usize = 16;
pub const MAX_WORKFLOW_OPERATIONS: usize = 10_000;
pub const MAX_FOR_EACH_ITEMS: usize = 1_000;
pub const MAX_WORKFLOW_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_JQ_EXPRESSION_BYTES: usize = 4 * 1024;
pub const MAX_JQ_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_JQ_OUTPUTS: usize = 128;
pub const MAX_JQ_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct WorkflowLimits {
    max_call_depth: usize,
    max_operations: usize,
    max_for_each_items: usize,
    max_output_bytes: usize,
    jq: JqLimits,
}

impl Default for WorkflowLimits {
    fn default() -> Self {
        Self {
            max_call_depth: MAX_WORKFLOW_CALL_DEPTH,
            max_operations: MAX_WORKFLOW_OPERATIONS,
            max_for_each_items: MAX_FOR_EACH_ITEMS,
            max_output_bytes: MAX_WORKFLOW_OUTPUT_BYTES,
            jq: JqLimits::new(
                MAX_JQ_EXPRESSION_BYTES,
                MAX_JQ_INPUT_BYTES,
                MAX_JQ_OUTPUTS,
                MAX_JQ_OUTPUT_BYTES,
            ),
        }
    }
}

impl WorkflowLimits {
    pub fn max_call_depth(self) -> usize {
        self.max_call_depth
    }

    pub fn max_operations(self) -> usize {
        self.max_operations
    }

    pub fn max_for_each_items(self) -> usize {
        self.max_for_each_items
    }

    pub fn max_output_bytes(self) -> usize {
        self.max_output_bytes
    }

    pub fn jq(self) -> JqLimits {
        self.jq
    }

    fn explain_value(self) -> Value {
        json!({
            "call_depth": self.max_call_depth,
            "operations": self.max_operations,
            "for_each_items": self.max_for_each_items,
            "output_bytes": self.max_output_bytes,
            "jq": {
                "expression_bytes": MAX_JQ_EXPRESSION_BYTES,
                "input_bytes": MAX_JQ_INPUT_BYTES,
                "outputs": MAX_JQ_OUTPUTS,
                "output_bytes": MAX_JQ_OUTPUT_BYTES,
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowProgram {
    plans: BTreeMap<String, Arc<WorkflowPlan>>,
    limits: WorkflowLimits,
}

impl WorkflowProgram {
    pub fn compile<'a, F>(
        manifest: &ExtensionManifest,
        config: &Value,
        resolve_command: F,
    ) -> Result<Self, String>
    where
        F: Fn(&[String]) -> Option<&'a CommandSpec>,
    {
        if !manifest.is_portable() {
            return Ok(Self {
                plans: BTreeMap::new(),
                limits: WorkflowLimits::default(),
            });
        }
        let limits = WorkflowLimits::default();
        let mut drafts = BTreeMap::new();
        for workflow in manifest.workflows().values() {
            let draft = compile_workflow(
                workflow,
                manifest.config(),
                config,
                &resolve_command,
                limits,
            )?;
            drafts.insert(workflow.name().as_str().to_string(), draft);
        }

        let mut analyses = HashMap::new();
        for name in drafts.keys() {
            analyze_workflow(name, &drafts, &mut Vec::new(), &mut analyses, limits)?;
        }

        let plans = drafts
            .into_iter()
            .map(|(name, draft)| {
                let analysis = analyses
                    .remove(&name)
                    .expect("every workflow draft was analyzed");
                if analysis.effects.may_mutate() && !draft.allows_mutation {
                    return Err(format!(
                        "workflow '{name}' may change state through its expanded call graph; declare capabilities = [\"mutate\"]"
                    ));
                }
                Ok((
                    name.clone(),
                    Arc::new(WorkflowPlan {
                        name,
                        inputs: draft.inputs,
                        output: draft.output,
                        steps: draft.steps,
                        result: draft.result,
                        effects: analysis.effects,
                        reauthentication_retry: analysis.reauthentication_retry,
                        requires_authentication: analysis.requires_authentication,
                        worst_case_operations: analysis.operations,
                        call_depth: analysis.call_depth,
                        limits,
                    }),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Self { plans, limits })
    }

    pub fn plan(&self, name: &str) -> Option<Arc<WorkflowPlan>> {
        self.plans.get(name).cloned()
    }

    pub fn plans(&self) -> impl Iterator<Item = &WorkflowPlan> {
        self.plans.values().map(Arc::as_ref)
    }

    pub fn limits(&self) -> WorkflowLimits {
        self.limits
    }

    pub fn explain_value(&self, workflow: Option<&str>) -> Result<Value, String> {
        let workflows = if let Some(name) = workflow {
            let plan = self
                .plans
                .get(name)
                .ok_or_else(|| format!("workflow '{name}' was not found"))?;
            vec![plan.explain_value()]
        } else {
            self.plans
                .values()
                .map(|plan| plan.explain_value())
                .collect()
        };
        Ok(json!({
            "limits": self.limits().explain_value(),
            "workflow_count": self.plans().count(),
            "workflows": workflows,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowPlan {
    name: String,
    inputs: Vec<PlanInput>,
    output: PlanOutput,
    steps: Vec<PlanStep>,
    result: String,
    effects: CommandEffects,
    reauthentication_retry: ReauthenticationRetry,
    requires_authentication: bool,
    worst_case_operations: usize,
    call_depth: usize,
    limits: WorkflowLimits,
}

impl WorkflowPlan {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn inputs(&self) -> &[PlanInput] {
        &self.inputs
    }

    pub fn output(&self) -> &PlanOutput {
        &self.output
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    pub fn result(&self) -> &str {
        &self.result
    }

    pub fn effects(&self) -> CommandEffects {
        self.effects
    }

    pub fn reauthentication_retry(&self) -> ReauthenticationRetry {
        self.reauthentication_retry
    }

    pub fn requires_authentication(&self) -> bool {
        self.requires_authentication
    }

    pub fn worst_case_operations(&self) -> usize {
        self.worst_case_operations
    }

    pub fn call_depth(&self) -> usize {
        self.call_depth
    }

    pub fn limits(&self) -> WorkflowLimits {
        self.limits
    }

    pub fn resolve_inputs(&self, supplied: &Value) -> Result<Map<String, Value>, String> {
        let supplied = supplied
            .as_object()
            .ok_or_else(|| "workflow inputs must be an object".to_string())?;
        if let Some(name) = supplied
            .keys()
            .find(|name| !self.inputs.iter().any(|input| input.name() == *name))
        {
            return Err(format!("unknown workflow input '{name}'"));
        }
        let mut resolved = Map::new();
        for input in &self.inputs {
            let value = supplied
                .get(input.name())
                .cloned()
                .or_else(|| input.default().cloned());
            match value {
                Some(value) => {
                    input.validate(&value)?;
                    resolved.insert(input.name().to_string(), value);
                }
                None if input.required() => {
                    return Err(format!("required input '{}' is missing", input.name()));
                }
                None => {}
            }
        }
        Ok(resolved)
    }

    fn explain_value(&self) -> Value {
        json!({
            "name": self.name,
            "inputs": self.inputs().iter().map(PlanInput::explain_value).collect::<Vec<_>>(),
            "output": self.output.explain_value(),
            "steps": self.steps.iter().map(PlanStep::explain_value).collect::<Vec<_>>(),
            "effects": if self.effects.may_mutate() { "mutating" } else { "read_only" },
            "reauthentication_retry": if self.reauthentication_retry == ReauthenticationRetry::Safe { "safe" } else { "unsafe" },
            "requires_authentication": self.requires_authentication,
            "worst_case_operations": self.worst_case_operations(),
            "call_depth": self.call_depth(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PlanInput {
    name: String,
    value_type: DeclaredValueType,
    required: bool,
    repeatable: bool,
    default: Option<Value>,
}

impl PlanInput {
    fn from_declaration(input: &WorkflowInputDeclaration) -> Self {
        Self {
            name: input.name().to_string(),
            value_type: input.value_type(),
            required: input.required(),
            repeatable: input.repeatable(),
            default: input.default().cloned(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value_type(&self) -> DeclaredValueType {
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

    fn validate(&self, value: &Value) -> Result<(), String> {
        let valid = if self.repeatable {
            value
                .as_array()
                .is_some_and(|values| values.iter().all(|value| self.value_type.accepts(value)))
        } else {
            self.value_type.accepts(value)
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "input '{}' must have type '{}'{}",
                self.name,
                self.value_type.as_str(),
                if self.repeatable { "[]" } else { "" }
            ))
        }
    }

    fn explain_value(&self) -> Value {
        json!({
            "name": self.name,
            "type": self.value_type().as_str(),
            "required": self.required,
            "repeatable": self.repeatable(),
            "default": self.default,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PlanOutput {
    shape: SemanticOutputShape,
    value_type: DeclaredValueType,
    columns: Vec<String>,
}

impl PlanOutput {
    fn from_declaration(output: &WorkflowOutputDeclaration) -> Self {
        Self {
            shape: output.shape(),
            value_type: output.value_type(),
            columns: output.columns().to_vec(),
        }
    }

    pub fn shape(&self) -> SemanticOutputShape {
        self.shape
    }

    pub fn value_type(&self) -> DeclaredValueType {
        self.value_type
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn semantic_output(&self, value: Value) -> Result<SemanticOutput, String> {
        let output = SemanticOutput::new(self.shape, value, self.columns.clone())
            .map_err(|error| error.to_string())?;
        let typed_values: Vec<&Value> = match self.shape {
            SemanticOutputShape::Values | SemanticOutputShape::Lines => {
                output.value().as_array().into_iter().flatten().collect()
            }
            SemanticOutputShape::Empty => Vec::new(),
            SemanticOutputShape::Rows
            | SemanticOutputShape::Detail
            | SemanticOutputShape::Message => vec![output.value()],
        };
        if typed_values
            .into_iter()
            .any(|value| !self.value_type.accepts(value))
        {
            return Err(format!(
                "output contains a value incompatible with declared type '{}'",
                self.value_type.as_str()
            ));
        }
        Ok(output)
    }

    fn explain_value(&self) -> Value {
        json!({
            "shape": format!("{:?}", self.shape()).to_ascii_lowercase(),
            "type": self.value_type().as_str(),
            "columns": self.columns(),
        })
    }
}

#[derive(Debug, Clone)]
pub enum PlanStep {
    Run(PlanRunStep),
    Let(PlanLetStep),
    Assert(PlanAssertStep),
    Call(PlanCallStep),
    ForEach(PlanForEachStep),
}

impl PlanStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Run(step) => &step.id,
            Self::Let(step) => &step.id,
            Self::Assert(step) => &step.id,
            Self::Call(step) => &step.id,
            Self::ForEach(step) => &step.id,
        }
    }

    fn explain_value(&self) -> Value {
        match self {
            Self::Run(step) => json!({
                "id": step.id,
                "kind": "run",
                "run": step.run.display(),
                "when": step.when,
            }),
            Self::Let(step) => json!({ "id": step.id, "kind": "let", "expr": step.expr }),
            Self::Assert(step) => {
                json!({ "id": step.id, "kind": "assert", "condition": step.condition })
            }
            Self::Call(step) => json!({
                "id": step.id,
                "kind": "call",
                "call": step.call,
                "when": step.when,
            }),
            Self::ForEach(step) => json!({
                "id": step.id,
                "kind": "for_each",
                "call": step.call,
                "as": step.item_name,
                "max_items": step.max_items,
                "when": step.when,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanRunStep {
    id: String,
    run: CommandPath,
    bindings: BTreeMap<String, PlanBinding>,
    when: Option<String>,
    effects: CommandEffects,
    reauthentication_retry: ReauthenticationRetry,
    requires_authentication: bool,
}

impl PlanRunStep {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn run(&self) -> &CommandPath {
        &self.run
    }

    pub fn bindings(&self) -> &BTreeMap<String, PlanBinding> {
        &self.bindings
    }

    pub fn when(&self) -> Option<&str> {
        self.when.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct PlanLetStep {
    id: String,
    expr: String,
}

impl PlanLetStep {
    pub fn expr(&self) -> &str {
        &self.expr
    }
}

#[derive(Debug, Clone)]
pub struct PlanAssertStep {
    id: String,
    condition: String,
    message: String,
}

impl PlanAssertStep {
    pub fn id(&self) -> &str {
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
pub struct PlanCallStep {
    id: String,
    call: String,
    bindings: BTreeMap<String, PlanBinding>,
    when: Option<String>,
}

impl PlanCallStep {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn call(&self) -> &str {
        &self.call
    }

    pub fn bindings(&self) -> &BTreeMap<String, PlanBinding> {
        &self.bindings
    }

    pub fn when(&self) -> Option<&str> {
        self.when.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct PlanForEachStep {
    id: String,
    items: PlanBinding,
    item_name: String,
    call: String,
    bindings: BTreeMap<String, PlanBinding>,
    max_items: usize,
    when: Option<String>,
}

impl PlanForEachStep {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn items(&self) -> &PlanBinding {
        &self.items
    }

    pub fn item_name(&self) -> &str {
        &self.item_name
    }

    pub fn call(&self) -> &str {
        &self.call
    }

    pub fn bindings(&self) -> &BTreeMap<String, PlanBinding> {
        &self.bindings
    }

    pub fn max_items(&self) -> usize {
        self.max_items
    }

    pub fn when(&self) -> Option<&str> {
        self.when.as_deref()
    }
}

#[derive(Debug, Clone)]
pub enum PlanBinding {
    Literal(Value),
    Input(String),
    Config(String),
    Step {
        step: String,
        select: Option<String>,
    },
}

struct DraftPlan {
    inputs: Vec<PlanInput>,
    output: PlanOutput,
    steps: Vec<PlanStep>,
    result: String,
    allows_mutation: bool,
}

#[derive(Clone, Copy)]
struct Analysis {
    effects: CommandEffects,
    reauthentication_retry: ReauthenticationRetry,
    requires_authentication: bool,
    operations: usize,
    call_depth: usize,
}

fn compile_workflow<'a, F>(
    workflow: &WorkflowDeclaration,
    config_declarations: &BTreeMap<String, ConfigDeclaration>,
    config: &Value,
    resolve_command: &F,
    limits: WorkflowLimits,
) -> Result<DraftPlan, String>
where
    F: Fn(&[String]) -> Option<&'a CommandSpec>,
{
    validate_expression(workflow.name(), "result", workflow.result(), limits)?;
    let inputs = workflow
        .inputs()
        .iter()
        .map(PlanInput::from_declaration)
        .collect::<Vec<_>>();
    let mut steps = Vec::with_capacity(workflow.steps().len());
    for step in workflow.steps() {
        let compiled = match step {
            WorkflowStep::Run(step) => {
                validate_optional_expression(
                    workflow.name(),
                    step.id(),
                    "when",
                    step.when(),
                    limits,
                )?;
                let command = resolve_command(step.run().segments()).ok_or_else(|| {
                    format!(
                        "workflow '{}' step '{}' command '{}' was not found in the built-in catalog",
                        workflow.name().as_str(),
                        step.id().as_str(),
                        step.run().display()
                    )
                })?;
                if command.workflow_contract().effects().may_mutate() && !workflow.allows_mutation()
                {
                    return Err(format!(
                        "workflow '{}' step '{}' command '{}' may change state; declare capabilities = [\"mutate\"]",
                        workflow.name().as_str(),
                        step.id().as_str(),
                        command.workflow_contract().command_id()
                    ));
                }
                validate_run_bindings(
                    workflow,
                    step.id(),
                    step.bindings(),
                    command,
                    config_declarations,
                    config,
                )?;
                PlanStep::Run(PlanRunStep {
                    id: step.id().as_str().to_string(),
                    run: step.run().clone(),
                    bindings: compile_bindings(
                        workflow.name(),
                        step.id(),
                        step.bindings(),
                        limits,
                    )?,
                    when: step.when().map(str::to_string),
                    effects: command.workflow_contract().effects(),
                    reauthentication_retry: command.reauthentication_retry,
                    requires_authentication: !is_offline_builtin_command(step.run().segments())
                        && command.handler.requires_authentication(),
                })
            }
            WorkflowStep::Let(step) => {
                validate_expression(workflow.name(), "let expression", step.expr(), limits)?;
                PlanStep::Let(PlanLetStep {
                    id: step.id().as_str().to_string(),
                    expr: step.expr().to_string(),
                })
            }
            WorkflowStep::Assert(step) => {
                validate_expression(
                    workflow.name(),
                    "assert condition",
                    step.condition(),
                    limits,
                )?;
                PlanStep::Assert(PlanAssertStep {
                    id: step.id().as_str().to_string(),
                    condition: step.condition().to_string(),
                    message: step.message().to_string(),
                })
            }
            WorkflowStep::Call(step) => {
                validate_optional_expression(
                    workflow.name(),
                    step.id(),
                    "when",
                    step.when(),
                    limits,
                )?;
                PlanStep::Call(PlanCallStep {
                    id: step.id().as_str().to_string(),
                    call: step.call().as_str().to_string(),
                    bindings: compile_bindings(
                        workflow.name(),
                        step.id(),
                        step.bindings(),
                        limits,
                    )?,
                    when: step.when().map(str::to_string),
                })
            }
            WorkflowStep::ForEach(step) => {
                if step.max_items() > limits.max_for_each_items {
                    return Err(format!(
                        "workflow '{}' for_each step '{}' max_items={} exceeds limit {}",
                        workflow.name().as_str(),
                        step.id().as_str(),
                        step.max_items(),
                        limits.max_for_each_items
                    ));
                }
                validate_optional_expression(
                    workflow.name(),
                    step.id(),
                    "when",
                    step.when(),
                    limits,
                )?;
                PlanStep::ForEach(PlanForEachStep {
                    id: step.id().as_str().to_string(),
                    items: compile_binding(workflow.name(), step.id(), step.items(), limits)?,
                    item_name: step.item_name().to_string(),
                    call: step.call().as_str().to_string(),
                    bindings: compile_bindings(
                        workflow.name(),
                        step.id(),
                        step.bindings(),
                        limits,
                    )?,
                    max_items: step.max_items(),
                    when: step.when().map(str::to_string),
                })
            }
        };
        steps.push(compiled);
    }
    Ok(DraftPlan {
        inputs,
        output: PlanOutput::from_declaration(workflow.output()),
        steps,
        result: workflow.result().to_string(),
        allows_mutation: workflow.allows_mutation(),
    })
}

fn compile_bindings(
    workflow: &WorkflowName,
    step: &WorkflowStepId,
    bindings: &BTreeMap<WorkflowBindingName, WorkflowBinding>,
    limits: WorkflowLimits,
) -> Result<BTreeMap<String, PlanBinding>, String> {
    bindings
        .iter()
        .map(|(name, binding)| {
            compile_binding(workflow, step, binding, limits)
                .map(|binding| (name.as_str().to_string(), binding))
        })
        .collect()
}

fn compile_binding(
    workflow: &WorkflowName,
    step: &WorkflowStepId,
    binding: &WorkflowBinding,
    limits: WorkflowLimits,
) -> Result<PlanBinding, String> {
    Ok(match binding {
        WorkflowBinding::Literal(value) => PlanBinding::Literal(value.clone()),
        WorkflowBinding::Input { name } => PlanBinding::Input(name.clone()),
        WorkflowBinding::Config { key } => PlanBinding::Config(key.clone()),
        WorkflowBinding::Step {
            step: source,
            select,
        } => {
            if let Some(select) = select {
                validate_expression(workflow, "step selector", select, limits)
                    .map_err(|error| format!("{error} (in step '{}')", step.as_str()))?;
            }
            PlanBinding::Step {
                step: source.as_str().to_string(),
                select: select.clone(),
            }
        }
    })
}

fn validate_expression(
    workflow: &WorkflowName,
    label: &str,
    expression: &str,
    limits: WorkflowLimits,
) -> Result<(), String> {
    validate_bounded_jq_expression(expression, limits.jq()).map_err(|error| {
        format!(
            "workflow '{}' {label} expression is invalid: {error}",
            workflow.as_str()
        )
    })
}

fn validate_optional_expression(
    workflow: &WorkflowName,
    step: &WorkflowStepId,
    label: &str,
    expression: Option<&str>,
    limits: WorkflowLimits,
) -> Result<(), String> {
    expression.map_or(Ok(()), |expression| {
        validate_expression(workflow, label, expression, limits)
            .map_err(|error| format!("{error} (in step '{}')", step.as_str()))
    })
}

fn validate_run_bindings(
    workflow: &WorkflowDeclaration,
    step: &WorkflowStepId,
    bindings: &BTreeMap<WorkflowBindingName, WorkflowBinding>,
    command: &CommandSpec,
    config_declarations: &BTreeMap<String, ConfigDeclaration>,
    config: &Value,
) -> Result<(), String> {
    let contract = command.workflow_contract();
    for (name, binding) in bindings {
        let target = contract.input(name.as_str()).ok_or_else(|| {
            format!(
                "workflow '{}' step '{}' command '{}' has no input named '{}'",
                workflow.name().as_str(),
                step.as_str(),
                contract.command_id(),
                name.as_str()
            )
        })?;
        validate_binding_compatibility(workflow, binding, target, config_declarations, config)
            .map_err(|message| {
                format!(
                    "workflow '{}' step '{}' binding '{}': {message}",
                    workflow.name().as_str(),
                    step.as_str(),
                    name.as_str()
                )
            })?;
    }
    for input in contract.inputs() {
        if input.required()
            && !bindings.keys().any(|name| name.as_str() == input.id())
            && !["help", "json", "output", "table-headers"].contains(&input.id())
        {
            return Err(format!(
                "workflow '{}' step '{}' required input '{}' has no binding",
                workflow.name().as_str(),
                step.as_str(),
                input.id()
            ));
        }
    }
    Ok(())
}

fn validate_binding_compatibility(
    workflow: &WorkflowDeclaration,
    binding: &WorkflowBinding,
    target: &WorkflowInputContract,
    config_declarations: &BTreeMap<String, ConfigDeclaration>,
    config: &Value,
) -> Result<(), String> {
    let signature = match binding {
        WorkflowBinding::Literal(value) => BindingSignature::from_value(value),
        WorkflowBinding::Input { name } => {
            let input = workflow
                .inputs()
                .iter()
                .find(|input| input.name() == name)
                .expect("protocol validated workflow input reference");
            BindingSignature {
                value_type: input.value_type(),
                repeatable: input.repeatable(),
                dynamic: false,
            }
        }
        WorkflowBinding::Config { key } => {
            let declaration = config_declarations
                .get(key)
                .expect("protocol validated config reference");
            let value = config.as_object().and_then(|config| config.get(key));
            if let Some(value) = value {
                BindingSignature::from_value(value)
            } else {
                BindingSignature {
                    value_type: declaration.value_type(),
                    repeatable: declaration.repeatable(),
                    dynamic: false,
                }
            }
        }
        WorkflowBinding::Step { .. } => BindingSignature {
            value_type: DeclaredValueType::Json,
            repeatable: false,
            dynamic: true,
        },
    };
    signature.validate_target(target)
}

struct BindingSignature {
    value_type: DeclaredValueType,
    repeatable: bool,
    dynamic: bool,
}

impl BindingSignature {
    fn from_value(value: &Value) -> Self {
        let (value, repeatable) = match value {
            Value::Array(values) => (values.first().unwrap_or(&Value::Null), true),
            value => (value, false),
        };
        let value_type = match value {
            Value::String(_) => DeclaredValueType::String,
            Value::Number(number) if number.is_i64() || number.is_u64() => {
                DeclaredValueType::Integer
            }
            Value::Number(_) => DeclaredValueType::Number,
            Value::Bool(_) => DeclaredValueType::Boolean,
            Value::Null | Value::Array(_) | Value::Object(_) => DeclaredValueType::Json,
        };
        Self {
            value_type,
            repeatable,
            dynamic: matches!(value, Value::Null),
        }
    }

    fn validate_target(&self, target: &WorkflowInputContract) -> Result<(), String> {
        if self.dynamic {
            return Ok(());
        }
        let type_compatible = match target.value_type() {
            CommandValueType::Text => self.value_type == DeclaredValueType::String,
            CommandValueType::Integer => self.value_type == DeclaredValueType::Integer,
            CommandValueType::Number => matches!(
                self.value_type,
                DeclaredValueType::Integer | DeclaredValueType::Number
            ),
            CommandValueType::Boolean => self.value_type == DeclaredValueType::Boolean,
            CommandValueType::Json => true,
        };
        if !type_compatible {
            return Err(format!(
                "declared type '{}' is incompatible with target type {:?}",
                self.value_type.as_str(),
                target.value_type()
            ));
        }
        match (self.repeatable, target.cardinality()) {
            (true, WorkflowCardinality::One | WorkflowCardinality::Fixed(_)) => {
                Err("repeatable value cannot bind a non-repeatable command input".to_string())
            }
            (false, WorkflowCardinality::Fixed(_) | WorkflowCardinality::RepeatedFixed(_)) => {
                Err("scalar value cannot bind a fixed-arity command input".to_string())
            }
            _ => Ok(()),
        }
    }
}

fn analyze_workflow(
    name: &str,
    drafts: &BTreeMap<String, DraftPlan>,
    visiting: &mut Vec<String>,
    memo: &mut HashMap<String, Analysis>,
    limits: WorkflowLimits,
) -> Result<Analysis, String> {
    if let Some(analysis) = memo.get(name) {
        return Ok(*analysis);
    }
    if let Some(index) = visiting.iter().position(|candidate| candidate == name) {
        let mut cycle = visiting[index..].to_vec();
        cycle.push(name.to_string());
        return Err(format!(
            "workflow call cycle detected: {}",
            cycle.join(" -> ")
        ));
    }
    visiting.push(name.to_string());
    let draft = drafts
        .get(name)
        .ok_or_else(|| format!("workflow '{name}' was not found"))?;
    let mut analysis = Analysis {
        effects: CommandEffects::ReadOnly,
        reauthentication_retry: ReauthenticationRetry::Safe,
        requires_authentication: false,
        operations: 1,
        call_depth: 1,
    };
    for step in &draft.steps {
        analysis.operations = checked_add(analysis.operations, 1, name, limits)?;
        match step {
            PlanStep::Run(step) => {
                let expression_operations =
                    usize::from(step.when.is_some()) + binding_expression_count(&step.bindings);
                analysis.operations =
                    checked_add(analysis.operations, expression_operations, name, limits)?;
                if step.effects.may_mutate() {
                    analysis.effects = CommandEffects::Mutating;
                }
                if step.reauthentication_retry == ReauthenticationRetry::Unsafe {
                    analysis.reauthentication_retry = ReauthenticationRetry::Unsafe;
                }
                analysis.requires_authentication |= step.requires_authentication;
            }
            PlanStep::Call(step) => {
                let expression_operations =
                    usize::from(step.when.is_some()) + binding_expression_count(&step.bindings);
                analysis.operations =
                    checked_add(analysis.operations, expression_operations, name, limits)?;
                let target = analyze_workflow(&step.call, drafts, visiting, memo, limits)?;
                merge_analysis(&mut analysis, target, 1, name, limits)?;
            }
            PlanStep::ForEach(step) => {
                let item_expression_operations = usize::from(matches!(
                    &step.items,
                    PlanBinding::Step {
                        select: Some(_),
                        ..
                    }
                ));
                let per_item_expression_operations = binding_expression_count(&step.bindings)
                    .checked_mul(step.max_items)
                    .ok_or_else(|| format!("workflow '{name}' operation count overflowed"))?;
                let expression_operations = usize::from(step.when.is_some())
                    .checked_add(item_expression_operations)
                    .and_then(|count| count.checked_add(per_item_expression_operations))
                    .ok_or_else(|| format!("workflow '{name}' operation count overflowed"))?;
                analysis.operations =
                    checked_add(analysis.operations, expression_operations, name, limits)?;
                let target = analyze_workflow(&step.call, drafts, visiting, memo, limits)?;
                merge_analysis(&mut analysis, target, step.max_items, name, limits)?;
            }
            PlanStep::Let(_) | PlanStep::Assert(_) => {
                analysis.operations = checked_add(analysis.operations, 1, name, limits)?;
            }
        }
    }
    visiting.pop();
    if analysis.call_depth > limits.max_call_depth {
        return Err(format!(
            "workflow '{name}' expanded call depth {} exceeds limit {}",
            analysis.call_depth, limits.max_call_depth
        ));
    }
    memo.insert(name.to_string(), analysis);
    Ok(analysis)
}

fn binding_expression_count(bindings: &BTreeMap<String, PlanBinding>) -> usize {
    bindings
        .values()
        .filter(|binding| {
            matches!(
                binding,
                PlanBinding::Step {
                    select: Some(_),
                    ..
                }
            )
        })
        .count()
}

fn merge_analysis(
    analysis: &mut Analysis,
    target: Analysis,
    multiplier: usize,
    name: &str,
    limits: WorkflowLimits,
) -> Result<(), String> {
    if target.effects.may_mutate() {
        analysis.effects = CommandEffects::Mutating;
    }
    if target.reauthentication_retry == ReauthenticationRetry::Unsafe {
        analysis.reauthentication_retry = ReauthenticationRetry::Unsafe;
    }
    analysis.requires_authentication |= target.requires_authentication;
    let nested_operations = target
        .operations
        .checked_mul(multiplier)
        .ok_or_else(|| format!("workflow '{name}' operation count overflowed"))?;
    analysis.operations = checked_add(analysis.operations, nested_operations, name, limits)?;
    analysis.call_depth = analysis.call_depth.max(target.call_depth + 1);
    Ok(())
}

fn checked_add(
    current: usize,
    addition: usize,
    name: &str,
    limits: WorkflowLimits,
) -> Result<usize, String> {
    let total = current
        .checked_add(addition)
        .ok_or_else(|| format!("workflow '{name}' operation count overflowed"))?;
    if total > limits.max_operations {
        Err(format!(
            "workflow '{name}' worst-case operation count {total} exceeds limit {}",
            limits.max_operations
        ))
    } else {
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use hubuum_extension_protocol::ExtensionManifest;
    use serde_json::json;

    use super::{WorkflowLimits, WorkflowProgram, MAX_FOR_EACH_ITEMS, MAX_WORKFLOW_CALL_DEPTH};

    #[test]
    fn default_limits_are_mandatory_and_nonzero() {
        let limits = WorkflowLimits::default();
        assert_eq!(limits.max_call_depth(), MAX_WORKFLOW_CALL_DEPTH);
        assert_eq!(limits.max_for_each_items(), MAX_FOR_EACH_ITEMS);
        assert!(limits.max_operations() > 0);
        assert!(limits.max_output_bytes() > 0);
    }

    #[test]
    fn compiler_rejects_call_cycles() {
        let manifest = ExtensionManifest::parse(
            r#"schema_version = 1
kind = "portable"
name = "cycle"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"

[workflows.first]
result = ".steps.next"
step_order = ["next"]
[workflows.first.output]
shape = "values"
type = "json"
[workflows.first.steps.next]
kind = "call"
call = "second"

[workflows.second]
result = ".steps.next"
step_order = ["next"]
[workflows.second.output]
shape = "values"
type = "json"
[workflows.second.steps.next]
kind = "call"
call = "first"

[commands.run]
path = ["run"]
workflow = "first"
"#,
        )
        .expect("protocol-valid cycle");
        let error =
            WorkflowProgram::compile(&manifest, &json!({}), |_| None).expect_err("cycle must fail");
        assert!(error.contains("first -> second -> first"));
    }

    #[test]
    fn compiler_rejects_excessive_expanded_work() {
        let manifest = ExtensionManifest::parse(
            r#"schema_version = 1
kind = "portable"
name = "too-much-work"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"

[workflows.leaf]
result = "[.input.item]"
step_order = ["value"]
[workflows.leaf.inputs.item]
type = "json"
required = true
[workflows.leaf.output]
shape = "values"
type = "json"
[workflows.leaf.steps.value]
kind = "let"
expr = ".input.item"

[workflows.middle]
result = ".steps.items"
step_order = ["items"]
[workflows.middle.inputs.item]
type = "json"
required = true
[workflows.middle.output]
shape = "values"
type = "json"
[workflows.middle.steps.items]
kind = "for_each"
items = [1]
as = "item"
call = "leaf"
max_items = 100

[workflows.outer]
result = ".steps.items"
step_order = ["items"]
[workflows.outer.output]
shape = "values"
type = "json"
[workflows.outer.steps.items]
kind = "for_each"
items = [1]
as = "item"
call = "middle"
max_items = 100

[commands.run]
path = ["run"]
workflow = "outer"
"#,
        )
        .expect("protocol-valid graph");
        let error = WorkflowProgram::compile(&manifest, &json!({}), |_| None)
            .expect_err("work budget must fail");
        assert!(error.contains("operation count"));
    }

    #[test]
    fn compiler_rejects_excessive_call_depth() {
        let mut workflows = String::new();
        for index in 0..=MAX_WORKFLOW_CALL_DEPTH {
            workflows.push_str(&format!(
                "[workflows.w{index}]\nresult = \".steps.value\"\nstep_order = [\"value\"]\n[workflows.w{index}.output]\nshape = \"values\"\ntype = \"json\"\n[workflows.w{index}.steps.value]\n"
            ));
            if index == MAX_WORKFLOW_CALL_DEPTH {
                workflows.push_str("kind = \"let\"\nexpr = \"[1]\"\n\n");
            } else {
                workflows.push_str(&format!("kind = \"call\"\ncall = \"w{}\"\n\n", index + 1));
            }
        }
        let manifest = ExtensionManifest::parse(&format!(
            "schema_version = 1\nkind = \"portable\"\nname = \"deep\"\nversion = \"0.1.0\"\nrequires_cli = \">=0.0.9,<0.1\"\n\n{workflows}[commands.run]\npath = [\"run\"]\nworkflow = \"w0\"\n"
        ))
        .expect("protocol-valid graph");
        let error =
            WorkflowProgram::compile(&manifest, &json!({}), |_| None).expect_err("depth must fail");
        assert!(error.contains("call depth"));
    }
}
