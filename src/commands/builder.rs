use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use hubuum_extension_protocol::{WorkflowAction, WorkflowArgumentKind};
use hubuum_filter::validate_jq_expression;
use serde_json::Value;
use tokio::task::spawn_blocking;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalog, CommandCatalogBuilder, CommandContext, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::command_line::shell_escape;
use crate::commands::{self, command_options, render_format, table_headers, CliCommand};
use crate::config::get_config;
use crate::errors::AppError;
use crate::extensions::ExtensionRegistry;
use crate::output::{
    reset_output, set_pipeline, set_pipeline_suffix, set_render_format, set_table_headers,
    take_output,
};
use crate::tokenizer::CommandTokenizer;

use super::extension::register_extension_commands;
use super::extension_management::register_commands as register_extension_management_commands;

#[derive(Clone, Copy, Default)]
pub(crate) struct CommandDocs {
    pub about: Option<&'static str>,
    pub long_about: Option<&'static str>,
    pub examples: Option<&'static str>,
}

pub fn build_command_catalog() -> CommandCatalog {
    let mut builder = CommandCatalogBuilder::new();
    let mut extensions = ExtensionRegistry::discover(&get_config());

    commands::admin::register_commands(&mut builder);
    commands::alias::register_commands(&mut builder);
    commands::backup::register_commands(&mut builder);
    commands::audit::register_commands(&mut builder);
    commands::auth::register_commands(&mut builder);
    commands::jobs::register_commands(&mut builder);
    commands::class::register_commands(&mut builder);
    commands::config::register_commands(&mut builder);
    commands::collection::register_commands(&mut builder);
    commands::computed::register_commands(&mut builder);
    commands::user::register_commands(&mut builder);
    commands::group::register_commands(&mut builder);
    commands::export::register_commands(&mut builder);
    commands::imports::register_commands(&mut builder);
    commands::task::register_commands(&mut builder);
    commands::theme::register_commands(&mut builder);
    commands::object::register_commands(&mut builder);
    commands::relations::register_commands(&mut builder);
    commands::remote_target::register_commands(&mut builder);
    commands::event_sink::register_commands(&mut builder);
    commands::event_subscription::register_commands(&mut builder);
    commands::event_delivery::register_commands(&mut builder);
    commands::search::register_commands(&mut builder);
    commands::service_account::register_commands(&mut builder);
    commands::me::register_commands(&mut builder);
    commands::metrics::register_commands(&mut builder);
    commands::history::register_commands(&mut builder);
    commands::help::register_commands(&mut builder);
    commands::version::register_commands(&mut builder);
    extensions.validate_workflow_actions(|action, allows_unsafe_actions, config| {
        validate_workflow_action(&builder, action, allows_unsafe_actions, config)
    });
    let extensions = Arc::new(extensions);
    builder.set_extensions(extensions.clone());
    register_extension_management_commands(&mut builder);
    register_extension_commands(&mut builder, &extensions);

    builder.build()
}

fn validate_workflow_action(
    builder: &CommandCatalogBuilder,
    action: &WorkflowAction,
    allows_unsafe_actions: bool,
    config: &Value,
) -> Result<(), String> {
    let command = builder
        .command(action.command().segments())
        .ok_or_else(|| "command was not found in the built-in catalog".to_string())?;
    if command.reauthentication_retry == crate::errors::ReauthenticationRetry::Unsafe
        && !allows_unsafe_actions
    {
        return Err(
            "command is not safe to replay; declare allow_unsafe_actions = true on the workflow"
                .to_string(),
        );
    }

    let mut action_tokens = action.command().segments().to_vec();
    for argument in action.arguments() {
        if argument.kind() == WorkflowArgumentKind::ActionOutput {
            if let Some(selector) = argument.selector() {
                validate_jq_expression(selector)
                    .map_err(|error| format!("invalid action output selector: {error}"))?;
            }
        }
        let value = match argument.kind() {
            WorkflowArgumentKind::Literal => argument.value().to_string(),
            WorkflowArgumentKind::ConfigValue => config
                .as_object()
                .and_then(|config| config.get(argument.value()))
                .filter(|value| !value.is_null())
                .map(workflow_config_argument)
                .transpose()?
                .or_else(|| argument.default().map(str::to_string))
                .ok_or_else(|| format!("required config key '{}' is missing", argument.value()))?,
            WorkflowArgumentKind::OptionValue | WorkflowArgumentKind::ActionOutput => {
                "workflow-value".to_string()
            }
        };
        if value.contains('\0') {
            return Err("action arguments cannot contain NUL".to_string());
        }
        if matches!(
            value.as_str(),
            "-j" | "--json" | "-o" | "--output" | "--table-headers"
        ) || value.starts_with("--output=")
        {
            return Err("action arguments cannot override host output options".to_string());
        }
        action_tokens.push(value);
    }
    action_tokens.extend(["--output".to_string(), "json".to_string()]);
    let raw_line = action_tokens
        .iter()
        .map(|token| shell_escape(token))
        .collect::<Vec<_>>()
        .join(" ");
    let option_defs = command
        .options
        .iter()
        .map(OptionSpec::to_cli_option)
        .collect::<Vec<_>>();
    let command_index = action.command().segments().len() - 1;
    let tokens = CommandTokenizer::new_without_value_source_resolution_at(
        &raw_line,
        command_index,
        &option_defs,
    )
    .map_err(|error| error.to_string())?;

    let mut aliases = HashMap::new();
    for option in &command.options {
        if let Some(short) = option.short.as_deref() {
            aliases.insert(short.trim_start_matches('-').to_string(), option);
        }
        if let Some(long) = option.long.as_deref() {
            aliases.insert(long.trim_start_matches('-').to_string(), option);
        }
    }
    let mut counts = HashMap::<&str, usize>::new();
    for occurrence in tokens.get_option_occurrences() {
        let option = aliases
            .get(&occurrence.key)
            .ok_or_else(|| format!("unknown option '{}'", occurrence.key))?;
        if option.flag && !occurrence.value.is_empty() {
            return Err(format!("flag '{}' cannot carry a value", option.name));
        }
        *counts.entry(option.name.as_str()).or_default() += 1;
    }
    for option in command
        .options
        .iter()
        .filter(|option| option.short.is_some() || option.long.is_some())
    {
        let count = counts.get(option.name.as_str()).copied().unwrap_or(0);
        if option.required && count == 0 {
            return Err(format!("required option '{}' is missing", option.name));
        }
        if !option.repeatable && count > 1 {
            return Err(format!("option '{}' cannot be repeated", option.name));
        }
    }

    let positionals = command
        .options
        .iter()
        .filter(|option| option.short.is_none() && option.long.is_none())
        .collect::<Vec<_>>();
    let mut positional_index = 0;
    for option in positionals {
        if option.repeatable {
            if option.required && positional_index == tokens.get_positionals().len() {
                return Err(format!("required positional '{}' is missing", option.name));
            }
            positional_index = tokens.get_positionals().len();
        } else if positional_index < tokens.get_positionals().len() {
            positional_index += 1;
        } else if option.required {
            return Err(format!("required positional '{}' is missing", option.name));
        }
    }
    if positional_index < tokens.get_positionals().len() {
        return Err(format!(
            "unexpected positional '{}'",
            tokens.get_positionals()[positional_index]
        ));
    }
    Ok(())
}

fn workflow_config_argument(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err("workflow config values must be scalar".to_string())
        }
    }
}

pub(crate) fn catalog_command<C>(name: &str, command: C, docs: CommandDocs) -> CommandSpec
where
    C: CliCommand + Clone + 'static,
{
    let options = command_options::<C>()
        .into_iter()
        .map(|option| OptionSpec {
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
            completion: match option.autocomplete {
                Some(completion) => CompletionSpec::Dynamic(completion),
                None => CompletionSpec::None,
            },
        })
        .collect();

    CommandSpec {
        name: name.to_string(),
        about: docs.about.map(str::to_string),
        long_about: docs.long_about.map(str::to_string),
        examples: docs.examples.map(str::to_string),
        options,
        reauthentication_retry: C::REAUTHENTICATION_RETRY,
        handler: Arc::new(CommandHandler {
            command: Arc::new(command),
        }) as Arc<dyn AsyncCommandHandler>,
    }
}

struct CommandHandler<C>
where
    C: CliCommand + Clone + 'static,
{
    command: Arc<C>,
}

#[async_trait]
impl<C> AsyncCommandHandler for CommandHandler<C>
where
    C: CliCommand + Clone + 'static,
{
    async fn execute(
        &self,
        ctx: CommandContext,
        invocation: CommandInvocation,
    ) -> Result<CommandOutcome, AppError> {
        let command = self.command.clone();
        let services = ctx.services.clone().ok_or_else(|| {
            AppError::CommandExecutionError(
                "This command requires an authenticated Hubuum session".to_string(),
            )
        })?;
        let raw_line = invocation.raw_line.clone();
        let pipeline = invocation.pipeline.clone();

        spawn_blocking(move || {
            reset_output()?;
            set_pipeline(pipeline)?;
            set_pipeline_suffix(invocation.pipeline_suffix.clone())?;
            let tokens = CommandTokenizer::new_at(
                &raw_line,
                invocation.command_index,
                &command_options::<C>(),
            )?;
            set_render_format(render_format(&tokens)?)?;
            set_table_headers(table_headers(&tokens)?)?;

            command.execute(services.as_ref(), &tokens)?;
            services.invalidate_completion();

            Ok(CommandOutcome {
                output: take_output()?,
                scope_action: ScopeAction::None,
                ..Default::default()
            })
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
    }
}
