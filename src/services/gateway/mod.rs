mod classes;
mod groups;
mod namespaces;
mod objects;
mod relations;
mod shared;
mod users;

use std::sync::Arc;

use hubuum_client::{Authenticated, SyncClient};

pub use classes::{ClassFilter, CreateClassInput};
pub use groups::{CreateGroupInput, GroupFilter};
pub use namespaces::CreateNamespaceInput;
pub use objects::{CreateObjectInput, ObjectFilter, ObjectUpdateInput};
pub use relations::{RelationFilter, RelationTarget};
pub use users::{CreateUserInput, UserFilter};

#[derive(Clone)]
pub struct HubuumGateway {
    pub(super) client: Arc<SyncClient<Authenticated>>,
}

impl HubuumGateway {
    pub fn new(client: Arc<SyncClient<Authenticated>>) -> Self {
        Self { client }
    }
}
