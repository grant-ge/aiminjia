//! Application settings stored in `config.json`.
//!
//! Uses a flat key-value map for compatibility with `AppSettings::from_string_map()`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::error::StorageResult;
use super::io::{atomic_write_json, read_json_optional};
use super::types::SettingsMap;

fn config_path(base_dir: &Path) -> PathBuf {
    base_dir.join("config.json")
}

/// Get a single setting value.
pub fn get_setting(base_dir: &Path, key: &str) -> StorageResult<Option<String>> {
    let map = read_settings_map(base_dir)?;
    Ok(map.0.get(key).cloned())
}

/// Upsert a setting.
pub fn set_setting(base_dir: &Path, key: &str, value: &str) -> StorageResult<()> {
    let mut map = read_settings_map(base_dir)?;
    map.0.insert(key.to_string(), value.to_string());
    atomic_write_json(&config_path(base_dir), &map)?;
    Ok(())
}

/// Get all settings as a HashMap.
pub fn get_all_settings(base_dir: &Path) -> StorageResult<HashMap<String, String>> {
    let map = read_settings_map(base_dir)?;
    Ok(map.0)
}

/// Get all settings whose key starts with the given prefix.
pub fn get_settings_by_prefix(
    base_dir: &Path,
    prefix: &str,
) -> StorageResult<HashMap<String, String>> {
    let map = read_settings_map(base_dir)?;
    let filtered: HashMap<String, String> = map
        .0
        .into_iter()
        .filter(|(k, _)| k.starts_with(prefix))
        .collect();
    Ok(filtered)
}

/// Delete a setting by key.
pub fn delete_setting(base_dir: &Path, key: &str) -> StorageResult<()> {
    let mut map = read_settings_map(base_dir)?;
    map.0.remove(key);
    atomic_write_json(&config_path(base_dir), &map)?;
    Ok(())
}

// ─── Internal ────────────────────────────────────────────────────────────────

fn read_settings_map(base_dir: &Path) -> StorageResult<SettingsMap> {
    Ok(read_json_optional(&config_path(base_dir))?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        (base, dir)
    }

    #[test]
    fn test_settings_crud() {
        let (base, _dir) = setup();

        assert_eq!(get_setting(&base, "theme").unwrap(), None);

        set_setting(&base, "theme", "dark").unwrap();
        assert_eq!(get_setting(&base, "theme").unwrap(), Some("dark".to_string()));

        set_setting(&base, "theme", "light").unwrap();
        assert_eq!(
            get_setting(&base, "theme").unwrap(),
            Some("light".to_string())
        );
    }

    #[test]
    fn test_get_all_settings() {
        let (base, _dir) = setup();

        set_setting(&base, "theme", "dark").unwrap();
        set_setting(&base, "lang", "en").unwrap();

        let all = get_all_settings(&base).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all["theme"], "dark");
        assert_eq!(all["lang"], "en");
    }

    #[test]
    fn test_settings_by_prefix() {
        let (base, _dir) = setup();

        set_setting(&base, "apiKey:deepseek", "key1").unwrap();
        set_setting(&base, "apiKey:openai", "key2").unwrap();
        set_setting(&base, "theme", "dark").unwrap();

        let api_keys = get_settings_by_prefix(&base, "apiKey:").unwrap();
        assert_eq!(api_keys.len(), 2);
        assert!(api_keys.contains_key("apiKey:deepseek"));
    }

    #[test]
    fn test_delete_setting() {
        let (base, _dir) = setup();

        set_setting(&base, "key1", "val").unwrap();
        delete_setting(&base, "key1").unwrap();
        assert_eq!(get_setting(&base, "key1").unwrap(), None);
    }
}
