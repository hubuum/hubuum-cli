use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use hubuum_extension_protocol::{
    CommandDeclaration, ExtensionManifest, ExtensionResponse, OptionDeclaration, OptionKind,
    SemanticOutput, SemanticOutputShape, PROTOCOL_V1,
};
use hubuum_filter::OutputEnvelope;
use serde_json::{to_string, Value};
use tokio::process::Command;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalogBuilder, CommandContext, CommandInvocation, CommandOutcome,
    CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::commands::{render_format, standard_options, table_headers};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::extensions::{ExtensionPack, ExtensionRegistry};
use crate::output::{
    add_warning, reset_output, set_pipeline, set_pipeline_suffix, set_render_format,
    set_semantic_output, set_table_headers, take_output,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_external_commands(
    builder: &mut CommandCatalogBuilder,
    registry: &Arc<ExtensionRegistry>,
) {
    for pack in registry.enabled_packs() {
        let Some(manifest) = pack.manifest_arc() else {
            continue;
        };
        let Some(executable) = pack.executable().map(PathBuf::from) else {
            continue;
        };
        for command in manifest.commands() {
            register_external_command(builder, pack, manifest.clone(), executable.clone(), command);
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
    use hubuum_extension_protocol::ExtensionManifest;

    use super::{extension_cli_options, forwarded_arguments};
    use crate::tokenizer::CommandTokenizer;

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
}
