mod completion;
mod gateway;

use std::sync::Arc;
use std::time::Duration;

use hubuum_client::{Authenticated, SyncClient};
use tokio::runtime::Handle;

use crate::background::BackgroundManager;
use crate::config::AppConfig;

pub use completion::CompletionContext;
pub(crate) use gateway::filter_specs_for_command_path;
pub(crate) use gateway::sort_specs_for_command_path;
pub use gateway::{
    ClassUpdateInput, CreateClassInput, CreateGroupInput, CreateNamespaceInput, CreateObjectInput,
    CreateReportTemplateInput, CreateUserInput, GroupUpdateInput, HubuumGateway,
    NamespaceUpdateInput, ObjectUpdateInput, RelatedObjectOptions, RelationRoot, RelationTarget,
    RelationTraversalOptions, RunReportInput, SearchInput, SearchKind, SubmitImportInput,
    TaskLookupInput, UpdateReportTemplateInput, UserFilter, UserUpdateInput,
};

#[derive(Clone)]
pub struct AppServices {
    gateway: Arc<HubuumGateway>,
    background: BackgroundManager,
    completion: completion::CompletionStore,
}

impl AppServices {
    pub fn new(
        client: Arc<SyncClient<Authenticated>>,
        runtime: Handle,
        background_poll_interval: Duration,
    ) -> Self {
        let gateway = Arc::new(HubuumGateway::new(client));
        Self {
            background: BackgroundManager::new(runtime, gateway.clone(), background_poll_interval),
            gateway,
            completion: completion::CompletionStore::default(),
        }
    }

    pub fn gateway(&self) -> Arc<HubuumGateway> {
        self.gateway.clone()
    }

    pub fn background(&self) -> BackgroundManager {
        self.background.clone()
    }

    pub fn completion_context(
        self: &Arc<Self>,
        runtime: Handle,
        _config: &AppConfig,
    ) -> CompletionContext {
        CompletionContext::new(self.clone(), runtime)
    }

    pub fn invalidate_completion(&self) {
        self.completion.invalidate_all();
    }

    pub(crate) fn completion_store(&self) -> completion::CompletionStore {
        self.completion.clone()
    }
}
