//! Stable per-installation device identifier.
//!
//! Persists a random UUID-v4 to `~/.renlijia/device_id` on first call;
//! returned to the server when creating a session_key so the server can
//! dedupe active keys by `(user_id, device_id)` — preventing the same
//! desktop install from consuming a fresh slot every time the app opens.
//!
//! Cross-user, cross-tenant: the id describes the *machine + install*,
//! not the logged-in user. Stays at `aijia_home.root()` (not under
//! `users/{scope}/`) so logout / re-login keeps the same value.

use std::path::PathBuf;

use crate::storage::AiJiaHome;

const FILE_NAME: &str = "device_id";

fn device_id_path(home: &AiJiaHome) -> PathBuf {
    home.root().join(FILE_NAME)
}

/// Read the persisted device id; create one on first call. Never fails:
/// IO errors fall back to a freshly generated UUID that won't be persisted,
/// so worst case we just lose idempotency for one app session.
pub fn load_or_create(home: &AiJiaHome) -> String {
    let path = device_id_path(home);
    if let Ok(bytes) = std::fs::read(&path) {
        let id = String::from_utf8_lossy(&bytes).trim().to_string();
        // sanity check: a valid uuid is ≤64 chars, non-empty
        if !id.is_empty() && id.len() <= 64 {
            return id;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, id.as_bytes()) {
        log::warn!(
            "[device_id] failed to persist {}: {} — using ephemeral id this session",
            path.display(),
            e
        );
    }
    id
}
