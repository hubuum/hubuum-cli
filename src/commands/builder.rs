use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{
    AsyncCommandHandler, CommandCatalog, CommandCatalogBuilder, CommandContext, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::commands::{self, CliCommand};
use crate::errors::AppError;
use crate::output::{reset_output, take_output};

pub fn build_command_catalog() -> CommandCatalog {
    let mut builder = CommandCatalogBuilder::new();

    add_class_commands(&mut builder);
    add_namespace_commands(&mut builder);
    add_user_commands(&mut builder);
    add_group_commands(&mut builder);
    add_object_commands(&mut builder);
    add_relation_commands(&mut builder);

    builder.add_command(&[], legacy_command("help", commands::Help::default()));
    builder.build()
}

fn add_class_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(&["class"], legacy_command("create", commands::ClassNew::default()))
        .add_command(&["class"], legacy_command("list", commands::ClassList::default()))
        .add_command(&["class"], legacy_command("delete", commands::ClassDelete::default()))
        .add_command(&["class"], legacy_command("info", commands::ClassInfo::default()));
}

fn add_namespace_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["namespace"],
            legacy_command("create", commands::NamespaceNew::default()),
        )
        .add_command(
            &["namespace"],
            legacy_command("list", commands::NamespaceList::default()),
        )
        .add_command(
            &["namespace"],
            legacy_command("delete", commands::NamespaceDelete::default()),
        )
        .add_command(
            &["namespace"],
            legacy_command("info", commands::NamespaceInfo::default()),
        )
        .add_command(
            &["namespace", "permissions"],
            legacy_command("list", commands::NamespacePermissions::default()),
        )
        .add_command(
            &["namespace", "permissions"],
            legacy_command("set", commands::NamespacePermissionsSet::default()),
        );
}

fn add_user_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(&["user"], legacy_command("create", commands::UserNew::default()))
        .add_command(&["user"], legacy_command("list", commands::UserList::default()))
        .add_command(&["user"], legacy_command("delete", commands::UserDelete::default()))
        .add_command(&["user"], legacy_command("info", commands::UserInfo::default()));
}

fn add_group_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(&["group"], legacy_command("create", commands::GroupNew::default()))
        .add_command(&["group"], legacy_command("list", commands::GroupList::default()))
        .add_command(
            &["group"],
            legacy_command("add_user", commands::GroupAddUser::default()),
        )
        .add_command(
            &["group"],
            legacy_command("remove_user", commands::GroupRemoveUser::default()),
        )
        .add_command(&["group"], legacy_command("info", commands::GroupInfo::default()));
}

fn add_object_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(&["object"], legacy_command("create", commands::ObjectNew::default()))
        .add_command(&["object"], legacy_command("list", commands::ObjectList::default()))
        .add_command(&["object"], legacy_command("delete", commands::ObjectDelete::default()))
        .add_command(&["object"], legacy_command("modify", commands::ObjectModify::default()))
        .add_command(&["object"], legacy_command("info", commands::ObjectInfo::default()));
}

fn add_relation_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["relation"],
            legacy_command("create", commands::RelationNew::default()),
        )
        .add_command(
            &["relation"],
            legacy_command("list", commands::RelationList::default()),
        )
        .add_command(
            &["relation"],
            legacy_command("delete", commands::RelationDelete::default()),
        )
        .add_command(
            &["relation"],
            legacy_command("info", commands::RelationInfo::default()),
        );
}

fn legacy_command<C>(name: &str, command: C) -> CommandSpec
where
    C: CliCommand + Clone + 'static,
{
    let options = command
        .options()
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
        about: command.about(),
        long_about: command.long_about(),
        examples: command.examples(),
        options,
        handler: Arc::new(LegacyCommandHandler {
            command: Arc::new(command),
        }) as Arc<dyn AsyncCommandHandler>,
    }
}

struct LegacyCommandHandler<C>
where
    C: CliCommand + Clone + 'static,
{
    command: Arc<C>,
}

#[async_trait]
impl<C> AsyncCommandHandler for LegacyCommandHandler<C>
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
            let cmd_name = invocation
                .command_path
                .last()
                .cloned()
                .ok_or_else(|| AppError::CommandExecutionError("Missing command name".to_string()))?;

            let tokens = crate::tokenizer::CommandTokenizer::new(
                &raw_line,
                &cmd_name,
                &command.options(),
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
