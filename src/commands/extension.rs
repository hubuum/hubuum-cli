mod workflow;
use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use workflow::WorkflowCommandHandler;

use async_trait::async_trait;
use hubuum_extension_protocol::{
    CommandDeclaration, ExtensionManifest, ExtensionResponse, OptionDeclaration, OptionKind,
    SemanticOutput, SemanticOutputShape, PROTOCOL_V1,
};
use hubuum_filter::OutputEnvelope;
use serde_json::{from_str, to_string, Map, Value};
use tokio::process::Command;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalogBuilder, CommandContext, CommandEffects, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction, WorkflowCardinality,
    WorkflowInputContract, WorkflowValueType,
};
use crate::commands::{render_format, standard_options, table_headers, CliOption};
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

const WORKFLOW_HOST_INPUTS: &[&str] = &["help", "json", "output", "table-headers", "debug"];

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
    let standard_keys = HashSet::from([
        "h",
        "help",
        "j",
        "json",
        "o",
        "output",
        "table-headers",
        "debug",
    ]);
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
            "-h" | "--help" | "-j" | "--json" | "--debug" => index += 1,
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
    child.env(
        "HUBUUM_CLI__SERVER__SESSION_STORAGE",
        ctx.config.server.session_storage.to_string(),
    );
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
            "list target-1 --state active --output json --debug",
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
                    stream_output: false,
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
