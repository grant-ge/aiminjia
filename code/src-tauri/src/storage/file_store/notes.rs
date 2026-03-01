//! Notes (per-conversation) + Enterprise memory (shared, with auto-split).
//!
//! Per-conversation notes are stored as individual JSON files in `notes/`.
//! Enterprise memory is stored in `shared/memory/memory.jsonl` with auto-splitting
//! at 1 MB.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::error::StorageResult;
use super::io::{append_jsonl_with_split, read_all_jsonl_shards};
use super::types::MemoryEntry;

/// Memory file auto-split threshold: 1 MB.
const MEMORY_MAX_BYTES: u64 = 1_048_576;

fn memory_path(base_dir: &Path) -> PathBuf {
    base_dir.join("shared").join("memory").join("memory.jsonl")
}

// ─── Enterprise Memory ───────────────────────────────────────────────────────

/// Upsert a value in enterprise memory.
///
/// Uses last-writer-wins: appends a new entry. Reading resolves to the
/// last entry with a matching key.
pub fn set_memory(
    base_dir: &Path,
    key: &str,
    value: &str,
    source: Option<&str>,
) -> StorageResult<()> {
    let now = Utc::now().to_rfc3339();
    let entry = MemoryEntry {
        key: key.to_string(),
        value: value.to_string(),
        source: source.map(|s| s.to_string()),
        created_at: now.clone(),
        updated_at: now,
        deleted: false,
    };

    let path = memory_path(base_dir);
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    append_jsonl_with_split(&path, &entry, MEMORY_MAX_BYTES)?;
    Ok(())
}

/// Get a value from enterprise memory by key.
///
/// Returns the latest (last-written) non-deleted value for the key.
pub fn get_memory(base_dir: &Path, key: &str) -> StorageResult<Option<String>> {
    let entries = read_all_memory_entries(base_dir)?;

    // Find the last entry with this key that isn't deleted
    let result = entries
        .into_iter()
        .rev()
        .find(|e| e.key == key && !e.deleted)
        .map(|e| e.value);

    Ok(result)
}

/// Get all enterprise memory entries whose key starts with the given prefix.
///
/// Returns (key, value) pairs sorted by key.
pub fn get_memories_by_prefix(
    base_dir: &Path,
    prefix: &str,
) -> StorageResult<Vec<(String, String)>> {
    let entries = read_all_memory_entries(base_dir)?;

    // Build latest state: last-writer-wins per key
    let mut latest: HashMap<String, MemoryEntry> = HashMap::new();
    for entry in entries {
        if entry.key.starts_with(prefix) {
            latest.insert(entry.key.clone(), entry);
        }
    }

    // Filter out deleted, sort by key
    let mut results: Vec<(String, String)> = latest
        .into_values()
        .filter(|e| !e.deleted)
        .map(|e| (e.key, e.value))
        .collect();
    results.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(results)
}

// ─── Per-conversation Notes ──────────────────────────────────────────────────

/// Save a note for a conversation.
///
/// Notes are stored as individual JSON files in `conversations/{id}/notes/`.
pub fn save_note(
    base_dir: &Path,
    conversation_id: &str,
    name: &str,
    content: &serde_json::Value,
) -> StorageResult<()> {
    let notes_dir = base_dir
        .join("conversations")
        .join(conversation_id)
        .join("notes");
    std::fs::create_dir_all(&notes_dir)?;

    let note_path = notes_dir.join(format!("{}.json", sanitize_filename(name)));
    super::io::atomic_write_json(&note_path, content)?;
    Ok(())
}

/// Read a note for a conversation.
pub fn read_note(
    base_dir: &Path,
    conversation_id: &str,
    name: &str,
) -> StorageResult<Option<serde_json::Value>> {
    let note_path = base_dir
        .join("conversations")
        .join(conversation_id)
        .join("notes")
        .join(format!("{}.json", sanitize_filename(name)));

    Ok(super::io::read_json_optional(&note_path)?)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Read all memory entries from all shards.
fn read_all_memory_entries(base_dir: &Path) -> StorageResult<Vec<MemoryEntry>> {
    let path = memory_path(base_dir);
    if !path.exists() && !path.parent().map(|p| p.exists()).unwrap_or(false) {
        return Ok(Vec::new());
    }
    let entries = read_all_jsonl_shards::<MemoryEntry>(&path)?;
    Ok(entries)
}

/// Sanitize a filename to prevent path traversal.
fn sanitize_filename(name: &str) -> String {
    name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        std::fs::create_dir_all(base.join("shared").join("memory")).unwrap();
        std::fs::create_dir_all(base.join("conversations")).unwrap();
        (base, dir)
    }

    #[test]
    fn test_memory_set_and_get() {
        let (base, _dir) = setup();

        assert_eq!(get_memory(&base, "company_name").unwrap(), None);

        set_memory(&base, "company_name", "Acme Corp", Some("onboarding")).unwrap();
        assert_eq!(
            get_memory(&base, "company_name").unwrap(),
            Some("Acme Corp".to_string())
        );
    }

    #[test]
    fn test_memory_update() {
        let (base, _dir) = setup();

        set_memory(&base, "key1", "v1", None).unwrap();
        set_memory(&base, "key1", "v2", None).unwrap();

        assert_eq!(get_memory(&base, "key1").unwrap(), Some("v2".to_string()));
    }

    #[test]
    fn test_memory_prefix() {
        let (base, _dir) = setup();

        set_memory(&base, "note:c1:step1", "data1", None).unwrap();
        set_memory(&base, "note:c1:step2", "data2", None).unwrap();
        set_memory(&base, "note:c2:step1", "other", None).unwrap();

        let results = get_memories_by_prefix(&base, "note:c1:").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "note:c1:step1");
        assert_eq!(results[1].0, "note:c1:step2");
    }

    #[test]
    fn test_notes() {
        let (base, _dir) = setup();
        super::super::conversations::create_conversation(&base, "c1", "Test").unwrap();

        let content = serde_json::json!({"summary": "Step 1 completed"});
        save_note(&base, "c1", "step1_summary", &content).unwrap();

        let read = read_note(&base, "c1", "step1_summary").unwrap();
        assert!(read.is_some());
        assert_eq!(read.unwrap()["summary"], "Step 1 completed");
    }
}
