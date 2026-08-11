use std::sync::Arc;

use async_trait::async_trait;
use tokio::task::spawn_blocking;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalog, CommandCatalogBuilder, CommandContext, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::commands::{self, command_options, render_format, table_headers, CliCommand};
use crate::config::get_config;
use crate::errors::AppError;
use crate::extensions::{ExtensionRegistry, WorkflowProgram};
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
    extensions.compile_workflows(|manifest, config| {
        WorkflowProgram::compile(manifest, config, |path| builder.command(path))
    });
    let extensions = Arc::new(extensions);
    builder.set_extensions(extensions.clone());
    register_extension_management_commands(&mut builder);
    register_extension_commands(&mut builder, &extensions);

    builder.build()
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

    let mut spec = CommandSpec::new(
        name,
        options,
        C::REAUTHENTICATION_RETRY,
        C::EFFECTS,
        Arc::new(CommandHandler {
            command: Arc::new(command),
        }) as Arc<dyn AsyncCommandHandler>,
    );
    spec.about = docs.about.map(str::to_string);
    spec.long_about = docs.long_about.map(str::to_string);
    spec.examples = docs.examples.map(str::to_string);
    spec
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
