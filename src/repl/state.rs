use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct ReplState {
    scope: Vec<String>,
    next_page_command: Option<String>,
}

#[derive(Clone)]
pub struct SharedReplState {
    inner: Arc<Mutex<ReplState>>,
}

impl SharedReplState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ReplState::default())),
        }
    }

    pub fn scope(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .scope
            .clone()
    }

    pub fn set_scope(&self, scope: Vec<String>) {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .scope = scope;
    }

    pub fn exit_scope(&self) -> bool {
        let mut guard = self
            .inner
            .lock()
            .expect("session scope lock should not be poisoned");
        guard.scope.pop().is_some()
    }

    pub fn next_page_command(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .next_page_command
            .clone()
    }

    pub fn set_next_page_command(&self, command: Option<String>) {
        self.inner
            .lock()
            .expect("session scope lock should not be poisoned")
            .next_page_command = command;
    }
}
