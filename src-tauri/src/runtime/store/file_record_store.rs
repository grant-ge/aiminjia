//! Domain traits for file record access (uploaded files and generated files).
//!
//! Commands that read/write file records (`export`, `file`) go through these traits
//! instead of calling `AppStorage` directly.

use anyhow::Result;

pub trait FileRecordStore: Send + Sync {
    // ── Uploaded files ──────────────────────────────────────────────────────
    fn insert_uploaded_file(
        &self,
        id: &str,
        conversation_id: &str,
        original_name: &str,
        stored_path: &str,
        file_type: &str,
        file_size: i64,
        parsed_summary: Option<&str>,
    ) -> Result<()>;

    fn get_uploaded_file_for_conversation(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>>;

    fn get_uploaded_files_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>>;

    fn delete_uploaded_file(&self, id: &str, conversation_id: &str) -> Result<()>;

    // ── Generated files ─────────────────────────────────────────────────────
    #[allow(clippy::too_many_arguments)]
    fn insert_generated_file(
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
    ) -> Result<()>;

    fn get_generated_file_for_conversation(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>>;

    fn get_generated_files_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>>;
}
