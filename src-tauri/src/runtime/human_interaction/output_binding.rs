use std::collections::HashMap;
use std::sync::Mutex;

use crate::runtime::ids::{RunId, SessionId};

use super::types::OutputBinding;

#[derive(Default)]
pub struct RunOutputBindingRegistry {
    inner: Mutex<HashMap<(String, String), OutputBinding>>,
}

impl RunOutputBindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, session_id: &SessionId, run_id: &RunId, binding: OutputBinding) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                (session_id.as_str().into(), run_id.as_str().into()),
                binding,
            );
    }

    pub fn get(&self, session_id: &SessionId, run_id: &RunId) -> Option<OutputBinding> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(session_id.as_str().into(), run_id.as_str().into()))
            .cloned()
    }

    pub fn clear(&self, session_id: &SessionId, run_id: &RunId) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(session_id.as_str().into(), run_id.as_str().into()));
    }
}
