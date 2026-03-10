mod classes;
mod groups;
mod imports;
mod namespaces;
mod objects;
mod relations;
mod reports;
mod shared;
mod tasks;
mod users;

use std::sync::Arc;

use hubuum_client::{Authenticated, SyncClient};

use crate::list_query::FilterFieldSpec;

pub use classes::{ClassUpdateInput, CreateClassInput};
pub use groups::{CreateGroupInput, GroupUpdateInput};
pub use imports::SubmitImportInput;
pub use namespaces::{CreateNamespaceInput, NamespaceUpdateInput};
pub use objects::{CreateObjectInput, ObjectUpdateInput};
pub use relations::RelationTarget;
pub use reports::{CreateReportTemplateInput, RunReportInput, UpdateReportTemplateInput};
pub use tasks::TaskLookupInput;
pub use users::{CreateUserInput, UserFilter, UserUpdateInput};

#[derive(Clone)]
pub struct HubuumGateway {
    pub(super) client: Arc<SyncClient<Authenticated>>,
}

impl HubuumGateway {
    pub fn new(client: Arc<SyncClient<Authenticated>>) -> Self {
        Self { client }
    }
}

pub(crate) fn filter_specs_for_command_path(
    command_path: &[String],
) -> Option<&'static [FilterFieldSpec]> {
    match command_path {
        [scope, command] if scope == "class" && command == "list" => {
            Some(classes::CLASS_FILTER_SPECS)
        }
        [scope, command] if scope == "group" && command == "list" => {
            Some(groups::GROUP_FILTER_SPECS)
        }
        [scope, command] if scope == "namespace" && command == "list" => {
            Some(namespaces::NAMESPACE_FILTER_SPECS)
        }
        [scope, command] if scope == "object" && command == "list" => {
            Some(objects::OBJECT_FILTER_SPECS)
        }
        [scope, command] if scope == "relation" && command == "list" => {
            Some(relations::RELATION_FILTER_SPECS)
        }
        [scope, command] if scope == "report" && command == "list" => {
            Some(reports::REPORT_FILTER_SPECS)
        }
        [scope, command] if scope == "user" && command == "list" => Some(users::USER_FILTER_SPECS),
        _ => None,
    }
}
