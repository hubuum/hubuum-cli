mod completion;
mod gateway;

use std::sync::Arc;
use std::time::Duration;

use hubuum_client::{blocking::Client as BlockingClient, Authenticated};
use tokio::runtime::Handle;

use crate::background::BackgroundManager;
use crate::config::{get_config, AppConfig, UserPreferences};
use crate::errors::AppError;

pub use completion::CompletionContext;
use completion::CompletionStore;
pub(crate) use gateway::filter_specs_for_command_path;
pub(crate) use gateway::sort_specs_for_command_path;
pub use gateway::{
    AuditListInput, AuditScope, BackupInput, ClassUpdateInput, CollectionUpdateInput,
    ComputedDefinitionInput, ComputedOperationInput, ComputedOperationKind, ComputedPatchInput,
    ComputedPreviewTarget, ComputedResultKind, CreateClassInput, CreateCollectionInput,
    CreateExportTemplateInput, CreateGroupInput, CreateObjectInput, CreateRemoteTargetInput,
    CreateServiceAccountInput, CreateUserInput, GroupUpdateInput, HistoryInput, HistoryScope,
    HubuumGateway, InvokeRemoteTargetInput, ListTasksInput, NewTokenInput, ObjectDataPatchInput,
    ObjectUpdateInput, RelatedObjectOptions, RelationRoot, RelationTarget,
    RelationTraversalOptions, RemoteAuthConfigInput, RunBackupInput, RunExportInput, SearchInput,
    SearchKind, SubmitImportInput, TaskLookupInput, UpdateExportTemplateInput,
    UpdateRemoteTargetInput, UserFilter, UserUpdateInput,
};

#[derive(Debug, Clone)]
pub struct WaitTaskInput {
    pub task_id: i32,
    pub timeout_secs: Option<u64>,
    pub poll_interval_secs: Option<u64>,
}

#[derive(Clone)]
pub struct AppServices {
    gateway: Arc<HubuumGateway>,
    background: BackgroundManager,
    completion: CompletionStore,
}

impl AppServices {
    pub fn new(
        client: Arc<BlockingClient<Authenticated>>,
        runtime: Handle,
        background_poll_interval: Duration,
    ) -> Self {
        let gateway = Arc::new(HubuumGateway::new(client));
        Self {
            background: BackgroundManager::new(runtime, gateway.clone(), background_poll_interval),
            gateway,
            completion: CompletionStore::default(),
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

    pub fn sync_user_preferences_if_enabled(&self) -> Result<(), AppError> {
        let config = get_config();
        if config.settings.store_on_server {
            self.gateway
                .store_user_preferences(&UserPreferences::from_config(&config))?;
        }
        Ok(())
    }

    pub(crate) fn completion_store(&self) -> CompletionStore {
        self.completion.clone()
    }
}
