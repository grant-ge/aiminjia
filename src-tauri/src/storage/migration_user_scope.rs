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
    ("playwright-profile", "playwright-profile"),
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
    "dataMaskingLevel",
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
    "enableTaorTracking",
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
        fs::write(target, value)?;
    }
    Ok(())
}
