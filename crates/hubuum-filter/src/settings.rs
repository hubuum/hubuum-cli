use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::error::PipelineError;

/// How an explicitly selected path absent from every input record is handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MissingSelectorPolicy {
    #[default]
    Allow,
    Error,
}

/// Caller-owned policy for pipeline evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PipelineSettings {
    ignored_search_keys: BTreeSet<String>,
    missing_selectors: MissingSelectorPolicy,
    evaluation_time: Option<SystemTime>,
}

impl PipelineSettings {
    /// Creates settings that search every JSON key.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_missing_selector_policy(mut self, policy: MissingSelectorPolicy) -> Self {
        self.missing_selectors = policy;
        self
    }

    pub fn missing_selector_policy(&self) -> MissingSelectorPolicy {
        self.missing_selectors
    }

    /// Fix the clock for one application. Otherwise the clock is sampled once.
    pub fn with_evaluation_time(mut self, time: SystemTime) -> Self {
        self.evaluation_time = Some(time);
        self
    }

    pub(crate) fn evaluation_time(&self) -> SystemTime {
        self.evaluation_time.unwrap_or_else(SystemTime::now)
    }

    /// Excludes keys from recursive broad, key-only, and value-only searches.
    ///
    /// Values selected through an envelope's explicit visible columns remain
    /// searchable. Empty and whitespace-only keys are rejected.
    pub fn with_ignored_search_keys<I, S>(mut self, keys: I) -> Result<Self, PipelineError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for key in keys {
            let key = key.into();
            if key.trim().is_empty() {
                return Err(PipelineError::Pipe(
                    "Ignored search keys cannot be empty or whitespace-only".to_string(),
                ));
            }
            self.ignored_search_keys.insert(key);
        }
        Ok(self)
    }

    /// Returns ignored recursive-search keys in deterministic order.
    pub fn ignored_search_keys(&self) -> impl Iterator<Item = &str> {
        self.ignored_search_keys.iter().map(String::as_str)
    }

    pub(crate) fn ignores_search_key(&self, key: &str) -> bool {
        self.ignored_search_keys.contains(key)
    }
}
