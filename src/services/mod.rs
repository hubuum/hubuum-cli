mod completion;
mod gateway;

use std::sync::{Arc, RwLock};
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
    AuditActorKind, AuditListInput, AuditResourceKind, AuditScope, BackupInput, ClassUpdateInput,
    CloneTokenInput, CloneTokenOutcome, CollectionUpdateInput, ComputedDefinitionInput,
    ComputedOperationInput, ComputedOperationKind, ComputedPatchInput, ComputedPreviewTarget,
    ComputedResultKind, CreateClassInput, CreateClassRelationInput, CreateCollectionInput,
    CreateExportTemplateInput, CreateGroupInput, CreateObjectInput, CreateRemoteTargetInput,
    CreateServiceAccountInput, CreateUserInput, GroupUpdateInput, HistoryInput, HistoryScope,
    HubuumGateway, InvokeRemoteTargetInput, ListTasksInput, NewTokenInput,
    ObjectAggregateDimensionInput, ObjectAggregateInput, ObjectAggregateMeasureInput,
    ObjectAggregateSortInput, ObjectDataPatchInput, ObjectUpdateInput, RelatedObjectOptions,
    RelationRoot, RelationTarget, RelationTraversalOptions, RemoteAuthConfigInput, RenewTokenInput,
    RunBackupInput, RunExportInput, SearchInput, SearchKind, SourceTokenRevocation,
    SubmitImportInput, TaskLookupInput, TokenStateFilter, UpdateExportTemplateInput,
    UpdateRemoteTargetInput, UserFilter, UserUpdateInput,
};

#[derive(Debug, Clone)]
pub struct WaitTaskInput {
    pub task_id: i32,
    pub timeout_secs: Option<u64>,
    pub poll_interval_secs: Option<u64>,
}

#[derive(Clone)]
pub(super) struct AuthenticatedClient {
    inner: Arc<RwLock<Arc<BlockingClient<Authenticated>>>>,
}

impl AuthenticatedClient {
    fn new(client: Arc<BlockingClient<Authenticated>>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(client)),
        }
    }

    fn current(&self) -> Arc<BlockingClient<Authenticated>> {
        self.inner
            .read()
            .expect("authenticated client lock poisoned")
            .clone()
    }

    fn replace(&self, client: Arc<BlockingClient<Authenticated>>) {
        *self
            .inner
            .write()
            .expect("authenticated client lock poisoned") = client;
    }
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
        let client = AuthenticatedClient::new(client);
        let gateway = Arc::new(HubuumGateway::new_with_authenticated_client(client));
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
        self.completion.invalidate_volatile();
    }

    pub fn replace_authenticated_client(&self, client: Arc<BlockingClient<Authenticated>>) {
        self.gateway.replace_authenticated_client(client);
        self.invalidate_completion();
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use hubuum_client::{blocking::Client, Authenticated, MockTransport, Token};

    use super::AppServices;

    #[test]
    fn replacing_authenticated_client_updates_existing_gateway() {
        let old_client = authenticated_client("old-token");
        let new_client = authenticated_client("new-token");
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let services =
            AppServices::new(old_client, runtime.handle().clone(), Duration::from_secs(1));
        let gateway = services.gateway();

        assert_eq!(gateway.client().token(), "old-token");
        services.replace_authenticated_client(new_client);
        assert_eq!(gateway.client().token(), "new-token");
    }

    fn authenticated_client(token: &str) -> Arc<Client<Authenticated>> {
        Arc::new(
            Client::builder_from_url("https://example.invalid")
                .expect("base URL should parse")
                .with_transport(Arc::new(MockTransport::default()))
                .build()
                .expect("client should build")
                .authenticate(Token::new(token)),
        )
    }
}
