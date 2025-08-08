use std::sync::Arc;

use hubuum_client::{Authenticated, SyncClient};

use crate::commandlist::CommandList;
use crate::commands;

pub fn build_repl_commands(client: Arc<SyncClient<Authenticated>>) -> CommandList {
    let mut cli = CommandList::new(client);

    add_class_commands(&mut cli);
    add_namespace_commands(&mut cli);
    add_user_commands(&mut cli);
    add_group_commands(&mut cli);
    add_object_commands(&mut cli);
    add_relation_commands(&mut cli);

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
        .add_command("info", commands::NamespaceInfo::default())
        .add_scope("permissions")
        .add_command("list", commands::NamespacePermissions::default())
        .add_command("set", commands::NamespacePermissionsSet::default());
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
        .add_command("list", commands::GroupList::default())
        .add_command("add_user", commands::GroupAddUser::default())
        .add_command("remove_user", commands::GroupRemoveUser::default())
        .add_command("info", commands::GroupInfo::default());
}

fn add_object_commands(cli: &mut CommandList) {
    cli.add_scope("object")
        .add_command("create", commands::ObjectNew::default())
        .add_command("list", commands::ObjectList::default())
        .add_command("delete", commands::ObjectDelete::default())
        .add_command("modify", commands::ObjectModify::default())
        .add_command("info", commands::ObjectInfo::default());
}

fn add_relation_commands(cli: &mut CommandList) {
    cli.add_scope("relation")
        .add_command("create", commands::RelationNew::default())
        .add_command("list", commands::RelationList::default())
        .add_command("delete", commands::RelationDelete::default())
        .add_command("info", commands::RelationInfo::default());
}
