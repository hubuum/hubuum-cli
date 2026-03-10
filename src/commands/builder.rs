use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalog, CommandCatalogBuilder, CommandContext, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::commands::{self, command_options, CliCommand};
use crate::errors::AppError;
use crate::output::{reset_output, take_output};

#[derive(Clone, Copy, Default)]
pub(crate) struct CommandDocs {
    pub about: Option<&'static str>,
    pub long_about: Option<&'static str>,
    pub examples: Option<&'static str>,
}

pub fn build_command_catalog() -> CommandCatalog {
    let mut builder = CommandCatalogBuilder::new();

    commands::class::register_commands(&mut builder);
    commands::namespace::register_commands(&mut builder);
    commands::user::register_commands(&mut builder);
    commands::group::register_commands(&mut builder);
    commands::object::register_commands(&mut builder);
    commands::relations::register_commands(&mut builder);
    commands::help::register_commands(&mut builder);

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
        let services = ctx.app.services.clone();
        let raw_line = invocation.raw_line.clone();

        tokio::task::spawn_blocking(move || {
            reset_output()?;
            let cmd_name = invocation.command_path.last().cloned().ok_or_else(|| {
                AppError::CommandExecutionError("Missing command name".to_string())
            })?;

            let tokens = crate::tokenizer::CommandTokenizer::new(
                &raw_line,
                &cmd_name,
                &command_options::<C>(),
            )?;

            command.execute(services.as_ref(), &tokens)?;
            services.invalidate_completion();

            Ok(CommandOutcome {
                output: take_output()?,
                scope_action: ScopeAction::None,
            })
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
    }
}
