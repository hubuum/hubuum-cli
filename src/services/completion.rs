use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::runtime::Handle;

use crate::errors::AppError;

use super::AppServices;

#[derive(Clone)]
pub struct CompletionContext {
    services: Arc<AppServices>,
    runtime: Handle,
    disable_api_related: bool,
}

#[derive(Clone, Default)]
struct CompletionSnapshot {
    groups: Option<Vec<String>>,
    classes: Option<Vec<String>>,
    namespaces: Option<Vec<String>>,
    objects_by_class: HashMap<String, Vec<String>>,
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
}

impl CompletionContext {
    pub(crate) fn new(services: Arc<AppServices>, runtime: Handle, disable_api_related: bool) -> Self {
        Self {
            services,
            runtime,
            disable_api_related,
        }
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

    pub fn objects_from_class(&self, prefix: &str, parts: &[String], source: &str) -> Vec<String> {
        if self.disable_api_related {
            return Vec::new();
        }

        let Some(class_name) = parts.windows(2).find(|pair| pair[0] == source).map(|pair| pair[1].clone()) else {
            return Vec::new();
        };

        let fetched = self
            .runtime
            .block_on(self.services.completion_store().load_objects_for_class(
                self.services.gateway(),
                class_name,
            ))
            .unwrap_or_default();
        filter_prefix(&fetched, prefix)
    }

    fn complete(&self, prefix: &str, kind: CompletionKind) -> Vec<String> {
        if self.disable_api_related {
            return Vec::new();
        }

        let fetched = self
            .runtime
            .block_on(self.services.completion_store().load(
                self.services.gateway(),
                kind,
            ))
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
            }
        })
        .await
        .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            match kind {
                CompletionKind::Groups => snapshot.groups = Some(fetched.clone()),
                CompletionKind::Classes => snapshot.classes = Some(fetched.clone()),
                CompletionKind::Namespaces => snapshot.namespaces = Some(fetched.clone()),
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
        let fetched = tokio::task::spawn_blocking(move || gateway.list_object_names_for_class(&class_name))
            .await
            .map_err(|err| AppError::CommandExecutionError(err.to_string()))??;

        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.objects_by_class.insert(cache_key, fetched.clone());
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
