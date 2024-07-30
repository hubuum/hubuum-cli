use crate::commandlist::CommandList;
use crate::commands;

pub fn build_repl_commands() -> CommandList {
    let mut cli = CommandList::new();
    cli.add_scope("class")
        .add_command("create", commands::ClassNew::default());
    cli.add_scope("namespace")
        .add_command("create", commands::NamespaceNew::default());
    cli.add_command("help", commands::Help::default());
    cli
}
