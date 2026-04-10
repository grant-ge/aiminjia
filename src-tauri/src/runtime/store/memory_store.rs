use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

#[derive(Clone, Debug)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
}

pub trait MemoryStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
}

#[derive(Default)]
pub struct InMemoryMemoryStore {
    values: Mutex<HashMap<String, String>>,
}

impl MemoryStore for InMemoryMemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
}
