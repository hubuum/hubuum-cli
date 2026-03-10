mod completion;
mod gateway;

use std::sync::Arc;

use hubuum_client::{Authenticated, SyncClient};
use tokio::runtime::Handle;

use crate::config::AppConfig;

pub use completion::CompletionContext;
pub use gateway::{
    ClassFilter, ClassUpdateInput, CreateClassInput, CreateGroupInput, CreateNamespaceInput,
    CreateObjectInput, CreateUserInput, GroupFilter, GroupUpdateInput, HubuumGateway,
    NamespaceUpdateInput, ObjectFilter, ObjectUpdateInput, RelationFilter, RelationTarget,
    UserFilter, UserUpdateInput,
};

#[derive(Clone)]
pub struct AppServices {
    gateway: Arc<HubuumGateway>,
    completion: completion::CompletionStore,
}

impl AppServices {
    pub fn new(client: Arc<SyncClient<Authenticated>>) -> Self {
        Self {
            gateway: Arc::new(HubuumGateway::new(client)),
            completion: completion::CompletionStore::default(),
        }
    }

    pub fn gateway(&self) -> Arc<HubuumGateway> {
        self.gateway.clone()
    }

    pub fn completion_context(
        self: &Arc<Self>,
        runtime: Handle,
        config: &AppConfig,
    ) -> CompletionContext {
        CompletionContext::new(self.clone(), runtime, config.completion.disable_api_related)
    }

    pub fn invalidate_completion(&self) {
        self.completion.invalidate_all();
    }

    pub(crate) fn completion_store(&self) -> completion::CompletionStore {
        self.completion.clone()
    }
}
