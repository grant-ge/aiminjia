use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::json;

use super::migration::{copy_dir, read_state_json, update_state_json};

const LEGACY_ITEMS: &[(&str, &str)] = &[
    ("index.json", "index.json"),
    ("conversations", "conversations"),
    ("shared", "shared"),
    ("audit", "audit"),
    ("mcp_servers.json", "mcp_servers.json"),
    ("permissions.json", "permissions.json"),
    ("agent_invocations.json", "agent_invocations.json"),
    ("subagent_transcripts", "subagent_transcripts"),
    ("schedules", "schedules"),
    ("project_memories", "project_memories"),
    ("api-data", "api-data"),
    ("screenshots", "screenshots"),
    ("site-profiles", "site-profiles"),
];

const USER_CONFIG_KEYS: &[&str] = &[
    "workspacePath",
    "primaryModel",
    "primaryApiKey",
    "autoModelRouting",
    "cloudModel",
    "cloudModelType",
    "autoCleanupEnabled",
    "tempFileRetentionDays",
    "keepOldVersions",
    "tavilyApiKey",
    "bochaApiKey",
    "customModelEndpoint",
    "customModelName",
    "useCloud",
    "personaOnboardingDone",
    "thinkingType",
    "thinkingBudgetTokens",
    "analysisThreshold",
];

const AUTH_KEYS: &[&str] = &["cloud_auth"];

pub fn migrate_legacy_to_user_scope_if_needed(
    root: &Path,
    user_dir: &Path,
    scope_key: &str,
    global_state_path: &Path,
) -> std::io::Result<()> {
    let has_index = root.join("index.json").exists();
    let has_conversations = root.join("conversations").exists();
    if !has_index && !has_conversations {
        return Ok(());
    }

    let state = read_state_json(global_state_path)?;
    let claimed_by = state
        .get("migrations")
        .and_then(|m| m.get("legacyRootClaim"))
        .and_then(|c| c.get("claimedBy"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    match claimed_by.as_deref() {
        Some(existing) if existing == scope_key => return Ok(()),
        Some(existing) => {
            log::info!(
                "[migration:user-scope] legacy root already claimed by {}, skip auto migration for {}",
                existing,
                scope_key
            );
            return Ok(());
        }
        None => {}
    }

    fs::create_dir_all(user_dir)?;

    for (src_rel, dst_rel) in LEGACY_ITEMS {
        let src = root.join(src_rel);
        let dst = user_dir.join(dst_rel);
        if !src.exists() || dst.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }

    let legacy_skills = root.join("skills");
    let user_skills = user_dir.join("skills");
    if legacy_skills.exists() {
        fs::create_dir_all(&user_skills)?;
        for entry in fs::read_dir(&legacy_skills)?.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Copy user-installed skills: _drafts/ (user drafts) and non-underscore dirs (custom installs).
            // Skip _builtins and other system _-prefixed dirs (except _drafts).
            let should_copy = if name_str == "_drafts" {
                true
            } else if name_str.starts_with('_') {
                false // system dirs like _builtins
            } else {
                entry.path().is_dir()
            };
            if should_copy {
                let dst = user_skills.join(&name);
                if !dst.exists() {
                    copy_dir(&entry.path(), &dst)?;
                }
            }
        }
    }

    update_state_json(global_state_path, |state| {
        state["migrations"]["legacyRootClaim"] = json!({
            "claimedBy": scope_key,
            "claimedAt": chrono::Utc::now().to_rfc3339(),
        });
    })
}

pub fn migrate_legacy_config_if_needed(
    root: &Path,
    user_dir: &Path,
    global_dir: &Path,
) -> std::io::Result<()> {
    let legacy_config = root.join("config.json");
    let global_config = global_dir.join("config.json");
    let user_config = user_dir.join("config.json");

    // case A: user already has its own config — fully migrated/seeded, nothing to do
    if user_config.exists() {
        return Ok(());
    }

    // case B: legacy on disk + global not yet split — old-version-on-old-machine,
    // first login of new build. Split legacy into user_cfg + global_cfg.
    if legacy_config.exists() && !global_config.exists() {
        let text = fs::read_to_string(&legacy_config)?;
        let map: HashMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
        let mut user_map = HashMap::new();
        let mut global_map = HashMap::new();

        for (key, value) in &map {
            if AUTH_KEYS.contains(&key.as_str()) {
                let auth_dir = global_dir.join("auth");
                fs::create_dir_all(&auth_dir)?;
                let cloud_auth_path = auth_dir.join("cloud_auth");
                if !cloud_auth_path.exists() {
                    fs::write(cloud_auth_path, value)?;
                }
            } else if USER_CONFIG_KEYS.contains(&key.as_str()) || key.starts_with("apiKey:") {
                user_map.insert(key.clone(), value.clone());
            } else {
                global_map.insert(key.clone(), value.clone());
            }
        }

        if !user_map.is_empty() {
            fs::create_dir_all(user_dir)?;
            let text = serde_json::to_string_pretty(&user_map)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            fs::write(&user_config, text)?;
        }

        fs::create_dir_all(global_dir)?;
        let text = serde_json::to_string_pretty(&global_map)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(global_config, text)?;
        return Ok(());
    }

    // case C: brand-new user (no legacy / already-split / different account on this
    // machine). Seed an empty user config so downstream readers don't fall back
    // to AppSettings::default() — user-scoped storage must always have a backing
    // file, otherwise newly-saved settings can land in the wrong store.
    fs::create_dir_all(user_dir)?;
    fs::write(&user_config, "{}\n")?;
    Ok(())
}

/// One-shot migration from the legacy flat `<root>/turn_stages/` layout to
/// the user-scoped `<user_dir>/turn_stages/` (spec 2026-05-17-turn-stages §5,
/// user-isolation fix).
///
/// `turn_stage.json` files are ephemeral — they only live for the duration of
/// an in-flight LLM turn and are deleted by `CleanupGuard` on the terminal
/// exit.  The worst case from a partial / failed migration is that one
/// in-flight turn's UI can't hydrate after a webview reload; no message data
/// is at risk.  Therefore the policy is:
///
/// 1. For every `<root>/turn_stages/*.json`, attempt `rename` into
///    `<user_dir>/turn_stages/`.  Skip files that already exist at the target
///    (the destination wins — it's the more recent write for that conv).
/// 2. After all files are processed, remove the now-empty `<root>/turn_stages/`
///    directory.  If non-empty (e.g. a stale lock), leave it for a later run.
///
/// Idempotent and safe to call every startup.
pub fn migrate_legacy_turn_stages_if_needed(root: &Path, user_dir: &Path) -> std::io::Result<()> {
    let legacy_dir = root.join("turn_stages");
    if !legacy_dir.is_dir() {
        return Ok(());
    }
    let target_dir = user_dir.join("turn_stages");
    fs::create_dir_all(&target_dir)?;

    let mut moved = 0usize;
    let mut skipped = 0usize;
    for entry in fs::read_dir(&legacy_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let dst = target_dir.join(name);
        if dst.exists() {
            // Target is fresher (current user wrote it after migration last
            // ran).  Drop the legacy copy to free space.
            let _ = fs::remove_file(&path);
            skipped += 1;
            continue;
        }
        match fs::rename(&path, &dst) {
            Ok(_) => moved += 1,
            Err(_) => {
                // Cross-device EXDEV or other transient — fall back to copy+delete.
                if let Err(e) = fs::copy(&path, &dst) {
                    log::warn!(
                        "[migration:turn-stages] copy failed for {:?}: {e}; leaving in legacy dir",
                        path
                    );
                    continue;
                }
                let _ = fs::remove_file(&path);
                moved += 1;
            }
        }
    }

    // Try to remove the legacy dir.  Tolerated to fail (e.g. another process
    // is writing) — next startup will retry.
    let _ = fs::remove_dir(&legacy_dir);

    if moved > 0 || skipped > 0 {
        log::info!(
            "[migration:turn-stages] {} -> {} (moved={}, skipped={})",
            legacy_dir.display(),
            target_dir.display(),
            moved,
            skipped,
        );
    }
    Ok(())
}

pub fn bootstrap_cloud_auth_if_needed(root: &Path, global_dir: &Path) -> std::io::Result<()> {
    let target = global_dir.join("auth").join("cloud_auth");
    if target.exists() {
        return Ok(());
    }

    let legacy_config = root.join("config.json");
    if !legacy_config.exists() {
        return Ok(());
    }

    let text = fs::read_to_string(&legacy_config)?;
    let map: HashMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
    if let Some(value) = map.get("cloud_auth") {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Atomic write: tmp + rename. Without this, an interrupted write
        // (app force-killed during account-switch) leaves a truncated/empty
        // file that later confuses restore() into clearing it — handing
        // bootstrap the chance to revive the fossil on the next launch.
        let tmp = target.with_extension("tmp");
        fs::write(&tmp, value)?;
        fs::rename(&tmp, &target)?;
        // Now that the migration is done, drop the legacy key so a future
        // loss of the new file (logout-then-crash, corrupted partial write,
        // etc.) cannot revive this fossil token. The new persist path in
        // GlobalConfigStore never writes cloud_auth back into config.json,
        // so the legacy entry is frozen at the value it had on first
        // migration — typically the very first login the user ever made.
        // See SLS incident: zhaoyunxing (user_id=87) repeatedly snapped
        // back to a token from his initial login across many app launches.
        let _ = super::data_version::remove_cloud_auth_from_legacy_config(root);
    }
    Ok(())
}

#[cfg(test)]
mod turn_stages_migration_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn noop_when_legacy_dir_absent() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let user_dir = root.join("users/t_1__u_2");
        // No <root>/turn_stages/ at all — must be a clean noop, not an error.
        migrate_legacy_turn_stages_if_needed(root, &user_dir).unwrap();
        assert!(!user_dir.join("turn_stages").exists());
    }

    #[test]
    fn moves_files_and_removes_empty_legacy_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let legacy = root.join("turn_stages");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("conv-a.json"), b"a-payload").unwrap();
        fs::write(legacy.join("conv-b.json"), b"b-payload").unwrap();

        let user_dir = root.join("users/t_1__u_2");
        migrate_legacy_turn_stages_if_needed(root, &user_dir).unwrap();

        let target = user_dir.join("turn_stages");
        assert!(target.is_dir());
        assert_eq!(fs::read(target.join("conv-a.json")).unwrap(), b"a-payload");
        assert_eq!(fs::read(target.join("conv-b.json")).unwrap(), b"b-payload");
        // Legacy dir is gone now that it's empty.
        assert!(!legacy.exists());
    }

    #[test]
    fn target_wins_when_conflict_legacy_dropped() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let legacy = root.join("turn_stages");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("conv-a.json"), b"OLD").unwrap();

        let user_dir = root.join("users/t_1__u_2");
        let target = user_dir.join("turn_stages");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("conv-a.json"), b"NEW").unwrap();

        migrate_legacy_turn_stages_if_needed(root, &user_dir).unwrap();

        // Target is the current user's fresher write — keep it; drop the legacy.
        assert_eq!(fs::read(target.join("conv-a.json")).unwrap(), b"NEW");
        assert!(!legacy.join("conv-a.json").exists());
    }

    #[test]
    fn idempotent_second_call_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let legacy = root.join("turn_stages");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("conv-a.json"), b"a").unwrap();

        let user_dir = root.join("users/t_1__u_2");
        migrate_legacy_turn_stages_if_needed(root, &user_dir).unwrap();
        // Second call: legacy dir is gone, must still succeed cleanly.
        migrate_legacy_turn_stages_if_needed(root, &user_dir).unwrap();
        assert!(user_dir.join("turn_stages/conv-a.json").exists());
    }
}

#[cfg(test)]
mod bootstrap_cloud_auth_tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn write_legacy_with_cloud_auth(root: &Path, blob: &str) {
        let mut m = HashMap::new();
        m.insert("cloud_auth".to_string(), blob.to_string());
        m.insert("theme".to_string(), "dark".to_string());
        let text = serde_json::to_string_pretty(&m).unwrap();
        fs::write(root.join("config.json"), text).unwrap();
    }

    #[test]
    fn copies_legacy_and_purges_key_so_revival_is_impossible() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let global = root.join("global");
        fs::create_dir_all(&global).unwrap();
        write_legacy_with_cloud_auth(root, "FOSSIL_TOKEN");

        // First bootstrap: copies legacy → new and drops the legacy key.
        bootstrap_cloud_auth_if_needed(root, &global).unwrap();
        let target = global.join("auth").join("cloud_auth");
        assert_eq!(fs::read_to_string(&target).unwrap(), "FOSSIL_TOKEN");

        // Legacy key must be gone — otherwise a later loss of `target`
        // (logout-then-crash, partial write, etc.) would revive the fossil.
        let legacy_text = fs::read_to_string(root.join("config.json")).unwrap();
        let legacy_map: HashMap<String, String> = serde_json::from_str(&legacy_text).unwrap();
        assert!(!legacy_map.contains_key("cloud_auth"));
        // Unrelated keys are preserved.
        assert_eq!(legacy_map.get("theme").map(String::as_str), Some("dark"));
    }

    #[test]
    fn after_target_loss_second_bootstrap_does_not_revive_fossil() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let global = root.join("global");
        fs::create_dir_all(&global).unwrap();
        write_legacy_with_cloud_auth(root, "FOSSIL_TOKEN");

        // First bootstrap copies + purges legacy key.
        bootstrap_cloud_auth_if_needed(root, &global).unwrap();
        let target = global.join("auth").join("cloud_auth");

        // Simulate: user logged out (target removed), then app force-killed
        // before next login could persist new auth.
        fs::remove_file(&target).unwrap();

        // Next launch's bootstrap — legacy key is gone, so target stays
        // absent and `restore()` correctly sees "no cloud auth, log in".
        bootstrap_cloud_auth_if_needed(root, &global).unwrap();
        assert!(
            !target.exists(),
            "fossil cloud_auth revived after target loss — bug from SLS incident user_id=87"
        );
    }

    #[test]
    fn noop_when_target_already_exists() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let global = root.join("global");
        let auth_dir = global.join("auth");
        fs::create_dir_all(&auth_dir).unwrap();
        // Target already has fresh user data.
        fs::write(auth_dir.join("cloud_auth"), "FRESH_USER_DATA").unwrap();
        // Legacy still has a stale entry — must not overwrite target.
        write_legacy_with_cloud_auth(root, "STALE");

        bootstrap_cloud_auth_if_needed(root, &global).unwrap();
        assert_eq!(
            fs::read_to_string(auth_dir.join("cloud_auth")).unwrap(),
            "FRESH_USER_DATA"
        );
        // Legacy key untouched too — bootstrap short-circuited.
        let legacy_text = fs::read_to_string(root.join("config.json")).unwrap();
        let legacy_map: HashMap<String, String> = serde_json::from_str(&legacy_text).unwrap();
        assert!(legacy_map.contains_key("cloud_auth"));
    }

    #[test]
    fn noop_when_no_legacy_config() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let global = root.join("global");
        fs::create_dir_all(&global).unwrap();
        // No legacy config.json at all (brand-new install).
        bootstrap_cloud_auth_if_needed(root, &global).unwrap();
        assert!(!global.join("auth").join("cloud_auth").exists());
    }
}
