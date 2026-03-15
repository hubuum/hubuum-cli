mod classes;
mod groups;
mod imports;
mod namespaces;
mod objects;
mod relations;
mod reports;
mod search;
mod shared;
mod tasks;
mod users;

use std::sync::Arc;

use hubuum_client::{Authenticated, SyncClient};

use crate::list_query::{FilterFieldSpec, SortFieldSpec};

pub use classes::{ClassUpdateInput, CreateClassInput};
pub use groups::{CreateGroupInput, GroupUpdateInput};
pub use imports::SubmitImportInput;
pub use namespaces::{CreateNamespaceInput, NamespaceUpdateInput};
pub use objects::{CreateObjectInput, ObjectUpdateInput};
pub use relations::{RelatedObjectOptions, RelationRoot, RelationTarget, RelationTraversalOptions};
pub use reports::{CreateReportTemplateInput, RunReportInput, UpdateReportTemplateInput};
pub use search::{SearchInput, SearchKind};
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
        [scope, command] if scope == "report" && command == "list" => {
            Some(reports::REPORT_FILTER_SPECS)
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
        [scope, command] if scope == "namespace" && command == "list" => {
            Some(namespaces::NAMESPACE_SORT_SPECS)
        }
        [scope, command] if scope == "object" && command == "list" => {
            Some(objects::OBJECT_SORT_SPECS)
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
        [scope, command] if scope == "report" && command == "list" => {
            Some(reports::REPORT_SORT_SPECS)
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
