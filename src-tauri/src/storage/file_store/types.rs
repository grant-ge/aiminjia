//! Data structures for file-based storage.
//!
//! These types define the on-disk format for all stored data.
//! JSON files use `serde_json::to_string_pretty`, JSONL files use `serde_json::to_string`.

use serde::{Deserialize, Serialize};

// ─── Conversation ────────────────────────────────────────────────────────────

/// Conversation metadata stored in `conv.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub mode: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_archived: bool,
}

/// Lightweight entry in the global `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationIndexEntry {
    pub id: String,
    pub title: String,
    /// When the conversation was created. Defaults to empty string for backward
    /// compatibility with older index files that didn't store this field.
    #[serde(default)]
    pub created_at: String,
    pub updated_at: String,
    pub is_archived: bool,
}

/// Global conversation index stored in `index.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalIndex {
    pub conversations: Vec<ConversationIndexEntry>,
}

// ─── Message ─────────────────────────────────────────────────────────────────

/// A single message stored in `messages.{N}.jsonl`.
///
/// Messages support append-only updates: to update content, a new record with
/// the same `seq` but a higher `_rev` is appended. On read, only the highest
/// `_rev` per `seq` is kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub seq: u64,
    #[serde(rename = "_rev")]
    pub rev: u32,
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    /// Full message content as a JSON value (not a stringified JSON).
    pub content: serde_json::Value,
    pub created_at: String,
}

// ─── Files ───────────────────────────────────────────────────────────────────

/// A file entry (uploaded or generated) in `file_index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub id: String,
    /// `"upload"` or `"generated"`
    pub source: String,
    pub file_name: String,
    pub stored_path: String,
    pub file_type: String,
    pub file_size: i64,

    // ── Upload-specific fields ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_at: Option<String>,

    // ── Generated-specific fields ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub version: i32,
    #[serde(default = "default_true")]
    pub is_latest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_step: Option<i32>,

    // ── Common timestamps ──
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

fn default_true() -> bool {
    true
}

/// File index stored in `file_index.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileIndex {
    pub files: Vec<FileEntry>,
}

// ─── Analysis ────────────────────────────────────────────────────────────────

/// Analysis state stored in `analysis.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAnalysisState {
    pub conversation_id: String,
    pub current_step: i32,
    pub step_status: serde_json::Value,
    pub state_data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_status: Option<String>,
    pub updated_at: String,
}

// ─── Enterprise Memory ───────────────────────────────────────────────────────

/// A memory entry stored in `shared/memory/memory.jsonl`.
///
/// Uses last-writer-wins semantics: when reading, the last entry with a given
/// `key` is the current value. `deleted: true` means the key was removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}

fn is_false(b: &bool) -> bool {
    !(*b)
}

// ─── Audit Log ───────────────────────────────────────────────────────────────

/// An audit log entry stored in `audit/audit.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: String,
}

// ─── Search Cache ────────────────────────────────────────────────────────────

/// A search cache entry stored in `cache/{hash}.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub query_hash: String,
    pub query: String,
    pub results: String,
    pub expires_at: String,
}

// ─── Settings (Config) ──────────────────────────────────────────────────────

/// Application settings stored in `config.json`.
///
/// Uses a flat key-value map (same as the DB `settings` table) for compatibility
/// with `AppSettings::from_string_map()`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SettingsMap(pub std::collections::HashMap<String, String>);

/// Encrypted keys stored in `keys.enc`.
///
/// Each key is stored as `provider → encrypted_value`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncryptedKeys(pub std::collections::HashMap<String, String>);
