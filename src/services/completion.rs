use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::runtime::Handle;

use crate::config::get_config;
use crate::errors::AppError;
use crate::services::ListTasksInput;

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
    groups: Option<Vec<String>>,
    classes: Option<Vec<String>>,
    namespaces: Option<Vec<String>>,
    event_sinks: Option<Vec<String>>,
    report_templates: Option<Vec<String>>,
    objects_by_class: HashMap<String, Vec<String>>,
    event_subscriptions_by_namespace: HashMap<String, Vec<String>>,
    class_schemas: HashMap<String, Option<serde_json::Value>>,
    task_ids: Option<Vec<CompletionItem>>,
}

#[derive(Clone, Default)]
pub(crate) struct CompletionStore {
    snapshot: Arc<RwLock<CompletionSnapshot>>,
}

#[derive(Clone, Copy)]
enum CompletionKind {
    Groups,
    Classes,
    Namespaces,
    EventSinks,
    ReportTemplates,
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

    pub fn namespaces(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::Namespaces)
    }

    pub fn event_sinks(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::EventSinks)
    }

    pub fn report_templates(&self, prefix: &str) -> Vec<String> {
        self.complete(prefix, CompletionKind::ReportTemplates)
    }

    pub fn objects_from_class(&self, prefix: &str, parts: &[String], source: &str) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(class_name) = parts
            .windows(2)
            .find(|pair| pair[0] == source)
            .map(|pair| pair[1].clone())
        else {
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

    pub fn event_subscriptions_from_namespace(
        &self,
        prefix: &str,
        parts: &[String],
    ) -> Vec<String> {
        if get_config().completion.disable_api_related {
            return Vec::new();
        }

        let Some(namespace) = parts
            .windows(2)
            .find(|pair| pair[0] == "--namespace")
            .map(|pair| pair[1].clone())
        else {
            return Vec::new();
        };

        self.runtime
            .block_on(
                self.services
                    .completion_store()
                    .load_event_subscriptions_for_namespace(self.services.gateway(), namespace),
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

    pub fn class_schema(&self, class_name: &str) -> Option<Option<serde_json::Value>> {
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
        gateway: Arc<super::gateway::HubuumGateway>,
        kind: CompletionKind,
    ) -> Result<Vec<String>, AppError> {
        if let Some(cached) = self.cached(kind) {
            return Ok(cached);
        }

        let fetched = tokio::task::spawn_blocking(move || -> Result<Vec<String>, AppError> {
            match kind {
                CompletionKind::Groups => gateway.list_group_names(),
                CompletionKind::Classes => gateway.list_class_names(),
                CompletionKind::Namespaces => gateway.list_namespace_names(),
                CompletionKind::EventSinks => gateway.list_event_sink_names(),
                CompletionKind::ReportTemplates => gateway.list_report_template_names(),
            }
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            match kind {
                CompletionKind::Groups => snapshot.groups = Some(fetched.clone()),
                CompletionKind::Classes => snapshot.classes = Some(fetched.clone()),
                CompletionKind::Namespaces => snapshot.namespaces = Some(fetched.clone()),
                CompletionKind::EventSinks => snapshot.event_sinks = Some(fetched.clone()),
                CompletionKind::ReportTemplates => {
                    snapshot.report_templates = Some(fetched.clone())
                }
            }
        }

        Ok(fetched)
    }

    async fn load_objects_for_class(
        &self,
        gateway: Arc<super::gateway::HubuumGateway>,
        class_name: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.objects_by_class.get(&class_name) {
                return Ok(cached.clone());
            }
        }

        let cache_key = class_name.clone();
        let fetched =
            tokio::task::spawn_blocking(move || gateway.list_object_names_for_class(&class_name))
                .await
                .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.objects_by_class.insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn query_objects_for_class_prefix(
        &self,
        gateway: Arc<super::gateway::HubuumGateway>,
        class_name: String,
        prefix: String,
    ) -> Result<Vec<String>, AppError> {
        tokio::task::spawn_blocking(move || {
            gateway.list_object_names_for_class_prefix(&class_name, &prefix)
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))?
    }

    async fn load_event_subscriptions_for_namespace(
        &self,
        gateway: Arc<super::gateway::HubuumGateway>,
        namespace: String,
    ) -> Result<Vec<String>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.event_subscriptions_by_namespace.get(&namespace) {
                return Ok(cached.clone());
            }
        }

        let cache_key = namespace.clone();
        let fetched = tokio::task::spawn_blocking(move || {
            gateway.list_event_subscription_names_for_namespace(&namespace)
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot
                .event_subscriptions_by_namespace
                .insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_class_schema(
        &self,
        gateway: Arc<super::gateway::HubuumGateway>,
        class_name: String,
    ) -> Result<Option<serde_json::Value>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = snapshot.class_schemas.get(&class_name) {
                return Ok(cached.clone());
            }
        }

        let cache_key = class_name.clone();
        let fetched = tokio::task::spawn_blocking(move || gateway.class_schema(&class_name))
            .await
            .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.class_schemas.insert(cache_key, fetched.clone());
        }

        Ok(fetched)
    }

    async fn load_task_id_items(
        &self,
        gateway: Arc<super::gateway::HubuumGateway>,
    ) -> Result<Vec<CompletionItem>, AppError> {
        if let Ok(snapshot) = self.snapshot.read() {
            if let Some(cached) = &snapshot.task_ids {
                return Ok(cached.clone());
            }
        }

        let fetched = tokio::task::spawn_blocking(move || {
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

    fn cached(&self, kind: CompletionKind) -> Option<Vec<String>> {
        let Ok(snapshot) = self.snapshot.read() else {
            return None;
        };

        match kind {
            CompletionKind::Groups => snapshot.groups.clone(),
            CompletionKind::Classes => snapshot.classes.clone(),
            CompletionKind::Namespaces => snapshot.namespaces.clone(),
            CompletionKind::EventSinks => snapshot.event_sinks.clone(),
            CompletionKind::ReportTemplates => snapshot.report_templates.clone(),
        }
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

fn task_description(task: &crate::domain::TaskRecord) -> String {
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
