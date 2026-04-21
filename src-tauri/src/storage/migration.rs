use std::path::Path;

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

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
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
}
