macro_rules! transparent_record {
    ($name:ident, $inner:path) => {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

mod classes;
mod groups;
mod namespaces;
mod objects;
mod relations;
mod users;

pub use classes::{ClassDetails, ClassRecord};
pub use groups::{GroupDetails, GroupRecord};
pub use namespaces::{
    GroupPermissionsRecord, GroupPermissionsSummary, NamespacePermission, NamespacePermissionsView,
    NamespaceRecord,
};
pub use objects::{ObjectRecord, ResolvedObjectRecord};
pub use relations::{ResolvedClassRelationRecord, ResolvedObjectRelationRecord};
pub use users::{CreatedUser, UserRecord};
