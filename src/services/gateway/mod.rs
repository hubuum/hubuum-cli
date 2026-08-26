mod admin;
mod backups;
mod classes;
mod collections;
mod computed;
mod events;
mod exports;
mod groups;
mod identity;
mod imports;
mod object_aggregates;
mod objects;
mod principal_tokens;
mod relations;
mod remote_targets;
mod search;
mod service_accounts;
mod settings;
mod shared;
mod tasks;
mod tokens;
mod users;

use std::num::NonZeroUsize;
use std::sync::Arc;

use hubuum_client::{
    blocking::{Client as BlockingClient, GraphRequest},
    Authenticated,
};

use crate::errors::AppError;
use crate::list_query::{FilterFieldSpec, SortFieldSpec};

use super::AuthenticatedClient;

pub use backups::{BackupInput, RunBackupInput};
pub use classes::{ClassUpdateInput, CreateClassInput};
pub use collections::{CollectionUpdateInput, CreateCollectionInput};
pub use computed::{
    ComputedDefinitionInput, ComputedOperationInput, ComputedOperationKind, ComputedPatchInput,
    ComputedPreviewTarget, ComputedResultKind,
};
pub use events::{
    AuditActorKind, AuditListInput, AuditResourceKind, AuditScope, HistoryInput, HistoryScope,
};
pub use exports::{CreateExportTemplateInput, RunExportInput, UpdateExportTemplateInput};
pub use groups::{CreateGroupInput, GroupUpdateInput};
pub use imports::SubmitImportInput;
pub use object_aggregates::{
    ObjectAggregateDimensionInput, ObjectAggregateInput, ObjectAggregateMeasureInput,
    ObjectAggregateSortInput,
};
pub use objects::{CreateObjectInput, ObjectDataPatchInput, ObjectUpdateInput};
pub use principal_tokens::{
    CloneTokenInput, CloneTokenOutcome, NewTokenInput, RenewTokenInput, SourceTokenRevocation,
    TokenStateFilter,
};
pub use relations::{
    CreateClassRelationInput, RelatedObjectOptions, RelationRoot, RelationTarget,
    RelationTraversalOptions,
};
pub use remote_targets::{
    CreateRemoteTargetInput, InvokeRemoteTargetInput, RemoteAuthConfigInput,
    UpdateRemoteTargetInput,
};
pub use search::{SearchInput, SearchKind};
pub use service_accounts::CreateServiceAccountInput;
pub use tasks::{ListTasksInput, TaskLookupInput};
pub use users::{CreateUserInput, UserFilter, UserUpdateInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelatedGraphLimit(NonZeroUsize);

impl RelatedGraphLimit {
    pub fn new(value: usize) -> Result<Self, AppError> {
        NonZeroUsize::new(value).map(Self).ok_or_else(|| {
            AppError::InvalidOption("Related graph limit must be greater than zero".to_string())
        })
    }

    pub fn get(self) -> usize {
        self.0.get()
    }
}

fn apply_related_graph_limit<T>(
    request: GraphRequest<T>,
    limit: Option<RelatedGraphLimit>,
) -> GraphRequest<T> {
    match limit {
        Some(limit) => request.set_query_param("limit", limit.get()),
        None => request,
    }
}

#[derive(Clone)]
pub struct HubuumGateway {
    client: AuthenticatedClient,
}

impl HubuumGateway {
    #[cfg(test)]
    pub fn new(client: Arc<BlockingClient<Authenticated>>) -> Self {
        Self {
            client: AuthenticatedClient::new(client),
        }
    }

    pub(super) fn new_with_authenticated_client(client: AuthenticatedClient) -> Self {
        Self { client }
    }

    pub(super) fn replace_authenticated_client(&self, client: Arc<BlockingClient<Authenticated>>) {
        self.client.replace(client);
    }

    pub(super) fn client(&self) -> Arc<BlockingClient<Authenticated>> {
        self.client.current()
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
        [scope, command] if scope == "collection" && command == "list" => {
            Some(collections::COLLECTION_FILTER_SPECS)
        }
        [scope, command] if scope == "object" && command == "list" => {
            Some(objects::OBJECT_FILTER_SPECS)
        }
        [scope, command] if scope == "object" && command == "aggregate" => {
            Some(object_aggregates::OBJECT_AGGREGATE_FILTER_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "class" && command == "list" =>
        {
            Some(relations::RELATED_CLASS_FILTER_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "class" && command == "direct" =>
        {
            Some(relations::CLASS_RELATION_FILTER_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "class" && command == "graph" =>
        {
            Some(relations::RELATED_CLASS_FILTER_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "object" && command == "list" =>
        {
            Some(relations::RELATED_OBJECT_FILTER_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "object" && command == "direct" =>
        {
            Some(relations::OBJECT_RELATION_FILTER_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "object" && command == "graph" =>
        {
            Some(relations::RELATED_OBJECT_FILTER_SPECS)
        }
        [scope, command] if scope == "export" && command == "list" => {
            Some(exports::EXPORT_FILTER_SPECS)
        }
        [scope, command] if scope == "remote-target" && command == "list" => {
            Some(remote_targets::REMOTE_TARGET_FILTER_SPECS)
        }
        [scope, command] if scope == "user" && command == "list" => Some(users::USER_FILTER_SPECS),
        _ => None,
    }
}

pub(crate) fn sort_specs_for_command_path(
    command_path: &[String],
) -> Option<&'static [SortFieldSpec]> {
    match command_path {
        [scope, command] if scope == "class" && command == "list" => {
            Some(classes::CLASS_SORT_SPECS)
        }
        [scope, command] if scope == "group" && command == "list" => Some(groups::GROUP_SORT_SPECS),
        [scope, command] if scope == "collection" && command == "list" => {
            Some(collections::COLLECTION_SORT_SPECS)
        }
        [scope, command] if scope == "object" && command == "list" => {
            Some(objects::OBJECT_SORT_SPECS)
        }
        [scope, command] if scope == "object" && command == "aggregate" => {
            Some(object_aggregates::OBJECT_AGGREGATE_SORT_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "class" && command == "list" =>
        {
            Some(relations::RELATED_CLASS_SORT_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "class" && command == "direct" =>
        {
            Some(relations::CLASS_RELATION_SORT_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "object" && command == "list" =>
        {
            Some(relations::RELATED_OBJECT_SORT_SPECS)
        }
        [scope, subtype, command]
            if scope == "relation" && subtype == "object" && command == "direct" =>
        {
            Some(relations::OBJECT_RELATION_SORT_SPECS)
        }
        [scope, command] if scope == "export" && command == "list" => {
            Some(exports::EXPORT_SORT_SPECS)
        }
        [scope, command] if scope == "remote-target" && command == "list" => {
            Some(remote_targets::REMOTE_TARGET_SORT_SPECS)
        }
        [scope, command] if scope == "user" && command == "list" => Some(users::USER_SORT_SPECS),
        [scope, command] if scope == "task" && command == "events" => {
            Some(tasks::TASK_EVENT_SORT_SPECS)
        }
        [scope, command] if scope == "import" && command == "results" => {
            Some(imports::IMPORT_RESULT_SORT_SPECS)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RelatedGraphLimit;

    #[test]
    fn related_graph_limit_requires_a_positive_value() {
        assert_eq!(
            RelatedGraphLimit::new(250)
                .expect("positive limit should be valid")
                .get(),
            250
        );
        assert!(RelatedGraphLimit::new(0).is_err());
    }
}
