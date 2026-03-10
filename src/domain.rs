use std::collections::HashMap;

use hubuum_client::{
    Class, ClassRelation, Group, GroupPermissionsResult, Namespace, Object, ObjectRelation, User,
};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter};

macro_rules! transparent_record {
    ($name:ident, $inner:path) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $inner);

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<&$inner> for $name {
            fn from(value: &$inner) -> Self {
                Self(value.clone())
            }
        }
    };
}

transparent_record!(ClassRecord, Class);
transparent_record!(GroupRecord, Group);
transparent_record!(NamespaceRecord, Namespace);
transparent_record!(ObjectRecord, Object);
transparent_record!(UserRecord, User);
transparent_record!(GroupPermissionsRecord, GroupPermissionsResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDetails {
    pub class: ClassRecord,
    pub objects: Vec<ObjectRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDetails {
    pub group: GroupRecord,
    pub members: Vec<UserRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedUser {
    pub user: UserRecord,
    pub password: String,
}

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
pub struct ResolvedObjectRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub class: String,
    pub data: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedObjectRecord {
    pub fn new(
        object: &Object,
        classmap: &HashMap<i32, Class>,
        namespacemap: &HashMap<i32, Namespace>,
    ) -> Self {
        let namespace = namespacemap
            .get(&object.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        let class = classmap
            .get(&object.hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        Self {
            id: object.id,
            name: object.name.clone(),
            description: object.description.clone(),
            namespace,
            class,
            data: object.data.clone(),
            created_at: object.created_at.to_string(),
            updated_at: object.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedClassRelationRecord {
    pub id: i32,
    pub from_class: String,
    pub to_class: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedClassRelationRecord {
    pub fn new(class_relation: &ClassRelation, classmap: &HashMap<i32, Class>) -> Self {
        let from_class = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let to_class = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();

        Self {
            id: class_relation.id,
            from_class,
            to_class,
            created_at: class_relation.created_at.to_string(),
            updated_at: class_relation.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedObjectRelationRecord {
    pub id: i32,
    pub from_class: String,
    pub to_class: String,
    pub from_object: String,
    pub to_object: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedObjectRelationRecord {
    pub fn new(
        object_relation: &ObjectRelation,
        class_relation: &ClassRelation,
        objectmap: &HashMap<i32, Object>,
        classmap: &HashMap<i32, Class>,
    ) -> Self {
        let from_class = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let to_class = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let from_object = objectmap
            .get(&object_relation.from_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        let to_object = objectmap
            .get(&object_relation.to_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();

        Self {
            id: object_relation.id,
            from_class,
            to_class,
            from_object,
            to_object,
            created_at: object_relation.created_at.to_string(),
            updated_at: object_relation.updated_at.to_string(),
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
