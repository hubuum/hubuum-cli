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
mod imports;
mod namespaces;
mod objects;
mod relations;
mod reports;
mod search;
mod tasks;
mod users;

pub use classes::{ClassDetails, ClassRecord};
pub use groups::{GroupDetails, GroupRecord};
pub use imports::ImportResultRecord;
pub use namespaces::{
    GroupPermissionsRecord, GroupPermissionsSummary, NamespacePermission, NamespacePermissionsView,
    NamespaceRecord,
};
pub use objects::{ObjectRecord, ResolvedObjectRecord};
pub use relations::{
    ResolvedClassRelationRecord, ResolvedObjectRelationRecord, ResolvedRelatedClassGraph,
    ResolvedRelatedClassRecord, ResolvedRelatedObjectGraph, ResolvedRelatedObjectRecord,
};
pub use reports::{ReportOutput, ReportTemplateRecord};
pub use search::{
    SearchBatchRecord, SearchCursorSet, SearchErrorEvent, SearchQueryEvent, SearchResponseRecord,
    SearchResultsRecord, SearchStreamEvent,
};
pub use tasks::{TaskEventRecord, TaskQueueStateRecord, TaskRecord};
pub use users::{CreatedUser, UserRecord};
