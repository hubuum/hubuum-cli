use std::sync::Arc;

use async_trait::async_trait;
use tokio::task::spawn_blocking;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalog, CommandCatalogBuilder, CommandContext, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::command_line::shell_escape;
use crate::commands::{self, command_options, render_format, table_headers, CliCommand};
use crate::config::get_config;
use crate::errors::AppError;
use crate::extensions::{ExtensionRegistry, WorkflowProgram};
use crate::output::{
    add_warning, reset_output, set_pipeline, set_pipeline_suffix, set_render_format,
    set_table_headers, take_output,
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

#[derive(Clone, Copy)]
pub(crate) struct CommandDeprecation {
    replacement_path: &'static [&'static str],
    option_renames: &'static [(&'static str, &'static str)],
}

impl CommandDeprecation {
    pub(crate) const fn renamed(
        replacement_path: &'static [&'static str],
        option_renames: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            replacement_path,
            option_renames,
        }
    }

    fn replacement_command(
        &self,
        tokens: &CommandTokenizer,
        command_index: usize,
        pipeline_suffix: Option<&str>,
    ) -> String {
        let command = self
            .replacement_path
            .iter()
            .map(|token| (*token).to_string())
            .chain(
                tokens
                    .raw_tokens()
                    .iter()
                    .skip(command_index.saturating_add(1))
                    .scan(false, |options_ended, token| {
                        if token == "--" {
                            *options_ended = true;
                        }
                        let token = if *options_ended {
                            token.to_string()
                        } else {
                            self.rename_option(token)
                        };
                        Some(shell_escape(&token))
                    }),
            )
            .collect::<Vec<_>>()
            .join(" ");
        match pipeline_suffix {
            Some(suffix) => format!("{command} {suffix}"),
            None => command,
        }
    }

    fn rename_option(&self, token: &str) -> String {
        for (old, new) in self.option_renames {
            if token == *old {
                return (*new).to_string();
            }
            if let Some(value) = token
                .strip_prefix(old)
                .and_then(|value| value.strip_prefix('='))
            {
                return format!("{new}={value}");
            }
        }
        token.to_string()
    }

    fn help_notice(&self) -> String {
        format!(
            "Deprecated: use '{}' instead. Invocations print an exact replacement command.",
            self.replacement_path.join(" ")
        )
    }

    fn warning(&self, command_path: &[String], replacement: &str) -> String {
        format!(
            "Command '{}' is deprecated; use `{replacement}` instead.",
            command_path.join(" ")
        )
    }
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
    commands::class_fields::register_commands(&mut builder);
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
    catalog_command_with_deprecation(name, command, docs, None)
}

pub(crate) fn deprecated_catalog_command<C>(
    name: &str,
    command: C,
    docs: CommandDocs,
    deprecation: CommandDeprecation,
) -> CommandSpec
where
    C: CliCommand + Clone + 'static,
{
    catalog_command_with_deprecation(name, command, docs, Some(deprecation))
}

fn catalog_command_with_deprecation<C>(
    name: &str,
    command: C,
    docs: CommandDocs,
    deprecation: Option<CommandDeprecation>,
) -> CommandSpec
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
            deprecation,
        }) as Arc<dyn AsyncCommandHandler>,
    );
    spec.about = docs.about.map(|about| match deprecation {
        Some(_) => format!("{about} (deprecated)"),
        None => about.to_string(),
    });
    spec.long_about = match (docs.long_about, deprecation) {
        (Some(long_about), Some(deprecation)) => {
            Some(format!("{long_about}\n\n{}", deprecation.help_notice()))
        }
        (Some(long_about), None) => Some(long_about.to_string()),
        (None, Some(deprecation)) => Some(deprecation.help_notice()),
        (None, None) => None,
    };
    spec.examples = docs.examples.map(str::to_string);
    spec
}

struct CommandHandler<C>
where
    C: CliCommand + Clone + 'static,
{
    command: Arc<C>,
    deprecation: Option<CommandDeprecation>,
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
        let deprecation = self.deprecation;

        spawn_blocking(move || {
            reset_output()?;
            set_pipeline(pipeline)?;
            set_pipeline_suffix(invocation.pipeline_suffix.clone())?;
            let tokens = CommandTokenizer::new_at(
                &raw_line,
                invocation.command_index,
                &command_options::<C>(),
            )?;
            if let Some(deprecation) = deprecation {
                let replacement = deprecation.replacement_command(
                    &tokens,
                    invocation.command_index,
                    invocation.pipeline_suffix.as_deref(),
                );
                add_warning(deprecation.warning(&invocation.command_path, &replacement))?;
            }
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

#[cfg(test)]
mod tests {
    use super::CommandDeprecation;
    use crate::tokenizer::CommandTokenizer;

    const RENAMED_FIELDS: CommandDeprecation = CommandDeprecation::renamed(
        &["class", "fields"],
        &[("--class", "--name"), ("-c", "--name")],
    );

    #[test]
    fn command_deprecation_builds_an_exact_replacement() {
        let tokens = CommandTokenizer::new(
            "object fields --class 'Host Group' --limit 10 --containers",
            "fields",
            &[],
        )
        .expect("deprecated command should tokenize");

        let replacement = RENAMED_FIELDS.replacement_command(&tokens, 1, None);

        assert_eq!(
            replacement,
            "class fields --name 'Host Group' --limit 10 --containers"
        );
        assert_eq!(
            RENAMED_FIELDS.warning(&["object".to_string(), "fields".to_string()], &replacement),
            "Command 'object fields' is deprecated; use `class fields --name 'Host Group' --limit 10 --containers` instead."
        );
    }

    #[test]
    fn command_deprecation_replaces_scoped_invocations_and_short_options() {
        let tokens = CommandTokenizer::new("fields -c Hosts --depth 4", "fields", &[])
            .expect("scoped deprecated command should tokenize");

        assert_eq!(
            RENAMED_FIELDS.replacement_command(&tokens, 0, Some("| P Field Source")),
            "class fields --name Hosts --depth 4 | P Field Source"
        );
    }

    #[test]
    fn command_deprecation_does_not_rewrite_positionals_after_double_dash() {
        let tokens = CommandTokenizer::new("fields -c Hosts -- --class", "fields", &[])
            .expect("double-dash command should tokenize");

        assert_eq!(
            RENAMED_FIELDS.replacement_command(&tokens, 0, None),
            "class fields --name Hosts -- --class"
        );
    }
}
