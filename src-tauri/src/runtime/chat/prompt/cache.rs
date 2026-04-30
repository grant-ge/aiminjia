use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::PromptSectionId;

#[derive(Debug, Clone, Default)]
pub struct PromptSectionCache {
    entries: Arc<Mutex<HashMap<PromptSectionId, String>>>,
}

impl PromptSectionCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert(
        &self,
        section_id: PromptSectionId,
        compute: impl FnOnce() -> String,
    ) -> String {
        if let Some(cached) = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&section_id)
            .cloned()
        {
            return cached;
        }

        // Section builders may be slow or re-enter the cache, so compute outside the mutex.
        let computed = compute();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries
            .entry(section_id)
            .or_insert_with(|| computed.clone())
            .clone()
    }

    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}
