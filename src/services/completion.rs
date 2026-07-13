use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tokio::runtime::Handle;
use tokio::task::spawn_blocking;

use crate::config::get_config;
use crate::domain::{JsonRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{ListQuery, SortClause, SortDirectionArg};
use crate::services::{AuditListInput, AuditScope, ListTasksInput};

use super::gateway::HubuumGateway;
use super::AppServices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub value: String,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct CompletionContext {
    services: Arc<AppServices>,
    runtime: Handle,
}

#[derive(Clone, Default)]
struct CompletionSnapshot {
    simple_sources: HashMap<CompletionKind, Vec<String>>,
    objects_by_class: HashMap<String, Vec<String>>,
    event_subscriptions_by_collection: HashMap<String, Vec<String>>,
    class_schemas: HashMap<String, Option<Value>>,
    task_ids: Option<Vec<CompletionItem>>,
    audit_event_ids: Option<Vec<String>>,
    event_delivery_ids: Option<Vec<String>>,
}

#[derive(Clone, Default)]
pub(crate) struct CompletionStore {
    snapshot: Arc<RwLock<CompletionSnapshot>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CompletionKind {
    Groups,
    Classes,
    Collections,
    EventSinks,
    ExportTemplates,
    Users,
    ServiceAccounts,
    RemoteTargets,
}

impl CompletionContext {
    pub(crate) fn new(services: Arc<AppServices>, runtime: Handle) -> Self {
        Self { services, runtime }
    }

    pub fn groups(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::Groups)
    }

    pub fn classes(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::Classes)
    }

    pub fn collections(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::Collections)
    }

    pub fn event_sinks(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::EventSinks)
    }

    pub fn export_templates(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::ExportTemplates)
    }

    pub fn users(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::Users)
    }

    pub fn service_accounts(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::ServiceAccounts)
    }

    pub fn remote_targets(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::RemoteTargets)
    }

    pub fn objects_from_class(&self, prefix: &str, parts: &[String], source: &str) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(class_name) = option_value(parts, source) else {
            return Vec::new();
        };

        if prefix.is_empty() {
            let fetched = self
                .runtime
                .block_on(
                    self.services
                        .completion_store()
                        .load_objects_for_class(self.services.gateway(), class_name),
                )
                .unwrap_or_default();
            return filter_prefix(&fetched, prefix);
        }

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .query_objects_for_class_prefix(
                        self.services.gateway(),
                        class_name,
                        prefix.to_string(),
                    ),
            )
            .unwrap_or_default()
    }

    pub fn event_subscriptions_from_collection(
        &self,
        prefix: &str,
        parts: &[String],
    ) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(collection) = option_value(parts, "--collection") else {
            return Vec::new();
        };

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_event_subscriptions_for_collection(self.services.gateway(), collection),
            )
            .map(|values| filter_prefix(&values, prefix))
            .unwrap_or_default()
    }

    pub fn task_ids(&self, prefix: &str) -> Vec<CompletionItem> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_task_id_items(self.services.gateway()),
            )
            .map(|items| filter_item_prefix(&items, prefix))
            .unwrap_or_default()
    }

    pub fn import_task_ids(&self, prefix: &str) -> Vec<CompletionItem> {
        self.task_ids(prefix)
            .into_iter()
            .filter(|item| {
                item.description
                    .as_deref()
                    .is_some_and(|description| description.starts_with("import "))
            })
            .collect()
    }

    pub fn audit_event_ids(&self, prefix: &str) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_audit_event_ids(self.services.gateway()),
            )
            .map(|ids| filter_prefix(&ids, prefix))
            .unwrap_or_default()
    }

    pub fn event_delivery_ids(&self, prefix: &str) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_event_delivery_ids(self.services.gateway()),
            )
            .map(|ids| filter_prefix(&ids, prefix))
            .unwrap_or_default()
    }

    pub fn class_schema(&self, class_name: &str) -> Option<Option<Value>> {
        if get_config().completion.disable_api_related {
            return None;
        }

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_class_schema(self.services.gateway(), class_name.to_string()),
            )
            .ok()
    }

    fn complete(&self, prefix: &str, kind: CompletionKind) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let fetched = self
            .runtime
            .block_on(
                self.services
                    .completion_store()
                    .load(self.services.gateway(), kind),
            )
            .unwrap_or_default();
        filter_prefix(&fetched, prefix)
    }
}

impl CompletionStore {
    pub(crate) fn invalidate_all(&self) {
        if let Ok(mut snapshot) = self.snapshot.write() {
            *snapshot = CompletionSnapshot::default();
        }
    }

    async fn load(
        &self,
        gateway: Arc<HubuumGateway>,
        kind: CompletionKind,
    ) -> Result<Vec<String>, AppError> {
        if let Some(cached) = self.cached(kind) {
            return Ok(cached);
        }

        let fetched = spawn_blocking(move || -> Result<Vec<String>, AppError> {
            match kind {
                CompletionKind::Groups => gateway.list_group_names(),
                CompletionKind::Classes => gateway.list_class_names(),
                CompletionKind::Collections => gateway.list_collection_names(),
                CompletionKind::EventSinks => gateway.list_event_sink_names(),
                CompletionKind::ExportTemplates => gateway.list_export_template_names(),
                CompletionKind::Users => gateway.list_user_names(),
                CompletionKind::ServiceAccounts => gateway.list_service_account_names(),
                CompletionKind::RemoteTargets => gateway.list_remote_target_names(),
            }
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.simple_sources.insert(kind, fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_objects_for_class(
        &self,
        gateway: Arc<HubuumGateway>,
        class_name: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.objects_by_class.get(&class_name) {
                return Ok(cached.clone());
            }
        }

        let cache_key = class_name.clone();
        let fetched = spawn_blocking(move || gateway.list_object_names_for_class(&class_name))
            .await
            .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.objects_by_class.insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn query_objects_for_class_prefix(
        &self,
        gateway: Arc<HubuumGateway>,
        class_name: String,
        prefix: String,
    ) -> Result<Vec<String>, AppError> {
        spawn_blocking(move || gateway.list_object_names_for_class_prefix(&class_name, &prefix))
            .await
            .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
    }

    async fn load_event_subscriptions_for_collection(
        &self,
        gateway: Arc<HubuumGateway>,
        collection: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.event_subscriptions_by_collection.get(&collection) {
                return Ok(cached.clone());
            }
        }

        let cache_key = collection.clone();
        let fetched = spawn_blocking(move || {
            gateway.list_event_subscription_names_for_collection(&collection)
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot
                .event_subscriptions_by_collection
                .insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_class_schema(
        &self,
        gateway: Arc<HubuumGateway>,
        class_name: String,
    ) -> Result<Option<Value>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.class_schemas.get(&class_name) {
                return Ok(cached.clone());
            }
        }

        let cache_key = class_name.clone();
        let fetched = spawn_blocking(move || gateway.class_schema(&class_name))
            .await
            .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.class_schemas.insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_task_id_items(
        &self,
        gateway: Arc<HubuumGateway>,
    ) -> Result<Vec<CompletionItem>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = &snapshot.task_ids {
                return Ok(cached.clone());
            }
        }

        let fetched = spawn_blocking(move || {
            let tasks = gateway.list_tasks(ListTasksInput {
                limit: Some(50),
                ..ListTasksInput::default()
            })?;
            Ok::<_, AppError>(
                tasks
                    .items
                    .into_iter()
                    .map(|task| CompletionItem {
                        value: task.0.id.to_string(),
                        description: Some(task_description(&task)),
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.task_ids = Some(fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_audit_event_ids(
        &self,
        gateway: Arc<HubuumGateway>,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = &snapshot.audit_event_ids {
                return Ok(cached.clone());
            }
        }

        let fetched = spawn_blocking(move || {
            let page = gateway.audit_events(
                AuditScope::Global,
                AuditListInput {
                    limit: Some(50),
                    sort: Some("-occurred_at".to_string()),
                    ..AuditListInput::default()
                },
            )?;
            Ok::<_, AppError>(
                page.items
                    .into_iter()
                    .filter_map(|record| json_record_i64(&record, &["id", "event_id"]))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.audit_event_ids = Some(fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_event_delivery_ids(
        &self,
        gateway: Arc<HubuumGateway>,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = &snapshot.event_delivery_ids {
                return Ok(cached.clone());
            }
        }

        let fetched = spawn_blocking(move || {
            let page = gateway.event_deliveries(&ListQuery {
                limit: Some(50),
                sorts: vec![SortClause {
                    field: "updated_at".to_string(),
                    direction: SortDirectionArg::Desc,
                }],
                ..ListQuery::default()
            })?;
            Ok::<_, AppError>(
                page.items
                    .into_iter()
                    .filter_map(|record| json_record_i64(&record, &["id"]))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.event_delivery_ids = Some(fetched.clone());
        }

        Ok(fetched)
    }

    fn cached(&self, kind: CompletionKind) -> Option<Vec<String>> {
        let Ok(snapshot) = self.snapshot.read() else {
            return None;
        };

        snapshot.simple_sources.get(&kind).cloned()
    }
}

fn filter_prefix(values: &[String], prefix: &str) -> Vec<String> {
    values
        .iter()
        .filter(|value| value.starts_with(prefix))
        .cloned()
        .collect()
}

fn filter_item_prefix(values: &[CompletionItem], prefix: &str) -> Vec<CompletionItem> {
    values
        .iter()
        .filter(|value| value.value.starts_with(prefix))
        .cloned()
        .collect()
}

fn option_value(parts: &[String], long: &str) -> Option<String> {
    parts.iter().enumerate().find_map(|(index, part)| {
        if part == long {
            parts.get(index + 1).cloned()
        } else {
            part.strip_prefix(&format!("{long}=")).map(str::to_string)
        }
    })
}

fn task_description(task: &TaskRecord) -> String {
    let mut parts = vec![task.0.kind.to_string(), task.0.status.to_string()];
    if let Some(summary) = task
        .0
        .summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
    {
        parts.push(summary.to_string());
    }
    parts.push(task.0.created_at.to_string());
    parts.join("  ")
}

fn json_record_i64(record: &JsonRecord, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| record.value.get(*key).and_then(Value::as_i64))
}

#[cfg(test)]
mod tests {
    use super::filter_prefix;

    #[test]
    fn filter_prefix_matches_start_of_value() {
        let values = vec![
            "alpha".to_string(),
            "beta".to_string(),
            "alpine".to_string(),
        ];

        assert_eq!(
            filter_prefix(&values, "al"),
            vec!["alpha".to_string(), "alpine".to_string()]
        );
    }
}
