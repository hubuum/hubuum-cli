use std::collections::BTreeSet;

use crate::error::PipelineError;

/// Caller-owned policy for pipeline evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PipelineSettings {
    ignored_search_keys: BTreeSet<String>,
}

impl PipelineSettings {
    /// Creates settings that search every JSON key.
    pub fn new() -> Self {
        Self::default()
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
