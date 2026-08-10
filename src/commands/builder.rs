use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use hubuum_extension_protocol::{WorkflowBinding, WorkflowDeclaration};
use hubuum_filter::validate_jq_expression;
use serde_json::{json, Value};
use tokio::task::spawn_blocking;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalog, CommandCatalogBuilder, CommandContext, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::commands::{self, command_options, render_format, table_headers, CliCommand};
use crate::config::get_config;
use crate::errors::AppError;
use crate::extensions::ExtensionRegistry;
use crate::output::{
    reset_output, set_pipeline, set_pipeline_suffix, set_render_format, set_table_headers,
    take_output,
};
use crate::tokenizer::CommandTokenizer;

use super::extension::{
    register_extension_commands, workflow_binding_name, workflow_step_arguments,
};
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
    extensions.validate_workflows(|workflow, config| validate_workflow(&builder, workflow, config));
    let extensions = Arc::new(extensions);
    builder.set_extensions(extensions.clone());
    register_extension_management_commands(&mut builder);
    register_extension_commands(&mut builder, &extensions);

    builder.build()
}

fn validate_workflow(
    builder: &CommandCatalogBuilder,
    workflow: &WorkflowDeclaration,
    config: &Value,
) -> Result<(), String> {
    if let Some(result) = workflow.result() {
        validate_jq_expression(result)
            .map_err(|error| format!("result expression is invalid: {error}"))?;
    }

    for step in workflow.steps() {
        let command = builder.command(step.run().segments()).ok_or_else(|| {
            format!(
                "step '{}' command '{}' was not found in the built-in catalog",
                step.id().as_str(),
                step.run().display()
            )
        })?;
        if command.reauthentication_retry == crate::errors::ReauthenticationRetry::Unsafe
            && !workflow.allows_mutation()
        {
            return Err(format!(
                "step '{}' command '{}' may change state; declare capabilities = [\"mutate\"]",
                step.id().as_str(),
                step.run().display()
            ));
        }

        let mut values = BTreeMap::new();
        for (name, binding) in step.bindings() {
            let option = command
                .options
                .iter()
                .find(|option| workflow_binding_name(option) == name.as_str())
                .ok_or_else(|| {
                    format!(
                        "step '{}' command '{}' has no input named '{}'",
                        step.id().as_str(),
                        step.run().display(),
                        name.as_str()
                    )
                })?;
            let value = match binding {
                WorkflowBinding::Literal(value) => value.clone(),
                WorkflowBinding::Input { .. } => workflow_placeholder(option),
                WorkflowBinding::Config { key, default } => config
                    .as_object()
                    .and_then(|config| config.get(key))
                    .filter(|value| !value.is_null())
                    .cloned()
                    .or_else(|| default.clone())
                    .unwrap_or(Value::Null),
                WorkflowBinding::Step { select, .. } => {
                    if let Some(select) = select {
                        validate_jq_expression(select).map_err(|error| {
                            format!(
                                "step '{}' binding '{}' select expression is invalid: {error}",
                                step.id().as_str(),
                                name.as_str()
                            )
                        })?;
                    }
                    workflow_placeholder(option)
                }
            };
            values.insert(name.as_str().to_string(), value);
        }
        workflow_step_arguments(command, &values).map_err(|message| {
            format!(
                "step '{}' command '{}': {message}",
                step.id().as_str(),
                step.run().display()
            )
        })?;
    }
    Ok(())
}

fn workflow_placeholder(option: &OptionSpec) -> Value {
    if option.flag {
        return Value::Bool(true);
    }
    let value = || Value::String("workflow-value".to_string());
    match (option.repeatable, option.nargs) {
        (true, Some(count)) => {
            Value::Array(vec![Value::Array((0..count).map(|_| value()).collect())])
        }
        (_, Some(count)) => Value::Array((0..count).map(|_| value()).collect()),
        (true, None) => json!(["workflow-value"]),
        (false, None) => value(),
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
