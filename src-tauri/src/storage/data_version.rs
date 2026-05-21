//! Data layout compatibility marker.
//!
//! Bumped manually whenever a breaking change ships that older on-disk
//! data cannot survive — e.g. encryption key path / KDF change, `CloudAuth`
//! struct field rename, or storage path move that has no read-side migration.
//!
//! On startup, if the on-disk version is below [`REQUIRED_DATA_VERSION`] AND
//! the current `cloud_auth` blob cannot be decrypted with the current key,
//! the auth state is purged so the user is forced to re-login.  User content
//! (chat history, employees, files, settings) is NOT touched.
//!
//! Why this exists: between 0.3.x and 0.5.25 the master-key path moved
//! (commit 389f904), `cloud_auth` storage location moved (98d665b), and the
//! restore-on-fail behaviour changed from "clear and re-login" to "preserve
//! and retry forever" (ffd9d96).  Combined, an upgrading user could land in
//! an unrecoverable state — the legacy encrypted blob couldn't be decrypted
//! with the regenerated master key, restore() refused to clear it, and
//! `bootstrap_cloud_auth_if_needed` kept resurrecting the legacy ciphertext
//! from `~/.renlijia/config.json` after every logout.  Only `rm -rf
//! ~/.renlijia` plus reinstall would recover.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::storage::crypto::SecureStorage;
use crate::storage::AiJiaHome;

/// Bump when a breaking storage / encryption change ships.
///
/// History:
///   1 — baseline (0.5.26).  Purges 0.3.x / 0.4.x `cloud_auth` blobs that
///       cannot be decrypted under the current `SecureStorage`.
pub const REQUIRED_DATA_VERSION: u32 = 1;

const VERSION_FILE: &str = "data_version";

fn version_path(home: &AiJiaHome) -> PathBuf {
    home.root().join(VERSION_FILE)
}

fn read_on_disk(home: &AiJiaHome) -> u32 {
    fs::read_to_string(version_path(home))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn write_on_disk(home: &AiJiaHome, version: u32) -> std::io::Result<()> {
    let path = version_path(home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, version.to_string())
}

/// Ensure the on-disk data layout is compatible with this binary.
///
/// Behaviour matrix when on-disk version < required:
///   - cloud_auth file missing / empty → no-op purge, bump version (brand-new install).
///   - cloud_auth decrypts cleanly → bump version without purging (working same-major user).
///   - cloud_auth fails to decrypt → purge all auth state + bump version.
///
/// Returns true iff a purge happened.
pub fn ensure_compatible(home: &AiJiaHome, secure_storage: Option<&SecureStorage>) -> bool {
    let on_disk = read_on_disk(home);
    if on_disk >= REQUIRED_DATA_VERSION {
        return false;
    }

    let needs_purge = !cloud_auth_decryptable(home, secure_storage);

    if needs_purge {
        log::warn!(
            "[data_version] on-disk v{} < required v{} AND cloud_auth blob unreadable — purging auth state to force clean re-login",
            on_disk,
            REQUIRED_DATA_VERSION
        );
        if let Err(e) = purge_auth_state(home) {
            // Don't propagate — failing to purge is no worse than the
            // pre-fix state.  Still bump the version so we don't loop on
            // every launch.
            log::warn!("[data_version] purge_auth_state error (continuing): {}", e);
        }
    } else {
        log::info!(
            "[data_version] on-disk v{} → v{} (no purge needed)",
            on_disk,
            REQUIRED_DATA_VERSION
        );
    }

    if let Err(e) = write_on_disk(home, REQUIRED_DATA_VERSION) {
        log::error!("[data_version] failed to write version marker: {}", e);
    }

    needs_purge
}

/// True iff the current `cloud_auth` blob is either absent or successfully
/// decryptable with the active `SecureStorage`.  Does NOT validate the JSON
/// shape — that's `load_persisted_auth`'s job, and now (post ffd9d96 revert)
/// `restore()` clears the file on parse failure.
fn cloud_auth_decryptable(home: &AiJiaHome, secure_storage: Option<&SecureStorage>) -> bool {
    let path = home.cloud_auth_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return true; // brand-new install, nothing to protect
    };
    if raw.trim().is_empty() {
        return true;
    }
    match secure_storage {
        Some(ss) => ss.decrypt(&raw).is_ok(),
        None => true, // no encryption configured → raw is plaintext, can't "fail"
    }
}

/// Delete every persistence site that could resurrect a stale `cloud_auth`
/// blob.  Critically includes the legacy `~/.renlijia/config.json` key that
/// `bootstrap_cloud_auth_if_needed` reads from — without removing it there,
/// the next launch repopulates the new location with the same broken
/// ciphertext.
pub fn purge_auth_state(home: &AiJiaHome) -> std::io::Result<()> {
    let mut purged: Vec<String> = Vec::new();

    let cloud_auth = home.cloud_auth_path();
    if cloud_auth.exists() {
        fs::remove_file(&cloud_auth).ok();
        purged.push(cloud_auth.display().to_string());
    }

    let active_account = home.active_account_path();
    if active_account.exists() {
        fs::remove_file(&active_account).ok();
        purged.push(active_account.display().to_string());
    }

    if remove_cloud_auth_from_legacy_config(home.root())? {
        purged.push(format!("{}::cloud_auth", home.root().join("config.json").display()));
    }

    if !purged.is_empty() {
        log::warn!("[data_version] purged: {:?}", purged);
    }
    Ok(())
}

/// Remove a residual `"cloud_auth"` key from the legacy `~/.renlijia/config.json`
/// (the bootstrap source).  Returns `true` if the key was present and removed.
fn remove_cloud_auth_from_legacy_config(root: &Path) -> std::io::Result<bool> {
    let legacy = root.join("config.json");
    if !legacy.exists() {
        return Ok(false);
    }
    let text = match fs::read_to_string(&legacy) {
        Ok(t) => t,
        Err(_) => return Ok(false),
    };
    let mut map: HashMap<String, String> = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(_) => return Ok(false), // malformed config — leave it alone
    };
    if map.remove("cloud_auth").is_none() {
        return Ok(false);
    }
    let out = serde_json::to_string_pretty(&map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = legacy.with_extension("json.tmp");
    fs::write(&tmp, out)?;
    fs::rename(&tmp, &legacy)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_legacy_config_with_cloud_auth(root: &Path, blob: &str) {
        let mut m = HashMap::new();
        m.insert("cloud_auth".to_string(), blob.to_string());
        m.insert("theme".to_string(), "dark".to_string());
        let text = serde_json::to_string_pretty(&m).unwrap();
        fs::write(root.join("config.json"), text).unwrap();
    }

    #[test]
    fn brand_new_install_bumps_version_without_purge() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        home.ensure_global_dirs().unwrap();

        let ss = SecureStorage::new(&home.crypto_dir()).unwrap();
        let purged = ensure_compatible(&home, Some(&ss));

        assert!(!purged, "brand-new install should not log a purge");
        assert_eq!(read_on_disk(&home), REQUIRED_DATA_VERSION);
    }

    #[test]
    fn working_user_with_decryptable_blob_is_not_purged() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        home.ensure_global_dirs().unwrap();

        let ss = SecureStorage::new(&home.crypto_dir()).unwrap();
        let blob = ss.encrypt(r#"{"any":"json"}"#).unwrap();
        fs::write(home.cloud_auth_path(), &blob).unwrap();

        let purged = ensure_compatible(&home, Some(&ss));
        assert!(!purged, "decryptable blob should be preserved");
        assert!(home.cloud_auth_path().exists(), "cloud_auth must survive");
        assert_eq!(read_on_disk(&home), REQUIRED_DATA_VERSION);
    }

    #[test]
    fn unreadable_blob_triggers_purge_and_version_bump() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        home.ensure_global_dirs().unwrap();

        // Stale blob from a previous SecureStorage instance with a different key.
        let stale_ss = SecureStorage::new(&TempDir::new().unwrap().path().to_path_buf()).unwrap();
        let stale_blob = stale_ss.encrypt(r#"{"old":"data"}"#).unwrap();
        fs::write(home.cloud_auth_path(), &stale_blob).unwrap();
        write_legacy_config_with_cloud_auth(home.root(), &stale_blob);

        // New SecureStorage with a different master key (simulates upgrade).
        let new_ss = SecureStorage::new(&home.crypto_dir()).unwrap();

        let purged = ensure_compatible(&home, Some(&new_ss));
        assert!(purged, "stale blob should be purged");
        assert!(!home.cloud_auth_path().exists(), "cloud_auth file must be gone");
        assert_eq!(read_on_disk(&home), REQUIRED_DATA_VERSION);

        // Critical: legacy source emptied so bootstrap won't resurrect it.
        let legacy_text = fs::read_to_string(home.root().join("config.json")).unwrap();
        assert!(!legacy_text.contains("cloud_auth"), "legacy cloud_auth key must be removed");
        // Other legacy keys preserved.
        assert!(legacy_text.contains("theme"));
    }

    #[test]
    fn second_run_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        home.ensure_global_dirs().unwrap();
        let ss = SecureStorage::new(&home.crypto_dir()).unwrap();

        assert!(!ensure_compatible(&home, Some(&ss)));
        // Subsequent calls short-circuit (no log noise, no work).
        assert!(!ensure_compatible(&home, Some(&ss)));
        assert!(!ensure_compatible(&home, Some(&ss)));
    }

    #[test]
    fn purge_is_idempotent_when_files_missing() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        home.ensure_global_dirs().unwrap();
        // No files exist — purge must not error.
        purge_auth_state(&home).unwrap();
    }

    #[test]
    fn legacy_config_without_cloud_auth_is_unchanged() {
        let tmp = TempDir::new().unwrap();
        let mut map = HashMap::new();
        map.insert("theme".to_string(), "dark".to_string());
        fs::write(
            tmp.path().join("config.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();

        let removed = remove_cloud_auth_from_legacy_config(tmp.path()).unwrap();
        assert!(!removed);
        // File still parseable, theme intact.
        let text = fs::read_to_string(tmp.path().join("config.json")).unwrap();
        let parsed: HashMap<String, String> = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.get("theme").map(String::as_str), Some("dark"));
    }
}
