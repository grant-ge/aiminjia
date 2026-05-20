//! Data structures for file-based storage.
//!
//! These types define the on-disk format for all stored data.
//! JSON files use `serde_json::to_string_pretty`, JSONL files use `serde_json::to_string`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ─── Conversation ────────────────────────────────────────────────────────────

/// Authorized workspace stored on disk inside `conv.json`.
///
/// 不含 `session_id`：那是 runtime 内部 ID 概念，不应该被 disk 格式吃住。
/// `AuthorizedWorkspace`（runtime 层、带 session_id）写盘前由调用方手动映射成
/// `PersistedAuthorizedWorkspace`；读盘后由调用方按需补 session_id。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAuthorizedWorkspace {
    pub id: String,
    pub root_path: PathBuf,
    pub display_name: String,
    pub authorized_at: String,
}

/// Conversation source kind, mirrored from `ConversationSource` into `ConversationIndexEntry`.
/// Drives sidebar grouping + icon rendering — only `kind`, no IDs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConversationKind {
    #[default]
    User,
    Employee,
    ExpertTeam,
    Im,
}

/// 会话来源 tagged union。序列化为 `{"kind":"...","employeeId":"..."}` 等扁平结构。
///
/// 反序列化兜底：未知 `kind` → `User`（避免老桌面端打开未来加入的 variant 时挂掉）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationSource {
    User,
    Employee {
        #[serde(rename = "employeeId")]
        employee_id: String,
    },
    ExpertTeam {
        #[serde(rename = "expertTeamId")]
        expert_team_id: String,
    },
    Im,
}

impl Default for ConversationSource {
    fn default() -> Self {
        Self::User
    }
}

impl<'de> Deserialize<'de> for ConversationSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 先反序列化成通用 Value，再按 kind 字段分流
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("user");
        match kind {
            "user" => Ok(Self::User),
            "employee" => {
                let employee_id = value
                    .get("employeeId")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                match employee_id {
                    Some(id) if !id.is_empty() => Ok(Self::Employee { employee_id: id }),
                    _ => Ok(Self::User), // 缺字段 → 退化为 User
                }
            }
            "expertTeam" => {
                let team_id = value
                    .get("expertTeamId")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                match team_id {
                    Some(id) if !id.is_empty() => Ok(Self::ExpertTeam { expert_team_id: id }),
                    _ => Ok(Self::User),
                }
            }
            "im" => Ok(Self::Im),
            _ => Ok(Self::User), // 未知 kind → User
        }
    }
}

/// Conversation metadata stored in `conv.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_archived: bool,
    /// Reserved for backwards compatibility — older conv.json files may carry
    /// `modelOverride` (legacy per-conversation model selector, removed
    /// 2026-05). Skipped on serialize so we don't keep rewriting it; kept on
    /// deserialize so we don't reject legacy files.
    #[serde(default, skip_serializing)]
    pub model_override: Option<String>,
    /// Set by `dispatch_employee_run` when a conversation is created by
    /// dispatching a digital employee. UI uses this to render the employee
    /// identity card in the chat top bar. Empty / None for user-initiated
    /// conversations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
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
    /// Mirror of `ConversationMeta.employee_id`. Stored in the index so the
    /// sidebar / top bar don't have to fan-out and read every `conv.json` to
    /// know whether a conversation is a dispatch session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
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
    #[serde(default)]
    pub seq: Option<u64>,
    #[serde(rename = "_rev", default)]
    pub rev: Option<u32>,
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    /// Full message content as a JSON value (not a stringified JSON).
    pub content: serde_json::Value,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
}

impl StoredMessage {
    pub fn text(&self) -> &str {
        self.content
            .get("text")
            .and_then(|value| value.as_str())
            .or_else(|| self.content.as_str())
            .unwrap_or("")
    }

    pub fn seq_or(&self, default: u64) -> u64 {
        self.seq.unwrap_or(default)
    }

    pub fn rev_or(&self, default: u32) -> u32 {
        self.rev.unwrap_or(default)
    }
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

#[cfg(test)]
mod persisted_authorized_workspace_tests {
    use super::*;

    #[test]
    fn round_trip_serialize_deserialize() {
        let original = PersistedAuthorizedWorkspace {
            id: "ws-1".to_string(),
            root_path: PathBuf::from("/Users/foo/bar"),
            display_name: "bar".to_string(),
            authorized_at: "2026-05-20T00:00:00+00:00".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"rootPath\""));
        assert!(json.contains("\"displayName\""));
        assert!(json.contains("\"authorizedAt\""));
        let parsed: PersistedAuthorizedWorkspace = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn no_session_id_field() {
        let ws = PersistedAuthorizedWorkspace {
            id: "ws-1".to_string(),
            root_path: PathBuf::from("/x"),
            display_name: "x".to_string(),
            authorized_at: "t".to_string(),
        };
        let json = serde_json::to_string(&ws).unwrap();
        assert!(
            !json.contains("sessionId"),
            "PersistedAuthorizedWorkspace must NOT carry sessionId; got: {}",
            json
        );
    }
}

#[cfg(test)]
mod conversation_kind_tests {
    use super::*;

    #[test]
    fn serializes_as_camel_case() {
        assert_eq!(serde_json::to_string(&ConversationKind::User).unwrap(), "\"user\"");
        assert_eq!(serde_json::to_string(&ConversationKind::Employee).unwrap(), "\"employee\"");
        assert_eq!(serde_json::to_string(&ConversationKind::ExpertTeam).unwrap(), "\"expertTeam\"");
        assert_eq!(serde_json::to_string(&ConversationKind::Im).unwrap(), "\"im\"");
    }

    #[test]
    fn default_is_user() {
        assert_eq!(ConversationKind::default(), ConversationKind::User);
    }

    #[test]
    fn deserialize_unknown_string_errors() {
        // ConversationKind itself has no #[serde(other)] catch-all.
        // Unknown strings must fail to deserialize — `ConversationIndexEntry` will
        // then use `#[serde(default = "...")]` at the outer level to fall back.
        let result: Result<ConversationKind, _> = serde_json::from_str("\"futureVariant\"");
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod conversation_source_tests {
    use super::*;

    #[test]
    fn user_round_trip() {
        let s = ConversationSource::User;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"user"}"#);
        let parsed: ConversationSource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ConversationSource::User);
    }

    #[test]
    fn employee_round_trip() {
        let s = ConversationSource::Employee {
            employee_id: "emp-001".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"employee\""));
        assert!(json.contains("\"employeeId\":\"emp-001\""));
        let parsed: ConversationSource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn expert_team_round_trip() {
        let s = ConversationSource::ExpertTeam {
            expert_team_id: "marketing".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"expertTeam\""));
        assert!(json.contains("\"expertTeamId\":\"marketing\""));
        let parsed: ConversationSource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn im_round_trip() {
        let s = ConversationSource::Im;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"im"}"#);
        let parsed: ConversationSource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ConversationSource::Im);
    }

    #[test]
    fn unknown_kind_falls_back_to_user() {
        let json = r#"{"kind":"futureVariant","foo":"bar"}"#;
        let parsed: ConversationSource = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, ConversationSource::User);
    }

    #[test]
    fn employee_missing_id_falls_back_to_user() {
        let json = r#"{"kind":"employee"}"#;
        let parsed: ConversationSource = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, ConversationSource::User);
    }

    #[test]
    fn employee_empty_id_falls_back_to_user() {
        let json = r#"{"kind":"employee","employeeId":""}"#;
        let parsed: ConversationSource = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, ConversationSource::User);
    }

    #[test]
    fn default_is_user() {
        assert_eq!(ConversationSource::default(), ConversationSource::User);
    }
}
