use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::runtime::Handle;
use tokio::task::spawn_blocking;

use crate::config::get_config;
use crate::domain::{
    JsonRecord, ObservedObjectDataFields, TaskRecord, DEFAULT_OBJECT_FIELD_DEPTH,
    DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT,
};
use crate::errors::AppError;
use crate::json_schema::{schema_json_pointers, schema_paths};
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
    observed_fields_by_class: HashMap<String, TimedCompletion<ObservedObjectDataFields>>,
    computed_sort_fields_by_class: HashMap<String, Vec<String>>,
    task_ids: Option<Vec<CompletionItem>>,
    audit_event_ids: Option<Vec<String>>,
    event_delivery_ids: Option<Vec<String>>,
    user_token_ids: HashMap<String, Vec<String>>,
    service_account_token_ids: HashMap<String, Vec<String>>,
}

#[derive(Clone)]
struct TimedCompletion<T> {
    value: T,
    cached_at: Instant,
}

impl<T: Clone> TimedCompletion<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            cached_at: Instant::now(),
        }
    }

    #[cfg(test)]
    fn new_at(value: T, cached_at: Instant) -> Self {
        Self { value, cached_at }
    }

    fn fresh_value(&self, ttl: Duration, now: Instant) -> Option<T> {
        (now.saturating_duration_since(self.cached_at) < ttl).then(|| self.value.clone())
    }
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

    pub fn user_token_ids(&self, prefix: &str, parts: &[String]) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(username) = token_principal_name(parts, &["--username", "-u"]) else {
            return Vec::new();
        };
        let fetched = self
            .runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_user_token_ids(self.services.gateway(), username),
            )
            .unwrap_or_default();
        filter_prefix(&fetched, prefix)
    }

    pub fn service_account_token_ids(&self, prefix: &str, parts: &[String]) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(name) = token_principal_name(parts, &["--name", "-n"]) else {
            return Vec::new();
        };
        let fetched = self
            .runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_service_account_token_ids(self.services.gateway(), name),
            )
            .unwrap_or_default();
        filter_prefix(&fetched, prefix)
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

    pub fn computed_field_paths(&self, prefix: &str, parts: &[String]) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(class_name) = class_name_from_parts(parts) else {
            return Vec::new();
        };

        let schema = self.class_schema(&class_name).flatten();
        let observed = self.observed_fields_for_class(class_name);
        let pointers = merged_json_pointers(
            schema.as_ref(),
            observed
                .as_ref()
                .map(ObservedObjectDataFields::json_pointers)
                .unwrap_or_default(),
        );

        json_pointer_completion_candidates(&pointers, prefix)
    }

    pub fn object_data_fields(&self, parts: &[String]) -> Vec<String> {
        let Some(class_name) = class_name_from_parts(parts) else {
            return Vec::new();
        };
        self.object_data_fields_for_class(&class_name)
    }

    pub fn object_data_fields_for_class(&self, class_name: &str) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let schema = self.class_schema(class_name).flatten();
        let observed = self.observed_fields_for_class(class_name.to_string());
        merged_aggregate_data_fields(
            schema.as_ref(),
            observed
                .as_ref()
                .map(ObservedObjectDataFields::aggregate_paths)
                .unwrap_or_default(),
        )
    }

    pub fn computed_sort_fields(&self, parts: &[String]) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }
        let Some(class_name) = class_name_from_parts(parts) else {
            return Vec::new();
        };
        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_computed_sort_fields(self.services.gateway(), class_name),
            )
            .unwrap_or_default()
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

    fn observed_fields_for_class(&self, class_name: String) -> Option<ObservedObjectDataFields> {
        let ttl = observed_field_cache_ttl();
        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_observed_fields_for_class(self.services.gateway(), class_name, ttl),
            )
            .ok()
    }
}

impl CompletionStore {
    pub(crate) fn invalidate_volatile(&self) {
        if let Ok(mut snapshot) = self.snapshot.write() {
            let observed_fields_by_class = std::mem::take(&mut snapshot.observed_fields_by_class);
            *snapshot = CompletionSnapshot::default();
            snapshot.observed_fields_by_class = observed_fields_by_class;
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

    async fn load_user_token_ids(
        &self,
        gateway: Arc<HubuumGateway>,
        username: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.user_token_ids.get(&username) {
                return Ok(cached.clone());
            }
        }

        let cache_key = username.clone();
        let fetched = spawn_blocking(move || gateway.list_user_token_ids(&username))
            .await
            .map_err(|error| AppError::CommandExecutionError(error.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.user_token_ids.insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_service_account_token_ids(
        &self,
        gateway: Arc<HubuumGateway>,
        name: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.service_account_token_ids.get(&name) {
                return Ok(cached.clone());
            }
        }

        let cache_key = name.clone();
        let fetched = spawn_blocking(move || gateway.list_service_account_token_ids(&name))
            .await
            .map_err(|error| AppError::CommandExecutionError(error.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot
                .service_account_token_ids
                .insert(cache_key, fetched.clone());
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

    async fn load_observed_fields_for_class(
        &self,
        gateway: Arc<HubuumGateway>,
        class_name: String,
        ttl: Option<Duration>,
    ) -> Result<ObservedObjectDataFields, AppError> {
        if let Some(ttl) = ttl {
            if let Ok(snapshot) = self.snapshot.read() {
                if let Some(cached) = snapshot.observed_fields_by_class.get(&class_name) {
                    if let Some(value) = cached.fresh_value(ttl, Instant::now()) {
                        return Ok(value);
                    }
                }
            }
        }

        let cache_key = class_name.clone();
        let fetched = spawn_blocking(move || {
            gateway.observed_object_data_fields(
                &class_name,
                DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT,
                DEFAULT_OBJECT_FIELD_DEPTH,
            )
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            if ttl.is_some() {
                snapshot
                    .observed_fields_by_class
                    .insert(cache_key, TimedCompletion::new(fetched.clone()));
            } else {
                snapshot.observed_fields_by_class.remove(&cache_key);
            }
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
                    .map(|record| record.id().to_string())
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

    async fn load_computed_sort_fields(
        &self,
        gateway: Arc<HubuumGateway>,
        class_name: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.computed_sort_fields_by_class.get(&class_name) {
                return Ok(cached.clone());
            }
        }

        let lookup_class = class_name.clone();
        let fetched = spawn_blocking(move || {
            let shared = gateway.list_shared_computed_fields(&lookup_class)?;
            let personal = gateway.list_personal_computed_fields(
                Some(&lookup_class),
                &ListQuery {
                    limit: Some(100),
                    ..ListQuery::default()
                },
            );
            let mut fields = BTreeSet::new();
            fields.extend(
                shared
                    .definitions
                    .into_iter()
                    .filter(|field| field.enabled)
                    .map(|field| format!("S:{}", field.key)),
            );
            if let Ok(personal) = personal {
                fields.extend(
                    personal
                        .items
                        .into_iter()
                        .filter(|field| field.enabled)
                        .map(|field| format!("P:{}", field.key)),
                );
            }
            Ok::<_, AppError>(fields.into_iter().collect::<Vec<_>>())
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot
                .computed_sort_fields_by_class
                .insert(class_name, fetched.clone());
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

fn token_principal_name(parts: &[String], option_names: &[&str]) -> Option<String> {
    for option_name in option_names {
        if let Some(value) = option_value(parts, option_name) {
            return Some(value);
        }
    }

    parts
        .iter()
        .position(|part| matches!(part.as_str(), "show" | "clone" | "revoke"))
        .and_then(|command_index| parts.get(command_index + 1))
        .filter(|value| !value.starts_with('-'))
        .cloned()
}

fn class_name_from_parts(parts: &[String]) -> Option<String> {
    parts.iter().enumerate().find_map(|(index, part)| {
        if part == "--class" || part == "-c" {
            parts.get(index + 1).cloned()
        } else {
            part.strip_prefix("--class=")
                .or_else(|| part.strip_prefix("-c="))
                .map(str::to_string)
        }
    })
}

fn json_pointer_completion_candidates(pointers: &[String], prefix: &str) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    for pointer in pointers {
        candidates.insert(pointer.clone());
        let child_prefix = format!("{pointer}/");
        if pointers
            .iter()
            .any(|other| other.starts_with(&child_prefix))
        {
            candidates.insert(child_prefix);
        }
    }

    candidates
        .into_iter()
        .filter(|candidate| candidate.starts_with(prefix))
        .filter(|candidate| !(prefix.ends_with('/') && candidate == prefix))
        .collect()
}

fn merged_json_pointers(schema: Option<&Value>, observed: &[String]) -> Vec<String> {
    let mut pointers = BTreeSet::new();
    if let Some(schema) = schema {
        pointers.extend(schema_json_pointers(schema));
    }
    pointers.extend(observed.iter().cloned());
    pointers.into_iter().collect()
}

fn merged_aggregate_data_fields(schema: Option<&Value>, observed: &[String]) -> Vec<String> {
    let mut fields = BTreeSet::new();
    if let Some(schema) = schema {
        fields.extend(
            schema_paths(schema, false)
                .into_iter()
                .map(|path| format!("data.{path}")),
        );
    }
    fields.extend(observed.iter().cloned());
    fields.into_iter().collect()
}

fn observed_field_cache_ttl() -> Option<Duration> {
    let config = get_config();
    (!config.cache.disable && config.cache.time > 0).then(|| Duration::from_secs(config.cache.time))
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
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::StatusCode;
    use serde_json::json;

    use super::{
        filter_prefix, json_pointer_completion_candidates, merged_aggregate_data_fields,
        merged_json_pointers, token_principal_name, CompletionKind, CompletionStore, HubuumGateway,
        ObservedObjectDataFields, TimedCompletion,
    };

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

    #[test]
    fn token_principal_name_accepts_options_and_positionals() {
        assert_eq!(
            token_principal_name(
                &[
                    "service-account".to_string(),
                    "token".to_string(),
                    "show".to_string(),
                    "--name".to_string(),
                    "mi-ansible-facts".to_string(),
                    "--token-id".to_string(),
                ],
                &["--name", "-n"],
            )
            .as_deref(),
            Some("mi-ansible-facts")
        );
        assert_eq!(
            token_principal_name(
                &[
                    "clone".to_string(),
                    "mi-ansible-facts".to_string(),
                    "--token-id".to_string(),
                ],
                &["--name", "-n"],
            )
            .as_deref(),
            Some("mi-ansible-facts")
        );
    }

    #[test]
    fn service_account_token_ids_are_loaded_and_cached() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!([{
                    "id": 7,
                    "name": "mi-ansible-facts",
                    "description": "",
                    "owner_group_id": 2,
                    "created_by": 1,
                    "disabled_at": null,
                    "created_at": "2026-07-25T12:00:00Z",
                    "updated_at": "2026-07-25T12:00:00Z"
                }]),
            )
            .expect("service-account response should serialize"),
        );
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!([{
                    "id": 20,
                    "principal_id": 7,
                    "name": "ansible-facts",
                    "description": null,
                    "scope": {"permissions": ["ReadObject"]},
                    "issued": "2026-07-25T18:43:41Z",
                    "expires_at": "2029-01-01T20:42:00Z",
                    "last_used_at": null,
                    "revoked_at": null
                }]),
            )
            .expect("token response should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = Arc::new(HubuumGateway::new(Arc::new(client)));
        let store = CompletionStore::default();
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let first =
            runtime
                .block_on(store.load_service_account_token_ids(
                    gateway.clone(),
                    "mi-ansible-facts".to_string(),
                ))
                .expect("token IDs should load");
        let second = runtime
            .block_on(store.load_service_account_token_ids(gateway, "mi-ansible-facts".to_string()))
            .expect("token IDs should be cached");

        assert_eq!(first, ["20"]);
        assert_eq!(second, ["20"]);
        assert_eq!(transport.requests().len(), 2);
    }

    #[test]
    fn pointer_completion_supports_nested_expansion() {
        let pointers = vec![
            "/load".to_string(),
            "/load/five".to_string(),
            "/load/one".to_string(),
            "/owner".to_string(),
        ];

        assert_eq!(
            json_pointer_completion_candidates(&pointers, "/lo"),
            vec![
                "/load".to_string(),
                "/load/".to_string(),
                "/load/five".to_string(),
                "/load/one".to_string(),
            ]
        );
        assert_eq!(
            json_pointer_completion_candidates(&pointers, "/load/"),
            vec!["/load/five".to_string(), "/load/one".to_string()]
        );
    }

    #[test]
    fn schema_and_observed_paths_are_merged() {
        let schema = json!({
            "properties": {
                "schema_only": {"type": "string"},
                "shared": {"type": "string"}
            }
        });

        let pointers = merged_json_pointers(
            Some(&schema),
            &["/observed_only".to_string(), "/shared".to_string()],
        );

        assert_eq!(
            pointers,
            vec![
                "/observed_only".to_string(),
                "/schema_only".to_string(),
                "/shared".to_string(),
            ]
        );
    }

    #[test]
    fn aggregate_fields_merge_schema_and_observed_data() {
        let schema = json!({
            "properties": {
                "schema_only": {"type": "string"},
                "shared": {"type": "string"}
            }
        });

        assert_eq!(
            merged_aggregate_data_fields(
                Some(&schema),
                &["data.observed_only".to_string(), "data.shared".to_string()],
            ),
            vec![
                "data.observed_only".to_string(),
                "data.schema_only".to_string(),
                "data.shared".to_string(),
            ]
        );
    }

    #[test]
    fn timed_completion_expires_at_the_ttl() {
        let cached_at = Instant::now();
        let cached = TimedCompletion::new_at(vec!["value"], cached_at);
        let ttl = Duration::from_secs(60);

        assert_eq!(
            cached.fresh_value(ttl, cached_at + Duration::from_secs(59)),
            Some(vec!["value"])
        );
        assert_eq!(
            cached.fresh_value(ttl, cached_at + Duration::from_secs(60)),
            None
        );
    }

    #[test]
    fn command_invalidation_preserves_timed_observed_fields() {
        let store = CompletionStore::default();
        {
            let mut snapshot = store.snapshot.write().expect("completion snapshot");
            snapshot
                .simple_sources
                .insert(CompletionKind::Classes, vec!["Hosts".to_string()]);
            snapshot.observed_fields_by_class.insert(
                "Hosts".to_string(),
                TimedCompletion::new(ObservedObjectDataFields::default()),
            );
        }

        store.invalidate_volatile();

        let snapshot = store.snapshot.read().expect("completion snapshot");
        assert!(snapshot.simple_sources.is_empty());
        assert!(snapshot.observed_fields_by_class.contains_key("Hosts"));
    }
}
