use crate::commandlist::CommandList;
use crate::commands;

pub fn build_repl_commands() -> CommandList {
    let mut cli = CommandList::new();

    add_class_commands(&mut cli);
    add_namespace_commands(&mut cli);
    add_user_commands(&mut cli);
    add_group_commands(&mut cli);

    cli.add_command("help", commands::Help::default());

    cli
}

fn add_class_commands(cli: &mut CommandList) {
    cli.add_scope("class")
        .add_command("create", commands::ClassNew::default())
        .add_command("list", commands::ClassList::default())
        .add_command("delete", commands::ClassDelete::default())
        .add_command("info", commands::ClassInfo::default());
}

fn add_namespace_commands(cli: &mut CommandList) {
    cli.add_scope("namespace")
        .add_command("create", commands::NamespaceNew::default())
        .add_command("list", commands::NamespaceList::default())
        .add_command("delete", commands::NamespaceDelete::default())
        .add_command("info", commands::NamespaceInfo::default());
}

fn add_user_commands(cli: &mut CommandList) {
    cli.add_scope("user")
        .add_command("create", commands::UserNew::default())
        .add_command("list", commands::UserList::default())
        .add_command("delete", commands::UserDelete::default())
        .add_command("info", commands::UserInfo::default());
}

fn add_group_commands(cli: &mut CommandList) {
    cli.add_scope("group")
        .add_command("create", commands::GroupNew::default())
        .add_command("list", commands::GroupList::default());
}
