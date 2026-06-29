use hubuum_client::{GroupPermissionsResult, Namespace};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter};

transparent_record!(NamespaceRecord, Namespace);
transparent_record!(GroupPermissionsRecord, GroupPermissionsResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespacePermissionsView {
    pub entries: Vec<GroupPermissionsRecord>,
    pub summary: Vec<GroupPermissionsSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, EnumIter, Display)]
pub enum NamespacePermission {
    ReadCollection,
    UpdateCollection,
    DeleteCollection,
    DelegateCollection,
    CreateClass,
    ReadClass,
    UpdateClass,
    DeleteClass,
    CreateObject,
    ReadObject,
    UpdateObject,
    DeleteObject,
    CreateClassRelation,
    ReadClassRelation,
    UpdateClassRelation,
    DeleteClassRelation,
    CreateObjectRelation,
    ReadObjectRelation,
    UpdateObjectRelation,
    DeleteObjectRelation,
    ReadTemplate,
    CreateTemplate,
    UpdateTemplate,
    DeleteTemplate,
    ReadRemoteTarget,
    CreateRemoteTarget,
    UpdateRemoteTarget,
    DeleteRemoteTarget,
    ExecuteRemoteTarget,
}

impl NamespacePermission {
    pub fn api_name(self) -> String {
        self.to_string()
    }

    pub fn to_client(self) -> hubuum_client::Permissions {
        match self {
            Self::ReadCollection => hubuum_client::Permissions::ReadCollection,
            Self::UpdateCollection => hubuum_client::Permissions::UpdateCollection,
            Self::DeleteCollection => hubuum_client::Permissions::DeleteCollection,
            Self::DelegateCollection => hubuum_client::Permissions::DelegateCollection,
            Self::CreateClass => hubuum_client::Permissions::CreateClass,
            Self::ReadClass => hubuum_client::Permissions::ReadClass,
            Self::UpdateClass => hubuum_client::Permissions::UpdateClass,
            Self::DeleteClass => hubuum_client::Permissions::DeleteClass,
            Self::CreateObject => hubuum_client::Permissions::CreateObject,
            Self::ReadObject => hubuum_client::Permissions::ReadObject,
            Self::UpdateObject => hubuum_client::Permissions::UpdateObject,
            Self::DeleteObject => hubuum_client::Permissions::DeleteObject,
            Self::CreateClassRelation => hubuum_client::Permissions::CreateClassRelation,
            Self::ReadClassRelation => hubuum_client::Permissions::ReadClassRelation,
            Self::UpdateClassRelation => hubuum_client::Permissions::UpdateClassRelation,
            Self::DeleteClassRelation => hubuum_client::Permissions::DeleteClassRelation,
            Self::CreateObjectRelation => hubuum_client::Permissions::CreateObjectRelation,
            Self::ReadObjectRelation => hubuum_client::Permissions::ReadObjectRelation,
            Self::UpdateObjectRelation => hubuum_client::Permissions::UpdateObjectRelation,
            Self::DeleteObjectRelation => hubuum_client::Permissions::DeleteObjectRelation,
            Self::ReadTemplate => hubuum_client::Permissions::ReadTemplate,
            Self::CreateTemplate => hubuum_client::Permissions::CreateTemplate,
            Self::UpdateTemplate => hubuum_client::Permissions::UpdateTemplate,
            Self::DeleteTemplate => hubuum_client::Permissions::DeleteTemplate,
            Self::ReadRemoteTarget => hubuum_client::Permissions::ReadRemoteTarget,
            Self::CreateRemoteTarget => hubuum_client::Permissions::CreateRemoteTarget,
            Self::UpdateRemoteTarget => hubuum_client::Permissions::UpdateRemoteTarget,
            Self::DeleteRemoteTarget => hubuum_client::Permissions::DeleteRemoteTarget,
            Self::ExecuteRemoteTarget => hubuum_client::Permissions::ExecuteRemoteTarget,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPermissionsSummary {
    pub group: String,
    pub namespace: String,
    pub class: String,
    pub object: String,
    pub class_relation: String,
    pub object_relation: String,
    pub template: String,
    pub remote_target: String,
}

impl From<GroupPermissionsResult> for GroupPermissionsSummary {
    fn from(value: GroupPermissionsResult) -> Self {
        fn enabled(values: &[(&str, bool)]) -> String {
            values
                .iter()
                .filter_map(|(name, is_enabled)| is_enabled.then_some(*name))
                .collect::<Vec<_>>()
                .join(", ")
        }

        let permission = value.permission;
        Self {
            group: value.group.groupname,
            namespace: enabled(&[
                ("read", permission.has_read_namespace),
                ("update", permission.has_update_namespace),
                ("delete", permission.has_delete_namespace),
                ("delegate", permission.has_delegate_namespace),
            ]),
            class: enabled(&[
                ("create", permission.has_create_class),
                ("read", permission.has_read_class),
                ("update", permission.has_update_class),
                ("delete", permission.has_delete_class),
            ]),
            object: enabled(&[
                ("create", permission.has_create_object),
                ("read", permission.has_read_object),
                ("update", permission.has_update_object),
                ("delete", permission.has_delete_object),
            ]),
            class_relation: enabled(&[
                ("create", permission.has_create_class_relation),
                ("read", permission.has_read_class_relation),
                ("update", permission.has_update_class_relation),
                ("delete", permission.has_delete_class_relation),
            ]),
            object_relation: enabled(&[
                ("create", permission.has_create_object_relation),
                ("read", permission.has_read_object_relation),
                ("update", permission.has_update_object_relation),
                ("delete", permission.has_delete_object_relation),
            ]),
            template: enabled(&[
                ("create", permission.has_create_template),
                ("read", permission.has_read_template),
                ("update", permission.has_update_template),
                ("delete", permission.has_delete_template),
            ]),
            remote_target: enabled(&[
                ("create", permission.has_create_remote_target),
                ("read", permission.has_read_remote_target),
                ("update", permission.has_update_remote_target),
                ("delete", permission.has_delete_remote_target),
                ("execute", permission.has_execute_remote_target),
            ]),
        }
    }
}
