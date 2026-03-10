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
}

impl NamespacePermission {
    pub fn api_name(self) -> String {
        self.to_string()
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
        }
    }
}
