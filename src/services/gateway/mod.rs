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

pub use classes::{ClassFilter, ClassUpdateInput, CreateClassInput};
pub use groups::{CreateGroupInput, GroupFilter, GroupUpdateInput};
pub use imports::SubmitImportInput;
pub use namespaces::{CreateNamespaceInput, NamespaceUpdateInput};
pub use objects::{CreateObjectInput, ObjectFilter, ObjectUpdateInput};
pub use relations::{RelationFilter, RelationTarget};
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
