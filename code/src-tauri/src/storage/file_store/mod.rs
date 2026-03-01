//! File-based storage — complete replacement for SQLite.
//!
//! `AppStorage` provides the same public API as the old `Database` struct,
//! backed by JSON/JSONL files on disk.
//!
#![allow(dead_code)]
//! ## Directory layout
//!
//! ```text
//! {base_dir}/
//! ├── config.json              # App settings (key-value)
//! ├── index.json               # Global conversation index
//! ├── audit/                   # Audit log (auto-split at 2MB)
//! │   └── audit.jsonl
//! ├── shared/                  # Cross-conversation data
//! │   ├── memory/              # Enterprise memory (auto-split at 1MB)
//! │   │   └── memory.jsonl
//! │   └── cache/               # Search cache (per-query JSON)
//! └── conversations/           # Per-conversation isolation
//!     └── {id}/
//!         ├── conv.json        # Metadata (title, mode, timestamps)
//!         ├── file_index.json  # File records (uploads + generated)
//!         ├── analysis.json    # Analysis state
//!         ├── _current         # Shard metadata "{shard}:{seq}"
//!         ├── messages.N.jsonl # Message shards (100 msgs each)
//!         ├── notes/           # Analysis notes
//!         ├── uploads/         # Physical uploaded files
//!         ├── generated/       # Physical generated files
//!         └── run.lock         # PID-based agent lock
//! ```

pub mod analysis;
pub mod audit;
pub mod cache;
pub mod config;
pub mod conversations;
pub mod error;
pub mod files;
pub mod id;
pub mod io;
pub mod messages;
pub mod notes;
pub mod types;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use log::{info, warn};

/// Thread-safe file-based storage handle.
///
/// Write operations are serialized through `write_lock` to prevent
/// read-modify-write races on shared JSON files (config.json, index.json, etc.).
/// Read-only operations proceed without locking.
pub struct AppStorage {
    base_dir: PathBuf,
    /// Serializes all mutating file operations to prevent read-modify-write races.
    write_lock: Mutex<()>,
}

impl AppStorage {
    /// Create a new AppStorage, initializing the directory structure.
    pub fn new(base_dir: &Path) -> Result<Self> {
        let storage = Self {
            base_dir: base_dir.to_path_buf(),
            write_lock: Mutex::new(()),
        };
        storage.initialize()?;
        info!("File storage initialized at {:?}", base_dir);
        Ok(storage)
    }

    /// Get the base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Initialization
    // ═══════════════════════════════════════════════════════════════════════

    fn initialize(&self) -> Result<()> {
        // Create directory structure
        fs::create_dir_all(self.base_dir.join("conversations"))?;
        fs::create_dir_all(self.base_dir.join("shared").join("memory"))?;
        fs::create_dir_all(self.base_dir.join("shared").join("cache"))?;
        fs::create_dir_all(self.base_dir.join("audit"))?;

        // Reconcile global index with actual directories
        conversations::reconcile_index(&self.base_dir)
            .context("Failed to reconcile conversation index")?;

        // Clean up expired search cache
        let cleaned = cache::cleanup_expired_cache(&self.base_dir).unwrap_or(0);
        if cleaned > 0 {
            info!("Cleaned up {} expired cache entries", cleaned);
        }

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Conversations
    // ═══════════════════════════════════════════════════════════════════════

    pub fn create_conversation(&self, id: &str, title: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        conversations::create_conversation(&self.base_dir, id, title)?;
        Ok(())
    }

    pub fn update_conversation_title(&self, id: &str, title: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        conversations::update_conversation_title(&self.base_dir, id, title)?;
        Ok(())
    }

    pub fn get_conversation_mode(&self, id: &str) -> Result<String> {
        Ok(conversations::get_conversation_mode(&self.base_dir, id)?)
    }

    pub fn set_conversation_mode(&self, id: &str, mode: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        conversations::set_conversation_mode(&self.base_dir, id, mode)?;
        Ok(())
    }

    pub fn get_conversations(&self) -> Result<Vec<serde_json::Value>> {
        Ok(conversations::get_conversations(&self.base_dir)?)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        conversations::delete_conversation(&self.base_dir, id)?;
        Ok(())
    }

    pub fn get_file_paths_for_conversation(&self, conversation_id: &str) -> Result<Vec<String>> {
        Ok(conversations::get_file_paths_for_conversation(
            &self.base_dir,
            conversation_id,
        )?)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Messages
    // ═══════════════════════════════════════════════════════════════════════

    pub fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        messages::insert_message(&self.base_dir, id, conversation_id, role, content_json)?;
        Ok(())
    }

    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<serde_json::Value>> {
        Ok(messages::get_messages(&self.base_dir, conversation_id)?)
    }

    pub fn get_recent_messages(
        &self,
        conversation_id: &str,
        limit: u32,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(messages::get_recent_messages(
            &self.base_dir,
            conversation_id,
            limit,
        )?)
    }

    pub fn update_message_content(
        &self,
        id: &str,
        conversation_id: &str,
        content_json: &str,
    ) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        messages::update_message_content(&self.base_dir, id, conversation_id, content_json)?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Settings
    // ═══════════════════════════════════════════════════════════════════════

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(config::get_setting(&self.base_dir, key)?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        config::set_setting(&self.base_dir, key, value)?;
        Ok(())
    }

    pub fn get_all_settings(&self) -> Result<HashMap<String, String>> {
        Ok(config::get_all_settings(&self.base_dir)?)
    }

    pub fn get_settings_by_prefix(&self, prefix: &str) -> Result<HashMap<String, String>> {
        Ok(config::get_settings_by_prefix(&self.base_dir, prefix)?)
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        config::delete_setting(&self.base_dir, key)?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Generated Files
    // ═══════════════════════════════════════════════════════════════════════

    #[allow(clippy::too_many_arguments)]
    pub fn insert_generated_file(
        &self,
        id: &str,
        conversation_id: &str,
        message_id: Option<&str>,
        file_name: &str,
        stored_path: &str,
        file_type: &str,
        file_size: i64,
        category: &str,
        description: Option<&str>,
        version: i32,
        is_latest: bool,
        superseded_by: Option<&str>,
        created_by_step: Option<i32>,
        expires_at: Option<&str>,
    ) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        files::insert_generated_file(
            &self.base_dir,
            id,
            conversation_id,
            message_id,
            file_name,
            stored_path,
            file_type,
            file_size,
            category,
            description,
            version,
            is_latest,
            superseded_by,
            created_by_step,
            expires_at,
        )?;
        Ok(())
    }

    pub fn get_generated_files_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(files::get_generated_files_for_conversation(
            &self.base_dir,
            conversation_id,
        )?)
    }

    pub fn get_generated_files_by_ids(&self, ids: &[String]) -> Result<Vec<serde_json::Value>> {
        Ok(files::get_generated_files_by_ids(&self.base_dir, ids)?)
    }

    pub fn get_generated_file_for_conversation(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        Ok(files::get_generated_file_for_conversation(
            &self.base_dir,
            id,
            conversation_id,
        )?)
    }

    pub fn mark_file_superseded(&self, old_id: &str, new_id: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        files::mark_file_superseded(&self.base_dir, old_id, new_id)?;
        Ok(())
    }

    pub fn find_expired_temp_files(&self) -> Result<Vec<serde_json::Value>> {
        Ok(files::find_expired_temp_files(&self.base_dir)?)
    }

    pub fn delete_generated_file(&self, id: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        files::delete_generated_file(&self.base_dir, id)?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Uploaded Files
    // ═══════════════════════════════════════════════════════════════════════

    #[allow(clippy::too_many_arguments)]
    pub fn insert_uploaded_file(
        &self,
        id: &str,
        conversation_id: &str,
        original_name: &str,
        stored_path: &str,
        file_type: &str,
        file_size: i64,
        parsed_summary: Option<&str>,
    ) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        files::insert_uploaded_file(
            &self.base_dir,
            id,
            conversation_id,
            original_name,
            stored_path,
            file_type,
            file_size,
            parsed_summary,
        )?;
        Ok(())
    }

    pub fn get_uploaded_file(&self, id: &str) -> Result<Option<serde_json::Value>> {
        Ok(files::get_uploaded_file(&self.base_dir, id)?)
    }

    pub fn get_uploaded_files_by_ids(&self, ids: &[String]) -> Result<Vec<serde_json::Value>> {
        Ok(files::get_uploaded_files_by_ids(&self.base_dir, ids)?)
    }

    pub fn get_uploaded_file_for_conversation(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        Ok(files::get_uploaded_file_for_conversation(
            &self.base_dir,
            id,
            conversation_id,
        )?)
    }

    pub fn get_uploaded_files_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(files::get_uploaded_files_for_conversation(
            &self.base_dir,
            conversation_id,
        )?)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Analysis States
    // ═══════════════════════════════════════════════════════════════════════

    pub fn upsert_analysis_state(
        &self,
        conversation_id: &str,
        current_step: i32,
        step_status: &str,
        state_data: &str,
    ) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        analysis::upsert_analysis_state(
            &self.base_dir,
            conversation_id,
            current_step,
            step_status,
            state_data,
        )?;
        Ok(())
    }

    pub fn get_analysis_state(
        &self,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        Ok(analysis::get_analysis_state(
            &self.base_dir,
            conversation_id,
        )?)
    }

    pub fn finalize_analysis(&self, conversation_id: &str, final_status: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        analysis::finalize_analysis(&self.base_dir, conversation_id, final_status)?;
        Ok(())
    }

    pub fn reset_stuck_analysis_state(&self, conversation_id: &str) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        analysis::reset_stuck_analysis_state(&self.base_dir, conversation_id)?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Audit Log
    // ═══════════════════════════════════════════════════════════════════════

    pub fn log_action(&self, action: &str, detail: Option<&str>) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        audit::log_action(&self.base_dir, action, detail)?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Enterprise Memory
    // ═══════════════════════════════════════════════════════════════════════

    pub fn get_memory(&self, key: &str) -> Result<Option<String>> {
        Ok(notes::get_memory(&self.base_dir, key)?)
    }

    pub fn set_memory(&self, key: &str, value: &str, source: Option<&str>) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        notes::set_memory(&self.base_dir, key, value, source)?;
        Ok(())
    }

    pub fn get_memories_by_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        Ok(notes::get_memories_by_prefix(&self.base_dir, prefix)?)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Search Cache
    // ═══════════════════════════════════════════════════════════════════════

    pub fn upsert_search_cache(
        &self,
        query_hash: &str,
        query: &str,
        results: &str,
        expires_at: &str,
    ) -> Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        cache::upsert_search_cache(&self.base_dir, query_hash, query, results, expires_at)?;
        Ok(())
    }

    pub fn get_search_cache(
        &self,
        query_hash: &str,
    ) -> Result<Option<types::CacheEntry>> {
        Ok(cache::get_search_cache(&self.base_dir, query_hash)?)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Active Agent Tasks (crash recovery via run.lock)
    // ═══════════════════════════════════════════════════════════════════════

    /// Record that an agent task has started for a conversation.
    pub fn insert_active_task(&self, conversation_id: &str) -> Result<()> {
        let lock_path = conversations::conv_dir(&self.base_dir, conversation_id).join("run.lock");
        // Format: "PID:UNIX_TIMESTAMP" — enables 24h staleness detection in get_orphaned_tasks()
        let content = format!("{}:{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&lock_path, &content)?;
        Ok(())
    }

    /// Remove the active task record for a conversation.
    pub fn remove_active_task(&self, conversation_id: &str) -> Result<()> {
        let lock_path = conversations::conv_dir(&self.base_dir, conversation_id).join("run.lock");
        if lock_path.exists() {
            let _ = fs::remove_file(&lock_path);
        }
        Ok(())
    }

    /// Get all orphaned tasks (run.lock files with dead PIDs or stale timestamps).
    pub fn get_orphaned_tasks(&self) -> Result<Vec<String>> {
        let conversations_dir = self.base_dir.join("conversations");
        if !conversations_dir.exists() {
            return Ok(Vec::new());
        }

        let mut orphans = Vec::new();
        for entry in fs::read_dir(&conversations_dir)?.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let conv_id = entry.file_name().to_string_lossy().to_string();
            let lock_path = entry.path().join("run.lock");
            if lock_path.exists() {
                // Parse "PID:TIMESTAMP" format (backward compatible with old "PID" format)
                let content = fs::read_to_string(&lock_path).unwrap_or_default();
                let parts: Vec<&str> = content.trim().splitn(2, ':').collect();
                let pid = parts.first().and_then(|p| p.parse::<u32>().ok());
                let timestamp = parts.get(1).and_then(|t| t.parse::<u64>().ok());

                match pid {
                    Some(pid) => {
                        let is_dead = !io::process_alive(pid);
                        let is_stale = timestamp.map_or(false, |ts| {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            now.saturating_sub(ts) > 86400 // 24 hours
                        });
                        if is_dead || is_stale {
                            orphans.push(conv_id);
                        }
                    }
                    None => {
                        // Corrupted lock file
                        orphans.push(conv_id);
                    }
                }
            }
        }

        Ok(orphans)
    }

    /// Delete all orphaned task rows and return their conversation IDs.
    pub fn cleanup_orphaned_tasks(&self) -> Result<Vec<String>> {
        let orphans = self.get_orphaned_tasks()?;
        for conv_id in &orphans {
            let lock_path =
                conversations::conv_dir(&self.base_dir, conv_id).join("run.lock");
            if lock_path.exists() {
                let _ = fs::remove_file(&lock_path);
                warn!("Cleaned up orphan lock for conversation {}", conv_id);
            }
        }
        Ok(orphans)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_storage() -> (AppStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = AppStorage::new(dir.path()).unwrap();
        (storage, dir)
    }

    #[test]
    fn test_full_conversation_lifecycle() {
        let (storage, _dir) = test_storage();

        // Create conversation
        storage.create_conversation("c1", "Test Conv").unwrap();
        let convs = storage.get_conversations().unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0]["title"], "Test Conv");

        // Insert messages
        storage
            .insert_message("m1", "c1", "user", r#"{"text":"hello"}"#)
            .unwrap();
        storage
            .insert_message("m2", "c1", "assistant", r#"{"text":"hi"}"#)
            .unwrap();

        let msgs = storage.get_messages("c1").unwrap();
        assert_eq!(msgs.len(), 2);

        // Recent messages
        let recent = storage.get_recent_messages("c1", 1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0]["role"], "assistant");

        // Delete conversation
        storage.delete_conversation("c1").unwrap();
        let convs = storage.get_conversations().unwrap();
        assert_eq!(convs.len(), 0);
    }

    #[test]
    fn test_settings_lifecycle() {
        let (storage, _dir) = test_storage();

        assert_eq!(storage.get_setting("theme").unwrap(), None);

        storage.set_setting("theme", "dark").unwrap();
        assert_eq!(
            storage.get_setting("theme").unwrap(),
            Some("dark".to_string())
        );

        let all = storage.get_all_settings().unwrap();
        assert_eq!(all["theme"], "dark");
    }

    #[test]
    fn test_enterprise_memory() {
        let (storage, _dir) = test_storage();

        storage
            .set_memory("company", "Acme Corp", Some("onboarding"))
            .unwrap();
        assert_eq!(
            storage.get_memory("company").unwrap(),
            Some("Acme Corp".to_string())
        );

        // Update
        storage.set_memory("company", "Acme Inc", None).unwrap();
        assert_eq!(
            storage.get_memory("company").unwrap(),
            Some("Acme Inc".to_string())
        );
    }

    #[test]
    fn test_analysis_lifecycle() {
        let (storage, _dir) = test_storage();

        storage.create_conversation("c1", "Conv").unwrap();

        assert!(storage.get_analysis_state("c1").unwrap().is_none());

        storage
            .upsert_analysis_state("c1", 2, r#"{"step1":"done"}"#, r#"{"key":"val"}"#)
            .unwrap();

        let state = storage.get_analysis_state("c1").unwrap().unwrap();
        assert_eq!(state["currentStep"], 2);

        storage.finalize_analysis("c1", "completed").unwrap();
        let state = storage.get_analysis_state("c1").unwrap().unwrap();
        assert_eq!(state["finalStatus"], "completed");
    }

    #[test]
    fn test_file_records() {
        let (storage, _dir) = test_storage();
        storage.create_conversation("c1", "Conv").unwrap();

        storage
            .insert_uploaded_file(
                "uf1",
                "c1",
                "data.csv",
                "/tmp/data.csv",
                "csv",
                512,
                Some("100 rows"),
            )
            .unwrap();

        let file = storage
            .get_uploaded_file_for_conversation("uf1", "c1")
            .unwrap();
        assert!(file.is_some());

        storage
            .insert_generated_file(
                "gf1",
                "c1",
                None,
                "report.pdf",
                "/tmp/report.pdf",
                "pdf",
                1024,
                "report",
                Some("Monthly"),
                1,
                true,
                None,
                Some(3),
                None,
            )
            .unwrap();

        let files = storage
            .get_generated_files_for_conversation("c1")
            .unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_active_tasks() {
        let (storage, _dir) = test_storage();
        storage.create_conversation("c1", "Conv").unwrap();

        storage.insert_active_task("c1").unwrap();

        // Our own PID is alive, so it should NOT be in orphans
        let orphans = storage.get_orphaned_tasks().unwrap();
        assert!(orphans.is_empty());

        storage.remove_active_task("c1").unwrap();
    }

    #[test]
    fn test_audit_log() {
        let (storage, _dir) = test_storage();
        storage
            .log_action("conversation_created", Some("id=c1"))
            .unwrap();
        storage.log_action("file_deleted", None).unwrap();
        // Smoke test — just verify it doesn't panic
    }

    #[test]
    fn test_search_cache() {
        let (storage, _dir) = test_storage();

        storage
            .upsert_search_cache("h1", "query", "{}", "2099-12-31T23:59:59Z")
            .unwrap();

        let entry = storage.get_search_cache("h1").unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().query, "query");
    }
}
