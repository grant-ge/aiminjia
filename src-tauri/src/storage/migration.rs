use std::path::Path;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde_json::{json, Value};

static GLOBAL_STATE_WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) fn with_state_json_write_lock<T>(
    operation: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let _guard = GLOBAL_STATE_WRITE_LOCK
        .lock()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?;
    operation()
}

/// 启动时一次性迁移：把旧 app_data_dir 的数据复制到 ~/.renlijia/。
/// 完成后写 .migrated 标记，下次启动直接跳过。
/// 已存在的文件不覆盖，保护用户数据。
pub fn migrate_if_needed(old_dir: &Path, new_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(new_dir)?;

    let marker = new_dir.join(".migrated");
    if marker.exists() {
        return Ok(());
    }

    if !old_dir.exists() {
        std::fs::write(&marker, "1")?;
        return Ok(());
    }

    log::info!("[migration] {:?} -> {:?}", old_dir, new_dir);

    let items: &[(&str, &str)] = &[
        ("config.json", "config.json"),
        ("index.json", "index.json"),
        ("conversations", "conversations"),
        ("shared", "shared"),
        ("project_memories", "project_memories"),
        ("audit", "audit"),
        ("permissions.json", "permissions.json"),
        ("mcp_servers.json", "mcp_servers.json"),
        ("agent_invocations.json", "agent_invocations.json"),
        ("subagent_transcripts", "subagent_transcripts"),
        ("custom_plugins", "skills"),
        ("playwright-profile", "playwright-profile"),
        ("api-data", "api-data"),
        ("screenshots", "screenshots"),
        ("site-profiles", "site-profiles"),
        ("master.key", "crypto/master.key"),
        (".lotus-key", "crypto/master.key"),
    ];

    for (old_rel, new_rel) in items {
        let src = old_dir.join(old_rel);
        let dst = new_dir.join(new_rel);
        if !src.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if src.is_dir() {
            // Destination directories may already exist because AiJiaHome::ensure_dirs
            // runs before migration. Merge missing files without overwriting.
            copy_dir(&src, &dst)?;
        } else if !dst.exists() {
            std::fs::copy(&src, &dst)?;
        } else {
            continue;
        }
        log::info!("[migration] {} -> {}", old_rel, new_rel);
    }

    std::fs::write(&marker, "1")?;
    log::info!("[migration] done");
    Ok(())
}

/// 一次性补迁移旧目录里新目录缺失的 conversations。
/// 成功后写入 ~/.renlijia/state.json 的 migrations.legacyConversations=true。
pub fn reconcile_legacy_conversations_if_needed(
    old_dir: &Path,
    new_dir: &Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(new_dir)?;
    let state_path = new_dir.join("state.json");
    let state = read_state_json(&state_path)?;
    if state
        .get("migrations")
        .and_then(|m| m.get("legacyConversations"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(());
    }

    let old_conversations = old_dir.join("conversations");
    let new_conversations = new_dir.join("conversations");
    std::fs::create_dir_all(&new_conversations)?;

    if old_conversations.exists() {
        for entry in std::fs::read_dir(&old_conversations)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let dst = new_conversations.join(entry.file_name());
            if dst.exists() {
                continue;
            }
            copy_dir(&entry.path(), &dst)?;
            log::info!("[migration] legacy conversation -> {:?}", dst);
        }
    }

    update_state_json(&state_path, |state| {
        state["migrations"]["legacyConversations"] = json!(true);
    })?;
    Ok(())
}

/// 一次性把旧 messages.N.jsonl 合并进 messages.jsonl，并通过 state.json 打标。
pub fn migrate_message_shards_to_single_file_if_needed(new_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(new_dir)?;
    let state_path = new_dir.join("state.json");
    let state = read_state_json(&state_path)?;
    if state
        .get("migrations")
        .and_then(|m| m.get("messageShardsToSingleFile"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(());
    }

    let conversations_dir = new_dir.join("conversations");
    if conversations_dir.exists() {
        for entry in std::fs::read_dir(&conversations_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let conv_id = entry.file_name().to_string_lossy().to_string();
            crate::storage::file_store::messages::migrate_shards_to_single_file(new_dir, &conv_id)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
        }
    }

    update_state_json(&state_path, |state| {
        state["migrations"]["messageShardsToSingleFile"] = json!(true);
    })?;
    Ok(())
}

pub(crate) fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_symlink() {
            continue;
        }
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub(crate) fn read_state_json(path: &Path) -> std::io::Result<Value> {
    if !path.exists() {
        return Ok(json!({ "migrations": {} }));
    }
    let text = std::fs::read_to_string(path)?;
    let mut value: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
    if !value.get("migrations").map_or(false, Value::is_object) {
        value["migrations"] = json!({});
    }
    Ok(value)
}

pub(crate) fn update_state_json(
    path: &Path,
    update: impl FnOnce(&mut Value),
) -> std::io::Result<()> {
    with_state_json_write_lock(|| {
        let mut state = read_state_json(path)?;
        update(&mut state);
        write_state_json_unlocked(path, &state)
    })
}

pub(crate) fn write_state_json_unlocked(path: &Path, state: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(state)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_copies_and_renames_custom_plugins() {
        let old = TempDir::new().unwrap();
        let new = TempDir::new().unwrap();

        std::fs::write(old.path().join("config.json"), r#"{"k":"v"}"#).unwrap();
        std::fs::create_dir_all(old.path().join("conversations/conv1")).unwrap();
        std::fs::write(old.path().join("conversations/conv1/conv.json"), "{}").unwrap();
        let skill = old.path().join("custom_plugins").join("my-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("plugin.toml"), "[plugin]").unwrap();
        std::fs::write(old.path().join("master.key"), "key-data").unwrap();

        migrate_if_needed(old.path(), new.path()).unwrap();

        assert!(new.path().join("config.json").exists());
        assert!(new.path().join("conversations/conv1/conv.json").exists());
        assert!(new.path().join("skills/my-skill/plugin.toml").exists());
        assert!(new.path().join("crypto/master.key").exists());
        assert!(new.path().join(".migrated").exists());
    }

    #[test]
    fn test_idempotent() {
        let old = TempDir::new().unwrap();
        let new = TempDir::new().unwrap();
        std::fs::write(old.path().join("config.json"), "old").unwrap();

        migrate_if_needed(old.path(), new.path()).unwrap();
        std::fs::write(new.path().join("config.json"), "new").unwrap();
        migrate_if_needed(old.path(), new.path()).unwrap();

        let content = std::fs::read_to_string(new.path().join("config.json")).unwrap();
        assert_eq!(content, "new");
    }

    #[test]
    fn test_merges_into_precreated_skills_dir_without_overwrite() {
        let old = TempDir::new().unwrap();
        let new = TempDir::new().unwrap();
        let old_skill = old.path().join("custom_plugins/my-skill");
        std::fs::create_dir_all(&old_skill).unwrap();
        std::fs::write(old_skill.join("plugin.toml"), "old").unwrap();

        let existing_skill = new.path().join("skills/existing-skill");
        std::fs::create_dir_all(&existing_skill).unwrap();
        std::fs::write(existing_skill.join("plugin.toml"), "new").unwrap();

        migrate_if_needed(old.path(), new.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(new.path().join("skills/my-skill/plugin.toml")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(new.path().join("skills/existing-skill/plugin.toml")).unwrap(),
            "new"
        );
    }

    #[test]
    fn test_missing_old_dir_writes_marker() {
        let parent = TempDir::new().unwrap();
        let old = parent.path().join("missing-old");
        let new = parent.path().join("new-home");

        migrate_if_needed(&old, &new).unwrap();

        assert!(new.join(".migrated").exists());
    }

    #[test]
    fn test_reconciles_missing_legacy_conversations_once_with_state_json() {
        let old = TempDir::new().unwrap();
        let new = TempDir::new().unwrap();
        let legacy_conv = old.path().join("conversations/legacy-1");
        std::fs::create_dir_all(&legacy_conv).unwrap();
        std::fs::write(
            legacy_conv.join("conv.json"),
            r#"{"id":"legacy-1","title":"Legacy","createdAt":"2026-04-01T00:00:00Z","updatedAt":"2026-04-01T00:00:00Z","isArchived":false}"#,
        )
        .unwrap();
        std::fs::write(legacy_conv.join("messages.1.jsonl"), "{}	✓\n").unwrap();

        reconcile_legacy_conversations_if_needed(old.path(), new.path()).unwrap();

        assert!(new.path().join("conversations/legacy-1/conv.json").exists());
        assert!(new
            .path()
            .join("conversations/legacy-1/messages.1.jsonl")
            .exists());
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(new.path().join("state.json")).unwrap())
                .unwrap();
        assert_eq!(state["migrations"]["legacyConversations"], true);

        std::fs::create_dir_all(new.path().join("conversations/existing")).unwrap();
        reconcile_legacy_conversations_if_needed(old.path(), new.path()).unwrap();
        assert!(!new.path().join("conversations/existing/conv.json").exists());
    }

    #[test]
    fn test_migrates_message_shards_to_single_file_once_with_state_json() {
        let new = TempDir::new().unwrap();
        let conv = new.path().join("conversations/c1");
        std::fs::create_dir_all(&conv).unwrap();
        std::fs::write(
            conv.join("conv.json"),
            r#"{"id":"c1","title":"C1","createdAt":"2026-04-01T00:00:00Z","updatedAt":"2026-04-01T00:00:00Z","isArchived":false}"#,
        )
        .unwrap();
        let m1 = serde_json::json!({
            "seq": 1,
            "_rev": 1,
            "id": "m1",
            "conversationId": "c1",
            "role": "user",
            "content": {"text": "hi"},
            "createdAt": "2026-04-01T00:00:00Z"
        });
        std::fs::write(
            conv.join("messages.1.jsonl"),
            format!("{}\t✓\n", serde_json::to_string(&m1).unwrap()),
        )
        .unwrap();
        std::fs::write(conv.join("_current"), "1:2").unwrap();

        migrate_message_shards_to_single_file_if_needed(new.path()).unwrap();

        assert!(conv.join("messages.jsonl").exists());
        assert!(!conv.join("messages.1.jsonl").exists());
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(new.path().join("state.json")).unwrap())
                .unwrap();
        assert_eq!(state["migrations"]["messageShardsToSingleFile"], true);

        let m2 = serde_json::json!({
            "seq": 2,
            "_rev": 1,
            "id": "m2",
            "conversationId": "c1",
            "role": "user",
            "content": {"text": "skip"},
            "createdAt": "2026-04-01T00:00:01Z"
        });
        std::fs::write(
            conv.join("messages.1.jsonl"),
            format!("{}\t✓\n", serde_json::to_string(&m2).unwrap()),
        )
        .unwrap();
        migrate_message_shards_to_single_file_if_needed(new.path()).unwrap();

        let jsonl = std::fs::read_to_string(conv.join("messages.jsonl")).unwrap();
        assert!(jsonl.contains("m1"));
        assert!(!jsonl.contains("m2"));
    }
}
