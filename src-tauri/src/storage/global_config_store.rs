use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::storage::text_io::read_to_string_strip_bom;

#[derive(Debug, Serialize, Deserialize, Default)]
struct SettingsMap(HashMap<String, String>);

pub struct GlobalConfigStore {
    global_dir: PathBuf,
    write_lock: Mutex<()>,
}

impl GlobalConfigStore {
    pub fn new(global_dir: PathBuf) -> Self {
        fs::create_dir_all(&global_dir).ok();
        fs::create_dir_all(global_dir.join("auth")).ok();
        Self {
            global_dir,
            write_lock: Mutex::new(()),
        }
    }

    fn config_path(&self) -> PathBuf {
        self.global_dir.join("config.json")
    }

    fn cloud_auth_path(&self) -> PathBuf {
        self.global_dir.join("auth").join("cloud_auth")
    }

    pub fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        if key == "cloud_auth" {
            let path = self.cloud_auth_path();
            if !path.exists() {
                return Ok(None);
            }
            return Ok(Some(read_to_string_strip_bom(path)?));
        }
        let map = self.read_map()?;
        Ok(map.0.get(key).cloned())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        if key == "cloud_auth" {
            let path = self.cloud_auth_path();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            // Atomic write: tmp + rename. fs::write truncates first, so a
            // crash mid-write left an empty file that restore() would treat
            // as "unreadable" and delete → next launch's bootstrap revived
            // the legacy fossil. tmp+rename gives all-or-nothing semantics:
            // the old file stays intact until rename succeeds.
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, value)?;
            fs::rename(&tmp, &path)?;
            return Ok(());
        }
        let mut map = self.read_map()?;
        map.0.insert(key.to_string(), value.to_string());
        self.write_map(&map)
    }

    pub fn delete_setting(&self, key: &str) -> anyhow::Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        if key == "cloud_auth" {
            let path = self.cloud_auth_path();
            if path.exists() {
                fs::remove_file(path)?;
            }
            return Ok(());
        }
        let mut map = self.read_map()?;
        map.0.remove(key);
        self.write_map(&map)
    }

    fn read_map(&self) -> anyhow::Result<SettingsMap> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(SettingsMap::default());
        }
        let text = read_to_string_strip_bom(&path)?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    fn write_map(&self, map: &SettingsMap) -> anyhow::Result<()> {
        let path = self.config_path();
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(map)?;
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn set_and_get_setting() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("key1", "value1").unwrap();
        assert_eq!(
            store.get_setting("key1").unwrap(),
            Some("value1".to_string())
        );
    }

    #[test]
    fn delete_setting() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("key1", "value1").unwrap();
        store.delete_setting("key1").unwrap();
        assert_eq!(store.get_setting("key1").unwrap(), None);
    }

    #[test]
    fn cloud_auth_stored_in_dedicated_file_not_config_json() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("cloud_auth", "encrypted_blob").unwrap();

        assert!(tmp.path().join("auth/cloud_auth").exists());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("auth/cloud_auth")).unwrap(),
            "encrypted_blob"
        );

        let config_text =
            std::fs::read_to_string(tmp.path().join("config.json")).unwrap_or_default();
        assert!(!config_text.contains("cloud_auth"));

        assert_eq!(
            store.get_setting("cloud_auth").unwrap(),
            Some("encrypted_blob".to_string())
        );
    }

    #[test]
    fn delete_cloud_auth_removes_file() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("cloud_auth", "blob").unwrap();
        store.delete_setting("cloud_auth").unwrap();
        assert!(!tmp.path().join("auth/cloud_auth").exists());
        assert_eq!(store.get_setting("cloud_auth").unwrap(), None);
    }

    #[test]
    fn settings_persist_across_instances() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().to_path_buf();

        // Instance 1 writes
        let store1 = GlobalConfigStore::new(path.clone());
        store1.set_setting("theme", "dark").unwrap();
        store1.set_setting("cloud_auth", "token123").unwrap();
        drop(store1);

        // Instance 2 reads (simulates app restart)
        let store2 = GlobalConfigStore::new(path);
        assert_eq!(
            store2.get_setting("theme").unwrap(),
            Some("dark".to_string())
        );
        assert_eq!(
            store2.get_setting("cloud_auth").unwrap(),
            Some("token123".to_string())
        );
    }
}
