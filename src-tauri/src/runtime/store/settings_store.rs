use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

pub trait SettingsStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Return all key-value pairs.
    fn get_all(&self) -> Result<HashMap<String, String>>;
    /// Return all entries whose key starts with `prefix`.
    fn get_by_prefix(&self, prefix: &str) -> Result<HashMap<String, String>>;
    /// Delete a key (no-op if absent).
    fn delete(&self, key: &str) -> Result<()>;
}

#[derive(Default)]
pub struct InMemorySettingsStore {
    values: Mutex<HashMap<String, String>>,
}

impl SettingsStore for InMemorySettingsStore {
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

    fn get_all(&self) -> Result<HashMap<String, String>> {
        Ok(self.values.lock().unwrap().clone())
    }

    fn get_by_prefix(&self, prefix: &str) -> Result<HashMap<String, String>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}
