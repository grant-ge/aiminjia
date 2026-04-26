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
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.entry(section_id).or_insert_with(compute).clone()
    }

    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}
