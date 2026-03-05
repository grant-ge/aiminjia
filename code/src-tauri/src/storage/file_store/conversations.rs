//! Conversation CRUD + global index management.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use log::{info, warn};

use super::error::StorageResult;
use super::io::{atomic_write_json, read_json_optional, read_json_safe};
use super::types::{ConversationIndexEntry, ConversationMeta, GlobalIndex};

/// Get the directory for a conversation.
pub fn conv_dir(base_dir: &Path, conversation_id: &str) -> PathBuf {
    base_dir.join("conversations").join(conversation_id)
}

/// Get the path to a conversation's metadata file.
fn conv_meta_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("conv.json")
}

/// Get the path to the global index.
fn index_path(base_dir: &Path) -> PathBuf {
    base_dir.join("index.json")
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Create a new conversation.
///
/// 1. Creates the conversation directory + subdirs (uploads/, generated/, notes/)
/// 2. Writes `conv.json`
/// 3. Adds an entry to `index.json`
pub fn create_conversation(
    base_dir: &Path,
    id: &str,
    title: &str,
) -> StorageResult<()> {
    let dir = conv_dir(base_dir, id);
    let now = Utc::now().to_rfc3339();

    // Create directory structure
    fs::create_dir_all(dir.join("uploads"))?;
    fs::create_dir_all(dir.join("generated"))?;
    fs::create_dir_all(dir.join("notes"))?;

    // Write conv.json
    let meta = ConversationMeta {
        id: id.to_string(),
        title: title.to_string(),
        mode: "daily".to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
        is_archived: false,
    };
    atomic_write_json(&conv_meta_path(base_dir, id), &meta)?;

    // Update global index
    let mut index = read_global_index(base_dir)?;
    index.conversations.push(ConversationIndexEntry {
        id: id.to_string(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
        is_archived: false,
    });
    atomic_write_json(&index_path(base_dir), &index)?;

    info!("Created conversation: {}", id);
    Ok(())
}

/// Update a conversation's title.
pub fn update_conversation_title(
    base_dir: &Path,
    id: &str,
    title: &str,
) -> StorageResult<()> {
    let meta_path = conv_meta_path(base_dir, id);
    let mut meta: ConversationMeta = read_json_safe(&meta_path)?;
    let now = Utc::now().to_rfc3339();
    meta.title = title.to_string();
    meta.updated_at = now.clone();
    atomic_write_json(&meta_path, &meta)?;

    // Update index entry
    let mut index = read_global_index(base_dir)?;
    if let Some(entry) = index.conversations.iter_mut().find(|e| e.id == id) {
        entry.title = title.to_string();
        entry.updated_at = now;
    }
    atomic_write_json(&index_path(base_dir), &index)?;

    Ok(())
}

/// Get the current mode of a conversation.
pub fn get_conversation_mode(base_dir: &Path, id: &str) -> StorageResult<String> {
    let meta_path = conv_meta_path(base_dir, id);
    let meta: ConversationMeta = read_json_safe(&meta_path)?;
    Ok(meta.mode)
}

/// Set the mode of a conversation.
pub fn set_conversation_mode(
    base_dir: &Path,
    id: &str,
    mode: &str,
) -> StorageResult<()> {
    let meta_path = conv_meta_path(base_dir, id);
    let mut meta: ConversationMeta = read_json_safe(&meta_path)?;
    let now = Utc::now().to_rfc3339();
    meta.mode = mode.to_string();
    meta.updated_at = now;
    atomic_write_json(&meta_path, &meta)?;
    Ok(())
}

/// Retrieve all non-archived conversations, most recent first.
///
/// Returns `serde_json::Value` for backward compatibility with the existing
/// commands layer.
pub fn get_conversations(base_dir: &Path) -> StorageResult<Vec<serde_json::Value>> {
    let index = read_global_index(base_dir)?;
    let mut result: Vec<serde_json::Value> = index
        .conversations
        .iter()
        .filter(|e| !e.is_archived)
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "title": e.title,
                "createdAt": e.created_at,
                "updatedAt": e.updated_at,
                "isArchived": e.is_archived,
                "mode": get_conversation_mode_safe(base_dir, &e.id),
            })
        })
        .collect();

    // Sort by updatedAt descending
    result.sort_by(|a, b| {
        let a_time = a["updatedAt"].as_str().unwrap_or("");
        let b_time = b["updatedAt"].as_str().unwrap_or("");
        b_time.cmp(a_time)
    });

    Ok(result)
}

/// Delete a conversation and all associated data.
pub fn delete_conversation(base_dir: &Path, id: &str) -> StorageResult<()> {
    // Remove from global index
    let mut index = read_global_index(base_dir)?;
    index.conversations.retain(|e| e.id != id);
    atomic_write_json(&index_path(base_dir), &index)?;

    // Remove directory (best-effort)
    let dir = conv_dir(base_dir, id);
    if dir.exists() {
        if let Err(e) = fs::remove_dir_all(&dir) {
            warn!("Failed to remove conversation directory {:?}: {}", dir, e);
        }
    }

    info!("Deleted conversation: {}", id);
    Ok(())
}

/// Get all physical file paths associated with a conversation.
///
/// Used to clean up disk files before deleting the conversation.
pub fn get_file_paths_for_conversation(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<String>> {
    let file_index_path = conv_dir(base_dir, conversation_id).join("file_index.json");
    let file_index: super::types::FileIndex =
        read_json_optional(&file_index_path)?.unwrap_or_default();

    Ok(file_index
        .files
        .iter()
        .map(|f| f.stored_path.clone())
        .collect())
}

// ─── Index reconciliation (startup) ─────────────────────────────────────────

/// Reconcile the global index with the actual conversation directories.
///
/// 1. Directories that exist but are missing from index → add them
/// 2. Index entries whose directories don't exist → remove them
pub fn reconcile_index(base_dir: &Path) -> StorageResult<()> {
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(());
    }

    let mut index = read_global_index(base_dir)?;
    let indexed_ids: std::collections::HashSet<String> =
        index.conversations.iter().map(|e| e.id.clone()).collect();

    // Scan actual directories
    let mut dir_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(entries) = fs::read_dir(&conversations_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                dir_ids.insert(name);
            }
        }
    }

    let mut changed = false;

    // Add missing directories to index
    for dir_id in &dir_ids {
        if !indexed_ids.contains(dir_id) {
            let meta_path = conv_meta_path(base_dir, dir_id);
            if let Ok(meta) = read_json_safe::<ConversationMeta>(&meta_path) {
                index.conversations.push(ConversationIndexEntry {
                    id: meta.id,
                    title: meta.title,
                    created_at: meta.created_at,
                    updated_at: meta.updated_at,
                    is_archived: meta.is_archived,
                });
                info!("Reconciled: added missing index entry for {}", dir_id);
                changed = true;
            } else {
                warn!("Reconciled: directory {} has no valid conv.json, skipping", dir_id);
            }
        }
    }

    // Remove orphan index entries
    let before = index.conversations.len();
    index.conversations.retain(|e| dir_ids.contains(&e.id));
    if index.conversations.len() < before {
        info!(
            "Reconciled: removed {} orphan index entries",
            before - index.conversations.len()
        );
        changed = true;
    }

    if changed {
        atomic_write_json(&index_path(base_dir), &index)?;
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Read the global index, returning empty if it doesn't exist.
fn read_global_index(base_dir: &Path) -> StorageResult<GlobalIndex> {
    Ok(read_json_optional(&index_path(base_dir))?.unwrap_or_default())
}

/// Get conversation mode, defaulting to "daily" on error (for index reads).
fn get_conversation_mode_safe(base_dir: &Path, id: &str) -> String {
    get_conversation_mode(base_dir, id).unwrap_or_else(|_| "daily".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        fs::create_dir_all(base.join("conversations")).unwrap();
        (base, dir)
    }

    #[test]
    fn test_create_and_get_conversations() {
        let (base, _dir) = setup();

        create_conversation(&base, "c1", "Test Conv").unwrap();
        let convs = get_conversations(&base).unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0]["title"], "Test Conv");
        assert_eq!(convs[0]["mode"], "daily");
    }

    #[test]
    fn test_delete_conversation() {
        let (base, _dir) = setup();

        create_conversation(&base, "c1", "Conv 1").unwrap();
        assert_eq!(get_conversations(&base).unwrap().len(), 1);

        delete_conversation(&base, "c1").unwrap();
        assert_eq!(get_conversations(&base).unwrap().len(), 0);
        assert!(!conv_dir(&base, "c1").exists());
    }

    #[test]
    fn test_update_title() {
        let (base, _dir) = setup();

        create_conversation(&base, "c1", "Original").unwrap();
        update_conversation_title(&base, "c1", "Updated").unwrap();

        let convs = get_conversations(&base).unwrap();
        assert_eq!(convs[0]["title"], "Updated");
    }

    #[test]
    fn test_conversation_mode() {
        let (base, _dir) = setup();

        create_conversation(&base, "c1", "Conv").unwrap();
        assert_eq!(get_conversation_mode(&base, "c1").unwrap(), "daily");

        set_conversation_mode(&base, "c1", "analyzing").unwrap();
        assert_eq!(get_conversation_mode(&base, "c1").unwrap(), "analyzing");
    }

    #[test]
    fn test_reconcile_adds_missing_entries() {
        let (base, _dir) = setup();

        // Create a conversation normally
        create_conversation(&base, "c1", "Normal").unwrap();

        // Manually create a directory with conv.json (simulating orphan)
        let orphan_dir = conv_dir(&base, "c2");
        fs::create_dir_all(&orphan_dir).unwrap();
        let meta = ConversationMeta {
            id: "c2".to_string(),
            title: "Orphan".to_string(),
            mode: "daily".to_string(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            is_archived: false,
        };
        atomic_write_json(&orphan_dir.join("conv.json"), &meta).unwrap();

        reconcile_index(&base).unwrap();

        let convs = get_conversations(&base).unwrap();
        assert_eq!(convs.len(), 2);
    }

    #[test]
    fn test_reconcile_removes_orphan_entries() {
        let (base, _dir) = setup();

        create_conversation(&base, "c1", "Conv").unwrap();
        // Remove the directory but not the index
        fs::remove_dir_all(conv_dir(&base, "c1")).unwrap();

        reconcile_index(&base).unwrap();

        let convs = get_conversations(&base).unwrap();
        assert_eq!(convs.len(), 0);
    }

    #[test]
    fn test_multiple_conversations_sorted() {
        let (base, _dir) = setup();

        create_conversation(&base, "c1", "First").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_conversation(&base, "c2", "Second").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_conversation(&base, "c3", "Third").unwrap();

        let convs = get_conversations(&base).unwrap();
        assert_eq!(convs.len(), 3);
        // Most recent first
        assert_eq!(convs[0]["title"], "Third");
        assert_eq!(convs[2]["title"], "First");
    }
}
