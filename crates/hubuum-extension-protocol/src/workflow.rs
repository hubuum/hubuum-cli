use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::{
    invalid_named_workflow, is_option_word, is_snake_word, workflow_value_contains_nul,
    CommandPath, ConfigDeclaration, ProtocolError, RawWorkflow, RawWorkflowBinding,
    RawWorkflowInput, RawWorkflowOutput, RawWorkflowStep, SemanticOutput, SemanticOutputShape,
    WorkflowName, WorkflowValueType,
};

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
    pub(super) fn validate(
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

pub(super) fn validate_workflow_calls(
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
                validate_workflow_input_value(item, target).map_err(invalid)?;
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

pub(super) fn validate_declared_default<F>(
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

pub(super) fn validate_declared_value(
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
