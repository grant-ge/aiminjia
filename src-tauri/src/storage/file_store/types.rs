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
    ///
    /// **过渡期保留**：新写入由 `source = Employee { employee_id }` 表达；本字段
    /// 由 dispatch 路径双写以兼容老桌面端读 conv.json。后续 PR 删此字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    /// 新增：会话来源 tagged union。`#[serde(default)]` 让老 conv.json（无此字段）
    /// 反序列化为 `User`。IM connector（dingtalk / telegram / ...）创建的 conversation
    /// 走 `ConversationSource::Im`；具体平台从 `channels/<platform>/sessions.json`
    /// 反查（这一层不持有 platform 信息）。
    #[serde(default)]
    pub source: ConversationSource,
    /// 新增：会话当前授权的本地工作目录（来自原 memory.jsonl 的 authorized_workspace 那条线）。
    /// 不含 `session_id`，由读取方按需补 session_id 字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_workspace: Option<PersistedAuthorizedWorkspace>,
    /// 新增：人类可读副标题（员工 display name / 团名 / IM 渠道描述）。
    /// LLM 改 title 不影响这个字段——专门用于稳住"会话来源是什么"的视觉识别。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    /// The name of the currently-active team for the Lead in this conversation.
    /// Written by TeamCreate / TeamSwitch tools; read during ctx construction
    /// to populate `ToolExecutionContext::active_team_name`.  `None` for
    /// single-agent (no-team) conversations or old conv.json files.
    ///
    /// 这是 runtime team-tools 的状态，跟 `source_label`（来源标签）是两件事。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_team_name: Option<String>,
    /// 用户置顶的会话。前端在排序时按 (pinned DESC, updatedAt DESC) 升序排列，
    /// 让置顶会话浮在列表顶部。`pinned_at` 仅作为多个置顶之间的二级排序键备用，
    /// 当前 UI 仅按是否置顶分桶。老 conv.json 无此字段时反序列化为 `false / None`。
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<String>,
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
    /// 来源 kind mirror（不含 id；点开会话才需要 id，从 conv.json 读取）。
    /// 老 index.json 无此字段时反序列化为 `User`。
    #[serde(default)]
    pub kind: ConversationKind,
    /// 新增：人类可读副标题 mirror。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    /// 新增：授权目录 displayName mirror（用于侧边栏分组）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// 用户置顶 mirror。同 `ConversationMeta.is_pinned`，写入由 pin/unpin 路径双写。
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<String>,
}

/// Global conversation index stored in `index.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalIndex {
    pub conversations: Vec<ConversationIndexEntry>,
}

// ─── Message ─────────────────────────────────────────────────────────────────

/// 错误信息（PR2 引入；与 claude-code-best `isApiErrorMessage:true` 守卫位等价）。
///
/// 守卫规则（spec §3.2）：
/// - UI 渲染：永远显示（红色 callout）
/// - 持久化：写盘保留
/// - 发给 LLM 下一轮：`history.rs::build_chat_history` 过滤掉
/// - session 恢复找上轮终点：跳过
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MessageError {
    pub kind: ErrorKind,
    /// UI 兜底渲染文案；i18n 标题由前端按 kind 查表
    pub message: String,
    /// 原始错误（脱敏后）；UI 默认不显示，仅 dev / 客户主动复制时透出
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    ChunkTimeout,
    Network,
    PromptTooLong,
    AuthFailed,
    RateLimited,
    MaxIterations,
    BudgetExceeded,
    ExecutionError,
    Unknown,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_compact_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// 错误信息（PR2 引入；spec §3.1）。
    /// - 顶层字段，与 `content` 同级（不塞进 content）
    /// - `serde(default)` 保证旧 messages.jsonl 反序列化时 `None`
    /// - `skip_serializing_if = "Option::is_none"` 保证正常消息不写这字段
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,
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
        assert_eq!(
            serde_json::to_string(&ConversationKind::User).unwrap(),
            "\"user\""
        );
        assert_eq!(
            serde_json::to_string(&ConversationKind::Employee).unwrap(),
            "\"employee\""
        );
        assert_eq!(
            serde_json::to_string(&ConversationKind::ExpertTeam).unwrap(),
            "\"expertTeam\""
        );
        assert_eq!(
            serde_json::to_string(&ConversationKind::Im).unwrap(),
            "\"im\""
        );
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

#[cfg(test)]
mod conversation_meta_migration_tests {
    use super::*;

    #[test]
    fn old_conv_json_without_new_fields_deserializes_with_defaults() {
        let old_json = r#"{
            "id": "c-1",
            "title": "old conversation",
            "createdAt": "2026-04-01T00:00:00+00:00",
            "updatedAt": "2026-04-01T00:00:00+00:00",
            "isArchived": false
        }"#;
        let meta: ConversationMeta = serde_json::from_str(old_json).unwrap();
        assert_eq!(meta.id, "c-1");
        assert_eq!(meta.source, ConversationSource::User);
        assert!(meta.authorized_workspace.is_none());
        assert!(meta.source_label.is_none());
        assert!(meta.employee_id.is_none());
    }

    #[test]
    fn employee_dispatch_old_conv_json_still_works() {
        // 老格式：只有 employee_id 字段，没有 source。
        let old_json = r#"{
            "id": "c-2",
            "title": "dispatch session",
            "createdAt": "2026-04-15T00:00:00+00:00",
            "updatedAt": "2026-04-15T00:00:00+00:00",
            "isArchived": false,
            "employeeId": "emp-001"
        }"#;
        let meta: ConversationMeta = serde_json::from_str(old_json).unwrap();
        assert_eq!(meta.employee_id.as_deref(), Some("emp-001"));
        // source 仍是 User（兼容期的兜底语义），需要 dispatch 路径在新代码里双写
        assert_eq!(meta.source, ConversationSource::User);
    }

    #[test]
    fn new_conv_json_with_all_fields_round_trips() {
        let meta = ConversationMeta {
            id: "c-3".to_string(),
            title: "new conversation".to_string(),
            created_at: "2026-05-20T00:00:00+00:00".to_string(),
            updated_at: "2026-05-20T00:00:00+00:00".to_string(),
            is_archived: false,
            model_override: None,
            employee_id: Some("emp-002".to_string()),
            source: ConversationSource::Employee {
                employee_id: "emp-002".to_string(),
            },
            authorized_workspace: Some(PersistedAuthorizedWorkspace {
                id: "ws-1".to_string(),
                root_path: PathBuf::from("/Users/foo"),
                display_name: "foo".to_string(),
                authorized_at: "2026-05-20T00:00:00+00:00".to_string(),
            }),
            source_label: Some("小销".to_string()),
            active_team_name: None,
            is_pinned: false,
            pinned_at: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: ConversationMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "c-3");
        assert_eq!(parsed.employee_id.as_deref(), Some("emp-002"));
        assert!(matches!(
            parsed.source,
            ConversationSource::Employee { ref employee_id } if employee_id == "emp-002"
        ));
        assert_eq!(parsed.source_label.as_deref(), Some("小销"));
        assert!(parsed.authorized_workspace.is_some());
    }
}

#[cfg(test)]
mod stored_message_error_tests {
    use super::*;

    #[test]
    fn stored_message_deserialize_without_error_field_yields_none() {
        // 旧 messages.jsonl 不含 error 字段，反序列化必须成功且 error=None
        let json = r#"{
            "id": "msg-1",
            "conversationId": "conv-1",
            "role": "assistant",
            "content": {"text": "hi"},
            "createdAt": "2026-05-28T00:00:00Z"
        }"#;
        let m: StoredMessage = serde_json::from_str(json).unwrap();
        assert_eq!(m.error, None);
    }

    #[test]
    fn stored_message_serialize_omits_error_when_none() {
        // 正常消息不应写 error 字段（保持 messages.jsonl 紧凑）
        let m = StoredMessage {
            seq: None,
            rev: None,
            id: "msg-1".to_string(),
            conversation_id: "conv-1".to_string(),
            role: "assistant".to_string(),
            content: serde_json::json!({"text": "hi"}),
            created_at: "2026-05-28T00:00:00Z".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            subtype: None,
            compact_metadata: None,
            is_compact_summary: None,
            run_id: None,
            schema_version: None,
            sequence: None,
            error: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(
            !s.contains("error"),
            "serialized form must not contain 'error' field when None: {}",
            s
        );
    }

    #[test]
    fn stored_message_error_roundtrip_camelcase() {
        // error 字段往返序列化保持 snake_case kind
        let m = StoredMessage {
            seq: None,
            rev: None,
            id: "msg-1".to_string(),
            conversation_id: "conv-1".to_string(),
            role: "assistant".to_string(),
            content: serde_json::json!({"text": ""}),
            created_at: "2026-05-28T00:00:00Z".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            subtype: None,
            compact_metadata: None,
            is_compact_summary: None,
            run_id: None,
            schema_version: None,
            sequence: None,
            error: Some(MessageError {
                kind: ErrorKind::ChunkTimeout,
                message: "AI 服务暂时无法响应".to_string(),
                raw: Some("Chunk timeout (90s)".to_string()),
            }),
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(
            s.contains(r#""kind":"chunk_timeout""#),
            "kind must be snake_case: {}",
            s
        );
        let back: StoredMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(back.error, m.error);
    }
}

#[cfg(test)]
mod conversation_index_entry_migration_tests {
    use super::*;

    #[test]
    fn old_index_entry_deserializes_with_defaults() {
        let old_json = r#"{
            "id": "c-1",
            "title": "old",
            "createdAt": "2026-04-01T00:00:00+00:00",
            "updatedAt": "2026-04-01T00:00:00+00:00",
            "isArchived": false
        }"#;
        let entry: ConversationIndexEntry = serde_json::from_str(old_json).unwrap();
        assert_eq!(entry.kind, ConversationKind::User);
        assert!(entry.source_label.is_none());
        assert!(entry.workspace_name.is_none());
    }

    #[test]
    fn new_index_entry_round_trips() {
        let entry = ConversationIndexEntry {
            id: "c-2".to_string(),
            title: "new".to_string(),
            created_at: "2026-05-20T00:00:00+00:00".to_string(),
            updated_at: "2026-05-20T00:00:00+00:00".to_string(),
            is_archived: false,
            kind: ConversationKind::ExpertTeam,
            source_label: Some("市场专家团".to_string()),
            workspace_name: Some("foo-project".to_string()),
            is_pinned: false,
            pinned_at: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"kind\":\"expertTeam\""));
        assert!(json.contains("\"sourceLabel\":\"市场专家团\""));
        assert!(json.contains("\"workspaceName\":\"foo-project\""));
        let parsed: ConversationIndexEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kind, ConversationKind::ExpertTeam);
        assert_eq!(parsed.source_label.as_deref(), Some("市场专家团"));
        assert_eq!(parsed.workspace_name.as_deref(), Some("foo-project"));
    }
}
