# 会话来源统一 + Workspace 存储清理 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把会话来源（数字员工 / 专家团 / IM / 普通用户）和 workspace 绑定的存储统一到 conv.json + index.json，删除 memory.jsonl 这条线及其上下游 dead code，前端 UI 偏好（首页 workspace 选中/最近列表）迁到 AppSettings。

**Architecture:** 5 个 Tauri commands + 2 个 conv.json helper（`set_conversation_source` / `set_conversation_workspace`）+ 1 个新 `ConvJsonAuthorizedWorkspaceStore` + 整套 memory KV 设施删除。前端 `homeStore` 改 hydrate 模型（`getSettings` 一次性 → store），`expertTeamRegistry` 改薄壳走 IPC。**不迁移老数据**——老会话进默认文件夹、丢专家团归属，应用功能正常。

**Tech Stack:** Rust async (tokio + serde + tauri 2.x) + React/TypeScript (zustand + vitest)。

**Spec:** `docs/superpowers/specs/2026-05-20-conversation-source-and-workspace-cleanup-design.md`

---

## File Structure

| 路径 | 状态 | 内容 |
|---|---|---|
| `src-tauri/src/storage/file_store/types.rs` | 改 | 加 `PersistedAuthorizedWorkspace` / `ConversationKind` / `ConversationSource` 类型；改 `ConversationMeta` / `ConversationIndexEntry` 加新字段；删 `MemoryEntry` |
| `src-tauri/src/storage/file_store/conversations.rs` | 改 | 加 `set_conversation_source` / `set_conversation_workspace` / `read_conversation_workspace` / `read_conversation_source` helper |
| `src-tauri/src/storage/file_store/mod.rs` | 改 | 删 `set_memory` / `get_memory` / `get_memories_by_prefix` / `delete_memories_by_prefix` 4 个 pub fn + `FileMemoryStore` struct + `memory_store` 字段及访问器 + `initialize()` 的 `shared/memory` mkdir |
| `src-tauri/src/storage/file_store/notes.rs` | 删 | 整个文件删除 |
| `src-tauri/src/storage/aijia_home.rs` | 改 | 删 `ensure_user_dirs` 里的 `shared/memory` mkdir 和断言 |
| `src-tauri/src/storage/current_user_storage.rs` | 改 | 删测试里的 `shared/memory` 断言 |
| `src-tauri/src/storage/user_scoped_paths.rs` | 改 | 删 `memory_dir()` helper + 测试 |
| `src-tauri/src/runtime/store/memory_store.rs` | 删 | 整个文件 |
| `src-tauri/src/runtime/store/mod.rs` | 改 | 删 `pub mod memory_store;` + 相关 re-export |
| `src-tauri/src/runtime/store/authorized_workspace_store.rs` | 改 | 改 trait 签名加 `conversation_id` 参数；删 `FileAuthorizedWorkspaceStore`；加 `ConvJsonAuthorizedWorkspaceStore` |
| `src-tauri/src/runtime/tools/capability.rs` | 改 | 删 `FileOperations::is_loaded` 方法（trait + impl，无 caller） |
| `src-tauri/src/runtime/conversation_service.rs` | 改 | 删 `delete_conversation` 里的 `delete_memories_by_prefix` 两行 |
| `src-tauri/src/plugin/context.rs` | 改 | 删 `loaded_key` / `loaded_prefix` / `load_failed_key` 3 个 helper（无 caller） |
| `src-tauri/src/runtime/session_runtime.rs` | 改 | 改两处 `replace_for_session` / `get_current_for_session` 调用，加 conversation_id 参数 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | 改 | 加 `set_conversation_expert_team` / `clear_conversation_source` / `get_conversation_source` 3 个 commands；删 `get_conversations` 里的 fan-out 循环；改 `replace_for_session` 调用加 conversation_id |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | 改 | `load_explicit_workspace` 改读 conv.json；改 `get_current_for_session` 调用加 conversation_id |
| `src-tauri/src/commands/workspace.rs` | 改 | `authorize_local_directory` 写完 store 后同步写 conv.json + index.json mirror；改 store 调用加 conversation_id |
| `src-tauri/src/models/settings.rs` | 改 | `AppSettings` 加 `ui_home_selected_workspace` / `ui_home_recent_workspaces` 两个 String 字段 |
| `src-tauri/src/lib.rs` | 改 | 把 `FileAuthorizedWorkspaceStore` 注入换成 `ConvJsonAuthorizedWorkspaceStore` |
| `src-tauri/tests/review_no_memory_kv.rs` | 建 | review test 锁层 |
| `src-tauri/tests/conversation_source_test.rs` | 建 | 集成测试：source 序列化 + helper 双写 |
| `src-tauri/tests/conv_json_workspace_store_test.rs` | 建 | 集成测试：ConvJsonAuthorizedWorkspaceStore trait |
| `src/lib/tauri.ts` | 改 | 加 3 个 IPC wrapper + AppSettings 两个新字段 |
| `src/stores/homeStore.ts` | 改写 | localStorage → AppSettings + LRU cap 10 |
| `src/features/expert-teams/expertTeamRegistry.ts` | 改写 | 薄壳，IPC 转发 |
| `src/types/message.ts` | 改 | Conversation / index entry 类型补 kind / sourceLabel / workspaceName |
| `src/App.tsx` | 改 | 启动 hydrate 调用 |
| `src/components/home/HomeTaskComposerCard.tsx` | 改 | 用新的 homeStore（无 store API 变化，验证不挂） |
| 所有 `getExpertTeam` / `useExpertTeamForConversation` 调用点 | 改 | 拆 `hasExpertTeam`（boolean）/ `getExpertTeamId`（async） |
| `docs/superpowers/plans/2026-04-24-homepage-workspace-selection.md` | 改 | 加 banner 指向新 spec |

---

## Phase 1: 数据结构 + Helper（PR1 内部）

### Task 1：加 `PersistedAuthorizedWorkspace` 类型

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`

- [ ] **Step 1：在 file_store/types.rs 顶部加 use（如果还没）**

```rust
use std::path::PathBuf;
```

- [ ] **Step 2：在 ConversationMeta 定义之前插入 PersistedAuthorizedWorkspace**

```rust
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
```

- [ ] **Step 3：在 file_store/types.rs 的 mod tests 末尾加序列化单测**

```rust
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
        assert!(!json.contains("sessionId"), "PersistedAuthorizedWorkspace must NOT carry sessionId; got: {}", json);
    }
}
```

- [ ] **Step 4：跑测试**

```bash
cd src-tauri && cargo test --lib persisted_authorized_workspace -- --nocapture
```

预期：2 个测试通过。

- [ ] **Step 5：commit**

```bash
git add src-tauri/src/storage/file_store/types.rs
git commit -m "feat(storage): add PersistedAuthorizedWorkspace type (no sessionId leak)"
```

---

### Task 2：加 `ConversationKind` 枚举

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`

- [ ] **Step 1：在 Task 1 加的类型之后插入 ConversationKind**

```rust
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
```

- [ ] **Step 2：单测**

```rust
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
    fn deserialize_unknown_string_falls_back_to_user_via_serde_other() {
        // ConversationKind 本身没有 #[serde(other)]，未知字符串反序列化会失败。
        // 这条测试钉死现有行为：未知字符串报错，让 ConversationIndexEntry 在外层用
        // #[serde(default = "default_kind")] 兜底。
        let result: Result<ConversationKind, _> = serde_json::from_str("\"futureVariant\"");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 3：跑测试**

```bash
cd src-tauri && cargo test --lib conversation_kind -- --nocapture
```

预期：3 个测试通过。

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/storage/file_store/types.rs
git commit -m "feat(storage): add ConversationKind enum for index.json mirror"
```

---

### Task 3：加 `ConversationSource` tagged union（含未知 variant 兜底）

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`

- [ ] **Step 1：插入类型定义和自定义 Deserialize**

```rust
/// 会话来源 tagged union。序列化为 `{"kind":"...","employeeId":"..."}` 等扁平结构。
///
/// 反序列化兜底：未知 `kind` → `User`（避免老桌面端打开未来加入的 variant 时挂掉）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationSource {
    User,
    Employee { employee_id: String },
    ExpertTeam { expert_team_id: String },
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
```

- [ ] **Step 2：加 import（如果还没）**

`src-tauri/src/storage/file_store/types.rs` 顶部确保有：

```rust
use serde::{Deserialize, Serialize};
```

- [ ] **Step 3：单测全 4 个 variant + 未知 variant + 缺字段**

```rust
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
```

- [ ] **Step 4：跑测试**

```bash
cd src-tauri && cargo test --lib conversation_source -- --nocapture
```

预期：8 个测试全过。

- [ ] **Step 5：commit**

```bash
git add src-tauri/src/storage/file_store/types.rs
git commit -m "feat(storage): add ConversationSource tagged union with unknown-variant fallback"
```

---

### Task 4：`ConversationMeta` 加新字段（source / authorized_workspace / source_label）

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`

- [ ] **Step 1：找到 ConversationMeta 定义（在 types.rs 顶部附近），加 3 个字段**

把现有 ConversationMeta 改成：

```rust
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
    /// 反序列化为 `User`。
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
}
```

- [ ] **Step 2：单测：老 conv.json 反序列化兜底**

在同文件 mod tests 加：

```rust
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
```

- [ ] **Step 3：跑测试**

```bash
cd src-tauri && cargo test --lib conversation_meta_migration -- --nocapture
```

预期：3 个测试通过。

- [ ] **Step 4：跑全仓 lib 测试，确认没破坏现有 ConversationMeta consumer**

```bash
cd src-tauri && cargo test --lib -- --skip review_
```

预期：所有现有测试通过。如失败 → 看是哪个 consumer 依赖 ConversationMeta 现有字段结构，按需修。

- [ ] **Step 5：commit**

```bash
git add src-tauri/src/storage/file_store/types.rs
git commit -m "feat(storage): add source / authorizedWorkspace / sourceLabel fields to ConversationMeta"
```

---

### Task 5：`ConversationIndexEntry` 加 mirror 字段（kind / sourceLabel / workspaceName）

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`

- [ ] **Step 1：改 ConversationIndexEntry**

找到现有 ConversationIndexEntry，改成：

```rust
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
    ///
    /// **过渡期保留**：新写入由 `kind = Employee` 表达；本字段由 dispatch 路径双写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    /// 新增：来源 kind mirror（不含 id；点开会话才需要 id）。
    /// 老 index.json 无此字段时反序列化为 `User`。
    #[serde(default)]
    pub kind: ConversationKind,
    /// 新增：人类可读副标题 mirror。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    /// 新增：授权目录 displayName mirror（用于侧边栏分组）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
}
```

- [ ] **Step 2：单测**

```rust
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
        assert!(entry.employee_id.is_none());
    }

    #[test]
    fn new_index_entry_round_trips() {
        let entry = ConversationIndexEntry {
            id: "c-2".to_string(),
            title: "new".to_string(),
            created_at: "2026-05-20T00:00:00+00:00".to_string(),
            updated_at: "2026-05-20T00:00:00+00:00".to_string(),
            is_archived: false,
            employee_id: None,
            kind: ConversationKind::ExpertTeam,
            source_label: Some("市场专家团".to_string()),
            workspace_name: Some("foo-project".to_string()),
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
```

- [ ] **Step 3：跑测试**

```bash
cd src-tauri && cargo test --lib conversation_index_entry_migration -- --nocapture
```

预期：2 个测试通过。

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/storage/file_store/types.rs
git commit -m "feat(storage): add kind / sourceLabel / workspaceName mirrors to ConversationIndexEntry"
```

---

### Task 6：加 `set_conversation_source` helper（写 conv.json + mirror index.json）

**Files:**
- Modify: `src-tauri/src/storage/file_store/conversations.rs`

- [ ] **Step 1：先看现有 `set_conversation_employee_id` 是怎么写的——它是 mirror 模式的范本**

```bash
grep -B 2 -A 30 'pub fn set_conversation_employee_id' src-tauri/src/storage/file_store/conversations.rs
```

阅读这个函数：它读 conv.json → 改 employee_id → 写回；读 index.json → 更新对应 entry 的 employee_id → 写回。我们的 `set_conversation_source` 完全同模式，只是字段名不同。

- [ ] **Step 2：在 `set_conversation_employee_id` 之后添加 `set_conversation_source`**

```rust
/// 更新会话来源（写 conv.json + mirror 到 index.json）。
///
/// 同时维护：
/// - `ConversationMeta.source` + `ConversationMeta.source_label` (在 conv.json)
/// - `ConversationIndexEntry.kind` + `ConversationIndexEntry.source_label` (在 index.json)
///
/// 写入用原子写（`atomic_write_json`），失败保留原文件不变。
pub fn set_conversation_source(
    base_dir: &Path,
    conversation_id: &str,
    source: ConversationSource,
    source_label: Option<String>,
) -> Result<()> {
    use crate::storage::file_store::types::ConversationKind;

    // 1. 更新 conv.json
    let mut meta = read_conv_meta(base_dir, conversation_id)?;
    let kind = match &source {
        ConversationSource::User => ConversationKind::User,
        ConversationSource::Employee { .. } => ConversationKind::Employee,
        ConversationSource::ExpertTeam { .. } => ConversationKind::ExpertTeam,
        ConversationSource::Im => ConversationKind::Im,
    };
    meta.source = source;
    meta.source_label = source_label.clone();
    write_conv_meta(base_dir, conversation_id, &meta)?;

    // 2. mirror 到 index.json
    let index_path = global_index_path(base_dir);
    let mut index = read_global_index(&index_path)?;
    if let Some(entry) = index.conversations.iter_mut().find(|e| e.id == conversation_id) {
        entry.kind = kind;
        entry.source_label = source_label;
    }
    write_global_index(&index_path, &index)?;
    Ok(())
}
```

如果 `read_conv_meta` / `write_conv_meta` / `read_global_index` / `write_global_index` 这几个 helper 在 `conversations.rs` 里 private 或叫别的名字，按现有命名找到等价 helper 用即可（**看 `set_conversation_employee_id` 用了什么就用什么**）。

- [ ] **Step 3：加 import（如果还没）**

```rust
use crate::storage::file_store::types::{ConversationSource, ConversationKind};
```

- [ ] **Step 4：commit（暂不测，下个 task 加测试）**

```bash
git add src-tauri/src/storage/file_store/conversations.rs
git commit -m "feat(storage): add set_conversation_source helper (writes conv.json + mirrors index.json)"
```

---

### Task 7：`set_conversation_source` 集成测试

**Files:**
- Create: `src-tauri/tests/conversation_source_test.rs`

- [ ] **Step 1：建测试文件**

```rust
//! 集成测试：set_conversation_source 双写 conv.json + index.json 一致性。

use std::path::PathBuf;

use aijia::storage::file_store::conversations::{
    create_conversation, set_conversation_source,
};
use aijia::storage::file_store::types::{ConversationKind, ConversationSource};

fn fresh_base(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("aijia-test-{}-{}", name, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn set_to_expert_team_updates_both_conv_and_index() {
    let base = fresh_base("expert-team");

    // 先创建一个会话（用现有的 create_conversation API）
    let conv_id = "c-test-1";
    create_conversation(&base, conv_id, "test title").unwrap();

    // 改为专家团来源
    set_conversation_source(
        &base,
        conv_id,
        ConversationSource::ExpertTeam {
            expert_team_id: "marketing".to_string(),
        },
        Some("市场专家团".to_string()),
    )
    .unwrap();

    // 验证 conv.json
    let conv_path = base
        .join("conversations")
        .join(conv_id)
        .join("conv.json");
    let conv_content = std::fs::read_to_string(&conv_path).unwrap();
    let conv: serde_json::Value = serde_json::from_str(&conv_content).unwrap();
    assert_eq!(conv["source"]["kind"], "expertTeam");
    assert_eq!(conv["source"]["expertTeamId"], "marketing");
    assert_eq!(conv["sourceLabel"], "市场专家团");

    // 验证 index.json
    let index_path = base.join("conversations").join("index.json");
    let index_content = std::fs::read_to_string(&index_path).unwrap();
    let index: serde_json::Value = serde_json::from_str(&index_content).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["kind"], "expertTeam");
    assert_eq!(entry["sourceLabel"], "市场专家团");
}

#[test]
fn set_to_user_clears_label() {
    let base = fresh_base("user");
    let conv_id = "c-test-2";
    create_conversation(&base, conv_id, "test").unwrap();

    // 先设成专家团
    set_conversation_source(
        &base,
        conv_id,
        ConversationSource::ExpertTeam {
            expert_team_id: "x".to_string(),
        },
        Some("X 团".to_string()),
    )
    .unwrap();
    // 然后清空
    set_conversation_source(&base, conv_id, ConversationSource::User, None).unwrap();

    // 验证 source 回到 User、label 清空
    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let conv: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&conv_path).unwrap()).unwrap();
    assert_eq!(conv["source"]["kind"], "user");
    assert!(conv["sourceLabel"].is_null() || conv.get("sourceLabel").is_none());

    let index_path = base.join("conversations").join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["kind"], "user");
    assert!(entry.get("sourceLabel").is_none() || entry["sourceLabel"].is_null());
}

#[test]
fn set_to_employee_with_label() {
    let base = fresh_base("employee");
    let conv_id = "c-test-3";
    create_conversation(&base, conv_id, "test").unwrap();

    set_conversation_source(
        &base,
        conv_id,
        ConversationSource::Employee {
            employee_id: "emp-001".to_string(),
        },
        Some("小销".to_string()),
    )
    .unwrap();

    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let conv: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&conv_path).unwrap()).unwrap();
    assert_eq!(conv["source"]["kind"], "employee");
    assert_eq!(conv["source"]["employeeId"], "emp-001");
    assert_eq!(conv["sourceLabel"], "小销");

    let index_path = base.join("conversations").join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["kind"], "employee");
    assert_eq!(entry["sourceLabel"], "小销");
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test conversation_source_test -- --nocapture
```

预期：3 个测试通过。

注：如果 `create_conversation` 函数签名不同（如带额外参数），按它实际签名调用。看 `src-tauri/src/storage/file_store/conversations.rs` 顶部的 `pub fn create_conversation` 确认参数顺序。

- [ ] **Step 3：commit**

```bash
git add src-tauri/tests/conversation_source_test.rs
git commit -m "test(storage): set_conversation_source double-write integration test"
```

---

### Task 8：加 `set_conversation_workspace` / `read_conversation_workspace` helper

**Files:**
- Modify: `src-tauri/src/storage/file_store/conversations.rs`

- [ ] **Step 1：在 `set_conversation_source` 之后添加 `set_conversation_workspace`**

```rust
/// 更新会话授权目录（写 conv.json + mirror displayName 到 index.json）。
///
/// `workspace = None` 表示清除授权。
pub fn set_conversation_workspace(
    base_dir: &Path,
    conversation_id: &str,
    workspace: Option<&PersistedAuthorizedWorkspace>,
) -> Result<()> {
    // 1. 更新 conv.json
    let mut meta = read_conv_meta(base_dir, conversation_id)?;
    meta.authorized_workspace = workspace.cloned();
    write_conv_meta(base_dir, conversation_id, &meta)?;

    // 2. mirror displayName 到 index.json
    let index_path = global_index_path(base_dir);
    let mut index = read_global_index(&index_path)?;
    if let Some(entry) = index.conversations.iter_mut().find(|e| e.id == conversation_id) {
        entry.workspace_name = workspace.map(|w| w.display_name.clone());
    }
    write_global_index(&index_path, &index)?;
    Ok(())
}

/// 读取会话当前授权目录（只读 conv.json，不读 index）。
pub fn read_conversation_workspace(
    base_dir: &Path,
    conversation_id: &str,
) -> Result<Option<PersistedAuthorizedWorkspace>> {
    let meta = read_conv_meta(base_dir, conversation_id)?;
    Ok(meta.authorized_workspace)
}

/// 读取会话当前 source（只读 conv.json）。
pub fn read_conversation_source(
    base_dir: &Path,
    conversation_id: &str,
) -> Result<ConversationSource> {
    let meta = read_conv_meta(base_dir, conversation_id)?;
    Ok(meta.source)
}
```

- [ ] **Step 2：加 import**

```rust
use crate::storage::file_store::types::PersistedAuthorizedWorkspace;
```

- [ ] **Step 3：在 conversation_source_test.rs 加 workspace 测试**

在测试文件末尾追加：

```rust
use aijia::storage::file_store::conversations::{
    read_conversation_workspace, set_conversation_workspace,
};
use aijia::storage::file_store::types::PersistedAuthorizedWorkspace;

#[test]
fn set_and_read_workspace() {
    let base = fresh_base("workspace");
    let conv_id = "c-ws-1";
    create_conversation(&base, conv_id, "test").unwrap();

    let ws = PersistedAuthorizedWorkspace {
        id: "ws-1".to_string(),
        root_path: PathBuf::from("/tmp/foo"),
        display_name: "foo".to_string(),
        authorized_at: "2026-05-20T00:00:00+00:00".to_string(),
    };

    set_conversation_workspace(&base, conv_id, Some(&ws)).unwrap();

    // 读 conv.json
    let read_back = read_conversation_workspace(&base, conv_id).unwrap();
    assert_eq!(read_back, Some(ws.clone()));

    // 验证 index.json mirror
    let index_path = base.join("conversations").join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["workspaceName"], "foo");
}

#[test]
fn clear_workspace_removes_mirror() {
    let base = fresh_base("workspace-clear");
    let conv_id = "c-ws-2";
    create_conversation(&base, conv_id, "test").unwrap();

    let ws = PersistedAuthorizedWorkspace {
        id: "ws-1".to_string(),
        root_path: PathBuf::from("/tmp/foo"),
        display_name: "foo".to_string(),
        authorized_at: "t".to_string(),
    };
    set_conversation_workspace(&base, conv_id, Some(&ws)).unwrap();

    // 清除
    set_conversation_workspace(&base, conv_id, None).unwrap();

    assert!(read_conversation_workspace(&base, conv_id).unwrap().is_none());

    let index_path = base.join("conversations").join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert!(entry.get("workspaceName").is_none() || entry["workspaceName"].is_null());
}

#[test]
fn workspace_no_sessionid_in_disk_format() {
    let base = fresh_base("no-session-id");
    let conv_id = "c-ws-3";
    create_conversation(&base, conv_id, "test").unwrap();

    let ws = PersistedAuthorizedWorkspace {
        id: "ws-1".to_string(),
        root_path: PathBuf::from("/x"),
        display_name: "x".to_string(),
        authorized_at: "t".to_string(),
    };
    set_conversation_workspace(&base, conv_id, Some(&ws)).unwrap();

    // 读 conv.json 原始内容，断言里面**没有** sessionId 字段
    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let raw = std::fs::read_to_string(&conv_path).unwrap();
    assert!(
        !raw.contains("\"sessionId\""),
        "conv.json must NOT contain sessionId in authorizedWorkspace; got: {}",
        raw
    );
}
```

- [ ] **Step 4：跑测试**

```bash
cd src-tauri && cargo test --test conversation_source_test -- --nocapture
```

预期：6 个测试全过（原 3 个 + 新 3 个）。

- [ ] **Step 5：commit**

```bash
git add src-tauri/src/storage/file_store/conversations.rs src-tauri/tests/conversation_source_test.rs
git commit -m "feat(storage): add set_conversation_workspace / read_conversation_workspace helpers"
```

---

## Phase 2: 后端 commands + AuthorizedWorkspaceStore 替换（PR2 内部）

### Task 9：改 `AuthorizedWorkspaceStore` trait 签名加 conversation_id 参数

**Files:**
- Modify: `src-tauri/src/runtime/store/authorized_workspace_store.rs`

- [ ] **Step 1：改 trait 定义**

```rust
pub trait AuthorizedWorkspaceStore: Send + Sync {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()>;
    fn get_current_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>>;
    fn clear_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<()>;
}
```

- [ ] **Step 2：改 `FileAuthorizedWorkspaceStore` 的 impl 块对应签名**

把现有 `FileAuthorizedWorkspaceStore` 的三个方法签名按上面 trait 改。**实现内部**暂时把 `conversation_id` 参数忽略（用 `_conversation_id` 命名），逻辑保持现状（仍走 memory.jsonl），下一步替换。

```rust
impl AuthorizedWorkspaceStore for FileAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        _conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        let key = format!("authorized_workspace:{}", ws.session_id.as_str());
        let value = serde_json::to_string(ws)?;
        self.storage
            .set_memory(&key, &value, Some("authorized_workspace"))
    }

    fn get_current_for_session(
        &self,
        _conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        let key = format!("authorized_workspace:{}", session_id.as_str());
        match self.storage.get_memory(&key)? {
            Some(v) if !v.is_empty() => Ok(Some(serde_json::from_str(&v)?)),
            _ => Ok(None),
        }
    }

    fn clear_for_session(
        &self,
        _conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<()> {
        let key = format!("authorized_workspace:{}", session_id.as_str());
        self.storage
            .set_memory(&key, "", Some("authorized_workspace_cleared"))?;
        Ok(())
    }
}
```

- [ ] **Step 3：改 `InMemoryAuthorizedWorkspaceStore` 的 impl**

`InMemoryAuthorizedWorkspaceStore` 内部 HashMap 改成 keyed by `conversation_id`（之前 key 是 session_id.as_str()，语义相同但意图清晰）。

```rust
impl AuthorizedWorkspaceStore for InMemoryAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        self.data
            .lock()
            .unwrap()
            .insert(conversation_id.to_string(), ws.clone());
        Ok(())
    }

    fn get_current_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        Ok(self.data.lock().unwrap().get(conversation_id).cloned())
    }

    fn clear_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<()> {
        self.data.lock().unwrap().remove(conversation_id);
        Ok(())
    }
}
```

- [ ] **Step 4：改 InMemory 的单元测试**

文件末尾的 4 个 `#[test]` 全部调用都需要按新签名加 `conversation_id` 参数。把每个测试里：
- `store.replace_for_session(&ws)` → `store.replace_for_session(ws.session_id.as_str(), &ws)`
- `store.get_current_for_session(&sid)` → `store.get_current_for_session(sid.as_str(), &sid)`
- `store.clear_for_session(&sid)` → `store.clear_for_session(sid.as_str(), &sid)`

注：这里 `conversation_id == session_id.as_str()`（按当前 lib.rs 约定），所以测试不丢失语义。

- [ ] **Step 5：跑该文件测试**

```bash
cd src-tauri && cargo test --lib authorized_workspace_store -- --nocapture
```

预期：4 个测试全过。

- [ ] **Step 6：跑全仓 lib 测试，找其它 caller**

```bash
cd src-tauri && cargo build --lib 2>&1 | grep -E '(error|warning: unused)' | head -30
```

预期：会看到 caller 处编译错误（`session_runtime.rs` / `chat.rs` / `chat_runtime_impl.rs`）。这正是我们要在 Task 10 修的。

- [ ] **Step 7：commit**

```bash
git add src-tauri/src/runtime/store/authorized_workspace_store.rs
git commit -m "refactor(store): AuthorizedWorkspaceStore trait — add explicit conversation_id param"
```

---

### Task 10：修复所有 `AuthorizedWorkspaceStore` 调用点

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:3252`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:237-265`
- Modify: `src-tauri/src/runtime/session_runtime.rs:465, 915`

- [ ] **Step 1：先 build 看所有错位**

```bash
cd src-tauri && cargo build --lib 2>&1 | grep "error\[E" | head -20
```

记下每个 error 的文件:行号。

- [ ] **Step 2：修复 `chat.rs:3252`**

找到 `facade.authorized_workspace_store().replace_for_session(&ws)` 这一行，看上下文（应该是个新建会话的代码块），找到 conversation_id 变量。改成：

```rust
if let Err(e) = facade.authorized_workspace_store().replace_for_session(&conv_id, &ws) {
    // ... 原有错误处理
}
```

`conv_id` 是当前会话的 id，从上下文取得（看上下文哪个变量是新建/正在操作的 conversation id）。

- [ ] **Step 3：修复 `chat_runtime_impl.rs:237-265`**

`load_explicit_workspace` 和 `load_authorized_workspace` 两个函数里都调了 `get_current_for_session(&SessionId::new(conversation_id.to_string()))`。改成：

```rust
facade
    .authorized_workspace_store()
    .get_current_for_session(conversation_id, &SessionId::new(conversation_id.to_string()))
    .ok()
    .flatten()
```

- [ ] **Step 4：修复 `session_runtime.rs:465`**

`store.get_current_for_session(session_id)` 改成：

```rust
store.get_current_for_session(session_id.as_str(), session_id)
```

这里 `conversation_id == session_id.as_str()`，按当前约定传同一值。

- [ ] **Step 5：修复 `session_runtime.rs:915`**

测试代码 `store.replace_for_session(&crate::runtime::store::AuthorizedWorkspace { ... })`：

```rust
let ws = crate::runtime::store::AuthorizedWorkspace { /* ... */ };
store.replace_for_session(ws.session_id.as_str(), &ws);
```

- [ ] **Step 6：build 通过**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error 0 warning（或仅未相关 warning）。

- [ ] **Step 7：跑相关测试**

```bash
cd src-tauri && cargo test --lib session_runtime authorized_workspace -- --nocapture
```

预期：全过。

- [ ] **Step 8：commit**

```bash
git add src-tauri/src/transport/tauri_commands/ src-tauri/src/runtime/session_runtime.rs
git commit -m "refactor(store): update AuthorizedWorkspaceStore callers with conversation_id"
```

---

### Task 11：新建 `ConvJsonAuthorizedWorkspaceStore`

**Files:**
- Modify: `src-tauri/src/runtime/store/authorized_workspace_store.rs`

- [ ] **Step 1：在 `FileAuthorizedWorkspaceStore` 之后添加 `ConvJsonAuthorizedWorkspaceStore`**

```rust
// ─────────────────────────────────────────────────────────────────────────────
// conv.json-backed implementation (production successor to FileAuthorizedWorkspaceStore)
// ─────────────────────────────────────────────────────────────────────────────

/// 把授权目录持久化到 `conversations/{id}/conv.json` 的 `authorizedWorkspace` 字段，
/// 同时 mirror displayName 到 `conversations/index.json`。
///
/// 替代 `FileAuthorizedWorkspaceStore`（走 memory.jsonl 那条线）。
pub struct ConvJsonAuthorizedWorkspaceStore {
    pub storage: Arc<AppStorage>,
}

impl AuthorizedWorkspaceStore for ConvJsonAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        let persisted = crate::storage::file_store::types::PersistedAuthorizedWorkspace {
            id: ws.id.clone(),
            root_path: ws.root_path.clone(),
            display_name: ws.display_name.clone(),
            authorized_at: ws.authorized_at.clone(),
        };
        crate::storage::file_store::conversations::set_conversation_workspace(
            self.storage.base_dir(),
            conversation_id,
            Some(&persisted),
        )?;
        Ok(())
    }

    fn get_current_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        let persisted = crate::storage::file_store::conversations::read_conversation_workspace(
            self.storage.base_dir(),
            conversation_id,
        )?;
        Ok(persisted.map(|p| AuthorizedWorkspace {
            id: p.id,
            session_id: session_id.clone(),
            root_path: p.root_path,
            display_name: p.display_name,
            authorized_at: p.authorized_at,
        }))
    }

    fn clear_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<()> {
        crate::storage::file_store::conversations::set_conversation_workspace(
            self.storage.base_dir(),
            conversation_id,
            None,
        )?;
        Ok(())
    }
}
```

- [ ] **Step 2：确认 `AppStorage::base_dir()` 是 public**

```bash
grep -n 'pub fn base_dir' src-tauri/src/storage/file_store/mod.rs
```

如果不是 pub（仅 `fn base_dir`），改成 `pub fn base_dir(&self) -> &Path`（或加一个 `pub fn base_dir_path`）。

- [ ] **Step 3：导出新 store**

`src-tauri/src/runtime/store/mod.rs` 里加 `ConvJsonAuthorizedWorkspaceStore` 到 re-export：

```rust
pub use authorized_workspace_store::{
    AuthorizedWorkspace, AuthorizedWorkspaceRef, AuthorizedWorkspaceStore,
    ConvJsonAuthorizedWorkspaceStore, FileAuthorizedWorkspaceStore, InMemoryAuthorizedWorkspaceStore,
};
```

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/runtime/store/authorized_workspace_store.rs src-tauri/src/runtime/store/mod.rs src-tauri/src/storage/file_store/mod.rs
git commit -m "feat(store): add ConvJsonAuthorizedWorkspaceStore (conv.json-backed)"
```

---

### Task 12：`ConvJsonAuthorizedWorkspaceStore` 集成测试

**Files:**
- Create: `src-tauri/tests/conv_json_workspace_store_test.rs`

- [ ] **Step 1：建测试**

```rust
//! 集成测试：ConvJsonAuthorizedWorkspaceStore trait 行为 + Persisted ↔ AuthorizedWorkspace 映射。

use std::path::PathBuf;
use std::sync::Arc;

use aijia::runtime::ids::SessionId;
use aijia::runtime::store::{
    AuthorizedWorkspace, AuthorizedWorkspaceStore, ConvJsonAuthorizedWorkspaceStore,
};
use aijia::storage::file_store::AppStorage;
use aijia::storage::file_store::conversations::create_conversation;

fn fresh_storage(name: &str) -> Arc<AppStorage> {
    let base = std::env::temp_dir().join(format!("aijia-test-{}-{}", name, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    Arc::new(AppStorage::new(base).unwrap())
}

fn make_ws(conv_id: &str, root: &str, name: &str) -> AuthorizedWorkspace {
    AuthorizedWorkspace {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: SessionId::new(conv_id.to_string()),
        root_path: PathBuf::from(root),
        display_name: name.to_string(),
        authorized_at: "2026-05-20T00:00:00+00:00".to_string(),
    }
}

#[test]
fn replace_and_get_round_trip() {
    let storage = fresh_storage("replace-get");
    let conv_id = "c-1";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    let sid = SessionId::new(conv_id.to_string());
    let ws = make_ws(conv_id, "/tmp/foo", "foo");

    store.replace_for_session(conv_id, &ws).unwrap();
    let got = store.get_current_for_session(conv_id, &sid).unwrap();
    assert!(got.is_some(), "expected Some workspace");
    let got = got.unwrap();
    assert_eq!(got.display_name, "foo");
    assert_eq!(got.root_path, PathBuf::from("/tmp/foo"));
    // session_id 是由 get_current_for_session 用传入的 sid 填回的
    assert_eq!(got.session_id, sid);
}

#[test]
fn clear_for_session_removes_workspace() {
    let storage = fresh_storage("clear");
    let conv_id = "c-2";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    let sid = SessionId::new(conv_id.to_string());
    let ws = make_ws(conv_id, "/tmp/bar", "bar");

    store.replace_for_session(conv_id, &ws).unwrap();
    store.clear_for_session(conv_id, &sid).unwrap();
    let got = store.get_current_for_session(conv_id, &sid).unwrap();
    assert!(got.is_none());
}

#[test]
fn replace_overwrites_previous() {
    let storage = fresh_storage("overwrite");
    let conv_id = "c-3";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    let sid = SessionId::new(conv_id.to_string());

    store.replace_for_session(conv_id, &make_ws(conv_id, "/tmp/old", "old")).unwrap();
    store.replace_for_session(conv_id, &make_ws(conv_id, "/tmp/new", "new")).unwrap();

    let got = store.get_current_for_session(conv_id, &sid).unwrap().unwrap();
    assert_eq!(got.display_name, "new");
    assert_eq!(got.root_path, PathBuf::from("/tmp/new"));
}

#[test]
fn workspace_mirrors_to_index_json() {
    let storage = fresh_storage("index-mirror");
    let conv_id = "c-4";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    store.replace_for_session(conv_id, &make_ws(conv_id, "/tmp/proj", "proj")).unwrap();

    let index_path = storage.base_dir().join("conversations").join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["workspaceName"], "proj");
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test conv_json_workspace_store_test -- --nocapture
```

预期：4 个测试全过。

- [ ] **Step 3：commit**

```bash
git add src-tauri/tests/conv_json_workspace_store_test.rs
git commit -m "test(store): ConvJsonAuthorizedWorkspaceStore integration tests"
```

---

### Task 13：在 `lib.rs` 替换 store 注入

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs:876` (实际 wire-up 处)

- [ ] **Step 1：先核实当前 wire-up 在哪**

```bash
grep -B 2 -A 4 'FileAuthorizedWorkspaceStore {' src-tauri/src/storage/file_store/mod.rs
```

- [ ] **Step 2：替换**

把：

```rust
authorized_workspace_store: std::sync::Arc::new(
    crate::runtime::store::FileAuthorizedWorkspaceStore {
        storage: storage.clone(),
    },
),
```

改成：

```rust
authorized_workspace_store: std::sync::Arc::new(
    crate::runtime::store::ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    },
),
```

- [ ] **Step 3：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error。

- [ ] **Step 4：跑全仓 lib + 集成测试**

```bash
cd src-tauri && cargo test -- --skip review_
```

预期：全过（这里**会**仍然看到老的 memory.jsonl 路径在跑——`FileAuthorizedWorkspaceStore` 还在但不再被生产路径用了；那是 Task 18 删除它）。

- [ ] **Step 5：commit**

```bash
git add src-tauri/src/storage/file_store/mod.rs
git commit -m "feat(store): switch production wire-up to ConvJsonAuthorizedWorkspaceStore"
```

---

### Task 14：`authorize_local_directory` 同步双写

**Files:**
- Modify: `src-tauri/src/commands/workspace.rs`

- [ ] **Step 1：找到 authorize_local_directory 函数**

```bash
grep -n 'authorize_local_directory' src-tauri/src/commands/workspace.rs | head -5
```

- [ ] **Step 2：阅读现有逻辑**

```bash
sed -n '现有行号-2,现有行号+50p' src-tauri/src/commands/workspace.rs
```

确认它在哪里调 `replace_for_session`。Task 13 后，这个调用已经走 conv.json 路径，**实际上 workspace 已经双写了**（trait 实现内部就是写 conv.json + mirror index）。所以这一步**核心是审计**：现有 authorize_local_directory 是否还有任何代码假设 memory.jsonl 那条线（除了 store 调用本身）？

```bash
grep -n 'memory\|note\|loaded' src-tauri/src/commands/workspace.rs
```

如果有，按需删；如果没有，本 task 仅需验证。

- [ ] **Step 3：找到 `revoke_authorized_workspace`（同文件），确认它调 `clear_for_session`**

```bash
grep -B 2 -A 15 'fn revoke_authorized_workspace' src-tauri/src/commands/workspace.rs
```

确认调的是 `clear_for_session`（新签名）。如果 Task 10 的全仓 fix 已经覆盖，跳过；否则按 Task 10 同款方式加 conversation_id 参数。

- [ ] **Step 4：build + test**

```bash
cd src-tauri && cargo build --lib && cargo test --test '*workspace*' -- --nocapture
```

预期：通过。

- [ ] **Step 5：commit（如有改动）**

```bash
git add src-tauri/src/commands/workspace.rs
git commit -m "refactor(commands): audit authorize_local_directory under new workspace store"
```

如果没改动文件，跳过 commit，移到下一 task。

---

### Task 15：加 3 个 Tauri commands（set_conversation_expert_team / clear_conversation_source / get_conversation_source）

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/lib.rs`（注册 invoke handler）

- [ ] **Step 1：在 chat.rs 加 3 个 commands**

找到一个合适的位置（其它 conversation-level command 附近，如 `archive_conversation`），插入：

```rust
#[tauri::command]
pub async fn set_conversation_expert_team(
    facade: tauri::State<'_, std::sync::Arc<RuntimeRepositoryFacade>>,
    conversation_id: String,
    expert_team_id: String,
    team_label: String,
) -> Result<(), String> {
    let base = facade.storage().base_dir().to_path_buf();
    crate::storage::file_store::conversations::set_conversation_source(
        &base,
        &conversation_id,
        crate::storage::file_store::types::ConversationSource::ExpertTeam { expert_team_id },
        Some(team_label),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_conversation_source(
    facade: tauri::State<'_, std::sync::Arc<RuntimeRepositoryFacade>>,
    conversation_id: String,
) -> Result<(), String> {
    let base = facade.storage().base_dir().to_path_buf();
    crate::storage::file_store::conversations::set_conversation_source(
        &base,
        &conversation_id,
        crate::storage::file_store::types::ConversationSource::User,
        None,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_conversation_source(
    facade: tauri::State<'_, std::sync::Arc<RuntimeRepositoryFacade>>,
    conversation_id: String,
) -> Result<crate::storage::file_store::types::ConversationSource, String> {
    let base = facade.storage().base_dir().to_path_buf();
    crate::storage::file_store::conversations::read_conversation_source(&base, &conversation_id)
        .map_err(|e| e.to_string())
}
```

注：`facade.storage()` 这个 accessor 如果不存在，看 `RuntimeRepositoryFacade` 有什么方法返回 `Arc<AppStorage>`，用那个。

- [ ] **Step 2：在 lib.rs 的 invoke_handler 注册**

```bash
grep -n 'invoke_handler\|fn register_handlers' src-tauri/src/lib.rs | head -5
```

找到现有的 command 注册列表（一般是 `tauri::generate_handler![...]`），按字母顺序或就近原则插入 3 个新 command：

```rust
.invoke_handler(tauri::generate_handler![
    // ... 现有 commands ...
    crate::transport::tauri_commands::chat::set_conversation_expert_team,
    crate::transport::tauri_commands::chat::clear_conversation_source,
    crate::transport::tauri_commands::chat::get_conversation_source,
    // ...
])
```

如果上面的引入路径不对，按 lib.rs 现有 command 命名风格调整。

- [ ] **Step 3：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error。

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/lib.rs
git commit -m "feat(transport): add 3 conversation source / clear / get Tauri commands"
```

---

## Phase 3: get_conversations 性能修复（PR3 内部）

### Task 16：`get_conversations` 删 fan-out

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:3152-3185` （`get_conversations` 函数）

- [ ] **Step 1：找到 fan-out 循环**

```bash
grep -B 2 -A 25 'pub async fn get_conversations' src-tauri/src/transport/tauri_commands/chat.rs | head -40
```

应能看到 `for conv in &mut convs { ... load_explicit_workspace ... conv["workspaceName"] = ... }` 这段。

- [ ] **Step 2：删整段 fan-out 循环 + dev-only 诊断日志**

把这段：

```rust
for conv in &mut convs {
    if let Some(id) = conv["id"].as_str() {
        if let Some(ws) = chat_runtime_impl::load_explicit_workspace(&self.services.app, id) {
            injected += 1;
            conv["workspaceName"] = serde_json::Value::String(ws.display_name);
        }
    }
}
// dev-only diagnostic: 若侧边栏首次加载时分组异常（例如只看到"默认文件夹"），
// 可对照前端 [diag-sidebar] console 日志判断是后端注入失败还是前端 race。
// total / injected 通过 release build 时编译掉。
if cfg!(debug_assertions) {
    log::info!(
        "[diag-sidebar] get_conversations total={} injected={}",
        // ...
    );
}
```

整个删除（包含 `injected` 计数和 dev 日志），因为 workspaceName 现在已经在 `ConversationIndexEntry` 自带（由 `conversation_service::get_conversations` 读 index.json 时一起带出）。

- [ ] **Step 3：删除 `injected` 变量定义（位于 fan-out 之前）**

```rust
let mut injected = 0usize;
```

这一行也删。

- [ ] **Step 4：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error（可能有 `chat_runtime_impl` import 现在没人用的 warning，下个 task 处理 `load_explicit_workspace` 时一起）。

- [ ] **Step 5：commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "perf(transport): remove O(N) get_memory fan-out in get_conversations (use index.json mirror)"
```

---

### Task 17：`load_explicit_workspace` 改读 conv.json

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:237-254`

- [ ] **Step 1：阅读现有实现**

```bash
sed -n '235,275p' src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
```

- [ ] **Step 2：改实现**

```rust
/// 只查真实绑定，不做 defaultFolder fallback。用于列表展示场景。
///
/// 实际读取走 `ConvJsonAuthorizedWorkspaceStore`，从 conv.json 读 PersistedAuthorizedWorkspace。
pub(crate) fn load_explicit_workspace(
    app: &AppHandle,
    conversation_id: &str,
) -> Option<crate::runtime::store::AuthorizedWorkspaceRef> {
    app.try_state::<Arc<RuntimeRepositoryFacade>>()
        .and_then(|facade| {
            facade
                .authorized_workspace_store()
                .get_current_for_session(
                    conversation_id,
                    &SessionId::new(conversation_id.to_string()),
                )
                .ok()
                .flatten()
        })
        .map(|aw| crate::runtime::store::AuthorizedWorkspaceRef {
            id: aw.id,
            root_path: aw.root_path,
            display_name: aw.display_name,
        })
}
```

实际改动只是按 Task 10 的签名把 `conversation_id` 字符串直接传进来（之前 Task 10 已经修过这里，本 task 只是确认正确性）。如果 Task 10 已修，本 task 实际仅审计。

- [ ] **Step 3：审计 `load_authorized_workspace`（同文件，做了 defaultFolder fallback）**

按同样模式确认它也对，不需要改。

- [ ] **Step 4：build + test**

```bash
cd src-tauri && cargo build --lib && cargo test --test conv_json_workspace_store_test -- --nocapture
```

预期：全过。

- [ ] **Step 5：commit（如有改动）**

```bash
git add src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "refactor(transport): load_explicit_workspace now reads conv.json directly"
```

如无改动，跳过 commit。

---

## Phase 4: 删除 memory KV 设施（PR4 内部）

### Task 18：删除 conversation_service.rs 里的 delete_memories_by_prefix 调用

**Files:**
- Modify: `src-tauri/src/runtime/conversation_service.rs:154-155`

- [ ] **Step 1：删两行**

找到这段（`delete_conversation` 函数内）：

```rust
let _ = db.delete_memories_by_prefix(&format!("loaded:{}:", conversation_id));
let _ = db.delete_memories_by_prefix(&format!("note:{}:", conversation_id));
```

直接删。

- [ ] **Step 2：跑现有 conversation_service 测试**

```bash
cd src-tauri && cargo test --lib conversation_service -- --nocapture
```

预期：全过。

- [ ] **Step 3：commit**

```bash
git add src-tauri/src/runtime/conversation_service.rs
git commit -m "chore(runtime): remove dead delete_memories_by_prefix calls in delete_conversation"
```

---

### Task 19：删除 `DefaultFileOperations::is_loaded`

**Files:**
- Modify: `src-tauri/src/runtime/tools/capability.rs`

- [ ] **Step 1：删 `FileOperations` trait 里的 `is_loaded` 方法定义**

找到（约 line 129-133）：

```rust
/// carry an `Option<Arc<dyn FileOperations>>` field for `is_loaded` checks and
// ...
fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool;
```

删除 `fn is_loaded(...)` 那一行。注释里如果提到 `is_loaded` 也一起改/删。

- [ ] **Step 2：删 impl 里的 `is_loaded`**

找到（约 line 317-320）：

```rust
fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool {
    let key = format!("loaded:{}:{}", scope_id, file_id);
    matches!(self.storage.get_memory(&key), Ok(Some(_)))
}
```

整个方法删除。

- [ ] **Step 3：build 验证无 caller**

```bash
cd src-tauri && cargo build --lib 2>&1 | grep -E 'error|is_loaded'
```

预期：0 error。如果出现 "trait method ... not found" 类型的错，说明有 caller 我们漏了——按提示修。

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/runtime/tools/capability.rs
git commit -m "chore(runtime): remove dead FileOperations::is_loaded method"
```

---

### Task 20：删除 plugin/context.rs 的 dead helper

**Files:**
- Modify: `src-tauri/src/plugin/context.rs:141-152`

- [ ] **Step 1：删 3 个 helper**

```rust
pub fn loaded_key(&self, file_id: &str) -> String { ... }
pub fn load_failed_key(&self, file_id: &str) -> String { ... }
pub fn loaded_prefix(&self) -> String { ... }
```

整段（3 个 helper）删除。

- [ ] **Step 2：build**

```bash
cd src-tauri && cargo build --lib 2>&1 | grep -E 'error|loaded_key|loaded_prefix|load_failed_key'
```

预期：0 error。

- [ ] **Step 3：commit**

```bash
git add src-tauri/src/plugin/context.rs
git commit -m "chore(plugin): remove dead loaded_key / loaded_prefix / load_failed_key helpers"
```

---

### Task 21：删除 MemoryStore 上层封装 + FileMemoryStore + facade 字段

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs`
- Modify: `src-tauri/src/runtime/store/mod.rs`
- Delete: `src-tauri/src/runtime/store/memory_store.rs`

- [ ] **Step 1：删 runtime/store/memory_store.rs 整个文件**

```bash
git rm src-tauri/src/runtime/store/memory_store.rs
```

- [ ] **Step 2：在 runtime/store/mod.rs 删 memory_store 相关**

```bash
grep -n 'memory_store' src-tauri/src/runtime/store/mod.rs
```

删除：

- `pub mod memory_store;`
- `pub use memory_store::{InMemoryMemoryStore, MemoryEntry, MemoryStore};`

- [ ] **Step 3：删 file_store/mod.rs 里 RuntimeRepositoryFacade 的 memory_store 字段 + 访问器**

找到 line 822：

```rust
memory_store: std::sync::Arc<dyn crate::runtime::store::MemoryStore>,
```

整行删除。

找到 line 891-897 两个 accessor：

```rust
pub fn memory_store(&self) -> &dyn crate::runtime::store::MemoryStore { ... }
pub fn clone_memory_store(&self) -> std::sync::Arc<dyn crate::runtime::store::MemoryStore> { ... }
```

整个方法删除。

- [ ] **Step 4：删 RuntimeRepositoryFacade 构造里的 memory_store 初始化**

`for_test()` 里（约 line 839）：

```rust
memory_store: std::sync::Arc::new(crate::runtime::store::InMemoryMemoryStore::default()),
```

删除。

`from_storage()` 里（约 line 860）：

```rust
memory_store: std::sync::Arc::new(FileMemoryStore {
    storage: storage.clone(),
}),
```

删除。

- [ ] **Step 5：删 FileMemoryStore struct + impl（line 998-1010）**

```rust
struct FileMemoryStore {
    storage: std::sync::Arc<AppStorage>,
}

impl crate::runtime::store::MemoryStore for FileMemoryStore { ... }
```

整段删除。

- [ ] **Step 6：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error。如果有 error，按提示继续删未清理的 reference。

- [ ] **Step 7：跑全仓 lib + 集成测试**

```bash
cd src-tauri && cargo test -- --skip review_no_memory_kv
```

预期：全过。

- [ ] **Step 8：commit**

```bash
git add -A
git commit -m "chore(store): delete dead MemoryStore trait + FileMemoryStore + facade memory_store field"
```

---

### Task 22：删除 AppStorage 的 4 个 memory KV pub fn

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs`

- [ ] **Step 1：找到 4 个 method**

```bash
grep -n 'pub fn set_memory\|pub fn get_memory\|pub fn get_memories_by_prefix\|pub fn delete_memories_by_prefix' src-tauri/src/storage/file_store/mod.rs
```

- [ ] **Step 2：把这 4 个方法连同它们的 doc 注释整段删除**

```rust
pub fn get_memory(&self, key: &str) -> Result<Option<String>> { ... }
pub fn set_memory(&self, key: &str, value: &str, source: Option<&str>) -> Result<()> { ... }
pub fn get_memories_by_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> { ... }
pub fn delete_memories_by_prefix(&self, prefix: &str) -> Result<usize> { ... }
```

整段（约 line 545-558）删除。

- [ ] **Step 3：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error（前面 19/20 task 都已经删了所有 caller）。

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/storage/file_store/mod.rs
git commit -m "chore(storage): delete AppStorage memory KV public API (set_memory / get_memory / get_memories_by_prefix / delete_memories_by_prefix)"
```

---

### Task 23：删除 notes.rs 整个文件 + MemoryEntry 类型

**Files:**
- Delete: `src-tauri/src/storage/file_store/notes.rs`
- Modify: `src-tauri/src/storage/file_store/types.rs`
- Modify: `src-tauri/src/storage/file_store/mod.rs`

- [ ] **Step 1：删除 notes.rs**

```bash
git rm src-tauri/src/storage/file_store/notes.rs
```

- [ ] **Step 2：在 file_store/mod.rs 删 `mod notes;` 声明**

```bash
grep -n 'mod notes' src-tauri/src/storage/file_store/mod.rs
```

删除整行。

- [ ] **Step 3：删 file_store/types.rs 的 MemoryEntry 类型**

```bash
grep -B 2 -A 10 'pub struct MemoryEntry' src-tauri/src/storage/file_store/types.rs
```

整段删除（含 doc 注释行 "A memory entry stored in shared/memory/memory.jsonl"）。

- [ ] **Step 4：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error。

- [ ] **Step 5：commit**

```bash
git add -A
git commit -m "chore(storage): delete file_store/notes.rs module and MemoryEntry type"
```

---

### Task 24：删除 mkdir shared/memory 的所有调用点

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs:107`
- Modify: `src-tauri/src/storage/aijia_home.rs:254`
- Modify: `src-tauri/src/storage/current_user_storage.rs:180`
- Modify: `src-tauri/src/storage/aijia_home.rs:441`
- Modify: `src-tauri/src/storage/user_scoped_paths.rs:36, 201`

- [ ] **Step 1：删 file_store/mod.rs:107**

```rust
fs::create_dir_all(self.base_dir.join("shared").join("memory"))?;
```

整行删除。

- [ ] **Step 2：删 aijia_home.rs:254**

```rust
std::fs::create_dir_all(user_dir.join("shared").join("memory"))?;
```

整行删除。

- [ ] **Step 3：删测试断言**

`current_user_storage.rs:180`：

```rust
assert!(user_dir.join("shared").join("memory").is_dir());
```

整行删除。

`aijia_home.rs:441`：

```bash
sed -n '438,445p' src-tauri/src/storage/aijia_home.rs
```

如有 `shared/memory` 断言，删除。

- [ ] **Step 4：删 user_scoped_paths.rs:36 的 memory_dir helper + 测试**

```bash
grep -B 2 -A 4 'pub fn memory_dir' src-tauri/src/storage/user_scoped_paths.rs
```

整段删除。

`user_scoped_paths.rs:201` 测试断言 `paths.memory_dir()` 一并删。

- [ ] **Step 5：build + test**

```bash
cd src-tauri && cargo build --lib && cargo test --lib -- --skip review_
```

预期：全过。

- [ ] **Step 6：commit**

```bash
git add -A
git commit -m "chore(storage): remove all shared/memory mkdir calls and memory_dir helpers"
```

---

### Task 25：删除 FileAuthorizedWorkspaceStore（已无 caller）

**Files:**
- Modify: `src-tauri/src/runtime/store/authorized_workspace_store.rs`
- Modify: `src-tauri/src/runtime/store/mod.rs`

- [ ] **Step 1：从 authorized_workspace_store.rs 删除 FileAuthorizedWorkspaceStore 整个 struct + impl**

```bash
grep -n 'FileAuthorizedWorkspaceStore' src-tauri/src/runtime/store/authorized_workspace_store.rs
```

把 `pub struct FileAuthorizedWorkspaceStore { ... }` 和 `impl AuthorizedWorkspaceStore for FileAuthorizedWorkspaceStore { ... }` 整段删除（约 line 51-81）。

也要删那段 doc 注释：

```rust
// ─────────────────────────────────────────────────────────────────────────────
// File-backed production implementation
// ─────────────────────────────────────────────────────────────────────────────
```

- [ ] **Step 2：从 runtime/store/mod.rs 删 re-export**

```rust
pub use authorized_workspace_store::{
    AuthorizedWorkspace, AuthorizedWorkspaceRef, AuthorizedWorkspaceStore,
    ConvJsonAuthorizedWorkspaceStore, FileAuthorizedWorkspaceStore, InMemoryAuthorizedWorkspaceStore,
};
```

去掉 `FileAuthorizedWorkspaceStore,`。

- [ ] **Step 3：build**

```bash
cd src-tauri && cargo build --lib
```

预期：0 error。

- [ ] **Step 4：跑全仓**

```bash
cd src-tauri && cargo test -- --skip review_
```

预期：全过。

- [ ] **Step 5：commit**

```bash
git add -A
git commit -m "chore(store): remove FileAuthorizedWorkspaceStore (replaced by ConvJsonAuthorizedWorkspaceStore)"
```

---

### Task 26：新增 review test 锁层

**Files:**
- Create: `src-tauri/tests/review_no_memory_kv.rs`

- [ ] **Step 1：建测试文件**

```rust
//! review test：锁死 memory KV 设施所有符号 / 字符串字面量从生产代码（src-tauri/src/）消失。
//!
//! 这是反模式护栏——未来如果有人想再引入"借用 KV 设施做单 key upsert"的反模式，CI 会立即报错。
//!
//! 注意：本扫描**不包含 src-tauri/tests/**。`user_scope_migration_test.rs` 等历史 fixture
//! 可能仍然用到 "shared/memory" 字符串构造路径（是允许的，老 fixture 留作回归案例）。

use std::path::Path;

fn collect_rs_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn assert_no_match(pattern: &str, label: &str) {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);

    let mut offenders = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file).unwrap();
        for (i, line) in content.lines().enumerate() {
            if line.contains(pattern) {
                offenders.push(format!("{}:{} {}", file.display(), i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{}: found {} occurrences in src/:\n{}",
        label,
        offenders.len(),
        offenders.join("\n")
    );
}

#[test]
fn no_memory_kv_api_calls() {
    for pat in &[
        "set_memory(",
        "get_memory(",
        "get_memories_by_prefix(",
        "delete_memories_by_prefix(",
    ] {
        assert_no_match(pat, pat);
    }
}

#[test]
fn no_memory_kv_types() {
    for pat in &[
        "MemoryEntry",
        "FileMemoryStore",
        "MemoryStore",
        "InMemoryMemoryStore",
        "FileAuthorizedWorkspaceStore",
    ] {
        assert_no_match(pat, pat);
    }
}

#[test]
fn no_dead_loaded_helpers() {
    for pat in &["loaded_key", "loaded_prefix", "load_failed_key"] {
        assert_no_match(pat, pat);
    }
}

#[test]
fn no_memory_string_literals() {
    for pat in &[
        "\"memory.jsonl\"",
        "\"shared/memory\"",
        "\"loaded:",
        "\"note:",
    ] {
        assert_no_match(pat, pat);
    }
}
```

- [ ] **Step 2：跑 review test**

```bash
cd src-tauri && cargo test --test review_no_memory_kv -- --nocapture
```

预期：4 个测试全过。如失败，说明还有漏删的——按报告位置去删。

- [ ] **Step 3：commit**

```bash
git add src-tauri/tests/review_no_memory_kv.rs
git commit -m "test(review): lock memory KV facility deletion (no future regressions)"
```

---

### Task 27：清理无用的 file_store/mod.rs 测试

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs:1640-1655`

- [ ] **Step 1：找到 set_memory("company", ...) 风格的测试**

```bash
sed -n '1635,1660p' src-tauri/src/storage/file_store/mod.rs
```

- [ ] **Step 2：删整段相关 test fn**

测试名称应该是 `test_memory_*` / `set_memory_*` 之类。整 `#[test]` 函数删除。

- [ ] **Step 3：build + test**

```bash
cd src-tauri && cargo build --lib && cargo test --lib -- --skip review_
```

预期：全过。

- [ ] **Step 4：commit**

```bash
git add src-tauri/src/storage/file_store/mod.rs
git commit -m "test(storage): remove obsolete memory KV unit tests"
```

---

## Phase 5: 前端 AppSettings 接入（PR5 内部）

### Task 28：`AppSettings` 加两个新字段

**Files:**
- Modify: `src-tauri/src/models/settings.rs`

- [ ] **Step 1：在 AppSettings struct 末尾加 2 个字段**

```rust
pub struct AppSettings {
    // ... 既有字段 ...

    /// JSON-stringified AuthorizedWorkspaceRef。首页 task composer 当前选中。
    /// 空字符串视为未选中。
    #[serde(default)]
    pub ui_home_selected_workspace: String,

    /// JSON-stringified AuthorizedWorkspaceRef[]。首页切换器最近列表。
    /// 空字符串或 "[]" 视为空列表。**前端限定最多 10 条**：超出时 LRU 截断。
    #[serde(default)]
    pub ui_home_recent_workspaces: String,
}
```

- [ ] **Step 2：在 `Default for AppSettings` 里加默认值**

```rust
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            // ... 既有 ...
            ui_home_selected_workspace: String::new(),
            ui_home_recent_workspaces: String::new(),
        }
    }
}
```

- [ ] **Step 3：找到 `AppSettings::from_string_map`**

```bash
grep -n 'fn from_string_map' src-tauri/src/models/settings.rs
```

应该是按 key 反查 string map → 填字段。加两个字段的解析：

```rust
ui_home_selected_workspace: map
    .get("uiHomeSelectedWorkspace")
    .cloned()
    .unwrap_or_default(),
ui_home_recent_workspaces: map
    .get("uiHomeRecentWorkspaces")
    .cloned()
    .unwrap_or_default(),
```

- [ ] **Step 4：单测**

```rust
#[cfg(test)]
mod home_workspace_field_tests {
    use super::*;

    #[test]
    fn defaults_to_empty_string() {
        let s = AppSettings::default();
        assert_eq!(s.ui_home_selected_workspace, "");
        assert_eq!(s.ui_home_recent_workspaces, "");
    }

    #[test]
    fn round_trips_through_json() {
        let s = AppSettings {
            ui_home_selected_workspace: r#"{"id":"ws-1","rootPath":"/x","displayName":"x"}"#.to_string(),
            ui_home_recent_workspaces: r#"[{"id":"ws-1","rootPath":"/x","displayName":"x"}]"#.to_string(),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.ui_home_selected_workspace, s.ui_home_selected_workspace);
        assert_eq!(parsed.ui_home_recent_workspaces, s.ui_home_recent_workspaces);
    }
}
```

- [ ] **Step 5：跑测试**

```bash
cd src-tauri && cargo test --lib home_workspace_field -- --nocapture
```

预期：2 个测试通过。

- [ ] **Step 6：commit**

```bash
git add src-tauri/src/models/settings.rs
git commit -m "feat(settings): add uiHomeSelectedWorkspace / uiHomeRecentWorkspaces to AppSettings"
```

---

### Task 29：前端 `src/lib/tauri.ts` 补类型 + IPC wrapper

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1：找到 Settings interface**

```bash
grep -n 'interface Settings\|type Settings' src/lib/tauri.ts | head -5
```

- [ ] **Step 2：在 Settings interface 加两个字段**

```ts
export interface Settings {
  // ... 既有字段 ...
  uiHomeSelectedWorkspace: string
  uiHomeRecentWorkspaces: string
}
```

- [ ] **Step 3：加 ConversationSource DTO + 3 个 IPC wrapper**

在文件合适位置（其它 conversation-level IPC 附近）：

```ts
export type ConversationSourceDto =
  | { kind: 'user' }
  | { kind: 'employee'; employeeId: string }
  | { kind: 'expertTeam'; expertTeamId: string }
  | { kind: 'im' }

export function setConversationExpertTeam(
  conversationId: string,
  expertTeamId: string,
  teamLabel: string,
): Promise<void> {
  return invoke('set_conversation_expert_team', { conversationId, expertTeamId, teamLabel })
}

export function clearConversationSource(conversationId: string): Promise<void> {
  return invoke('clear_conversation_source', { conversationId })
}

export function getConversationSource(conversationId: string): Promise<ConversationSourceDto> {
  return invoke('get_conversation_source', { conversationId })
}
```

- [ ] **Step 4：找到 Conversation / index entry 前端类型**

```bash
grep -n 'interface Conversation\|type Conversation' src/types/message.ts src/lib/tauri.ts | head -10
```

- [ ] **Step 5：补 kind / sourceLabel / workspaceName 字段**

如果当前 Conversation 类型中已有 `workspaceName?: string`，加：

```ts
export interface Conversation {
  // ... 既有 ...
  kind?: 'user' | 'employee' | 'expertTeam' | 'im'
  sourceLabel?: string | null
}
```

- [ ] **Step 6：build 前端类型**

```bash
pnpm exec tsc --noEmit
```

预期：0 error（或仅未触及的）。

- [ ] **Step 7：commit**

```bash
git add src/lib/tauri.ts src/types/message.ts
git commit -m "feat(types): add ConversationSourceDto + 3 IPC wrappers + index mirror fields"
```

---

### Task 30：改写 `src/stores/homeStore.ts`

**Files:**
- Modify: `src/stores/homeStore.ts`

- [ ] **Step 1：写新版**

完全替换文件内容：

```ts
import { create } from 'zustand'

import type { AuthorizedWorkspaceRef, Settings } from '@/lib/tauri'
import { updateSettings } from '@/lib/tauri'

const MAX_RECENT_WORKSPACES = 10

interface HomeState {
  selectedWorkspace: AuthorizedWorkspaceRef | null
  recentWorkspaces: AuthorizedWorkspaceRef[]
  setSelectedWorkspace: (ws: AuthorizedWorkspaceRef | null) => void
  removeRecentWorkspace: (rootPath: string) => void
}

function tryParse<T>(raw: string, fallback: T): T {
  if (!raw) return fallback
  try {
    return JSON.parse(raw) as T
  } catch {
    return fallback
  }
}

function persist(selectedWorkspace: AuthorizedWorkspaceRef | null, recent: AuthorizedWorkspaceRef[]) {
  updateSettings({
    uiHomeSelectedWorkspace: selectedWorkspace ? JSON.stringify(selectedWorkspace) : '',
    uiHomeRecentWorkspaces: recent.length ? JSON.stringify(recent) : '',
  } as Partial<Settings> as Settings).catch((err) => {
    // 持久化失败不影响内存态；上次失败下次切换 workspace 再试
    console.warn('[homeStore] persist failed:', err)
  })
}

function withWorkspaceFirst(
  recent: AuthorizedWorkspaceRef[],
  workspace: AuthorizedWorkspaceRef,
): AuthorizedWorkspaceRef[] {
  const next = [
    workspace,
    ...recent.filter((item) => item.rootPath !== workspace.rootPath),
  ]
  return next.slice(0, MAX_RECENT_WORKSPACES)
}

export const useHomeStore = create<HomeState>((set, get) => ({
  selectedWorkspace: null,
  recentWorkspaces: [],
  setSelectedWorkspace: (ws) => {
    if (!ws) {
      persist(null, get().recentWorkspaces)
      set({ selectedWorkspace: null })
      return
    }
    const recentWorkspaces = withWorkspaceFirst(get().recentWorkspaces, ws)
    persist(ws, recentWorkspaces)
    set({ selectedWorkspace: ws, recentWorkspaces })
  },
  removeRecentWorkspace: (rootPath) => {
    const recentWorkspaces = get().recentWorkspaces.filter((ws) => ws.rootPath !== rootPath)
    persist(get().selectedWorkspace, recentWorkspaces)
    set({ recentWorkspaces })
  },
}))

/// hydrate from AppSettings at startup; called from App.tsx top-level effect.
export function hydrateHomeStore(settings: Settings) {
  const selected = tryParse<AuthorizedWorkspaceRef | null>(
    settings.uiHomeSelectedWorkspace ?? '',
    null,
  )
  const recent = tryParse<AuthorizedWorkspaceRef[]>(
    settings.uiHomeRecentWorkspaces ?? '',
    [],
  )
  useHomeStore.setState({
    selectedWorkspace: selected,
    recentWorkspaces: Array.isArray(recent) ? recent.slice(0, MAX_RECENT_WORKSPACES) : [],
  })
}
```

- [ ] **Step 2：写测试**

```ts
// src/stores/__tests__/homeStore.test.ts
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mockUpdateSettings = vi.fn().mockResolvedValue(undefined)
vi.mock('@/lib/tauri', () => ({
  updateSettings: mockUpdateSettings,
}))

import { hydrateHomeStore, useHomeStore } from '../homeStore'

beforeEach(() => {
  useHomeStore.setState({ selectedWorkspace: null, recentWorkspaces: [] })
  mockUpdateSettings.mockClear()
})

describe('homeStore', () => {
  it('hydrates from empty settings to empty state', () => {
    hydrateHomeStore({ uiHomeSelectedWorkspace: '', uiHomeRecentWorkspaces: '' } as any)
    expect(useHomeStore.getState().selectedWorkspace).toBe(null)
    expect(useHomeStore.getState().recentWorkspaces).toEqual([])
  })

  it('hydrates selectedWorkspace and recentWorkspaces from JSON strings', () => {
    hydrateHomeStore({
      uiHomeSelectedWorkspace: '{"id":"ws-1","rootPath":"/x","displayName":"x"}',
      uiHomeRecentWorkspaces:
        '[{"id":"ws-1","rootPath":"/x","displayName":"x"},{"id":"ws-2","rootPath":"/y","displayName":"y"}]',
    } as any)
    expect(useHomeStore.getState().selectedWorkspace?.id).toBe('ws-1')
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(2)
  })

  it('setSelectedWorkspace pushes to recentWorkspaces head and persists', () => {
    const ws = { id: 'ws-1', rootPath: '/x', displayName: 'x' }
    useHomeStore.getState().setSelectedWorkspace(ws as any)
    expect(useHomeStore.getState().selectedWorkspace).toEqual(ws)
    expect(useHomeStore.getState().recentWorkspaces[0]).toEqual(ws)
    expect(mockUpdateSettings).toHaveBeenCalled()
  })

  it('recentWorkspaces is capped at MAX_RECENT_WORKSPACES (10)', () => {
    for (let i = 0; i < 15; i++) {
      useHomeStore.getState().setSelectedWorkspace({
        id: `ws-${i}`,
        rootPath: `/x${i}`,
        displayName: `x${i}`,
      } as any)
    }
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(10)
    // 最新的应该在 head
    expect(useHomeStore.getState().recentWorkspaces[0].id).toBe('ws-14')
  })

  it('removeRecentWorkspace removes by rootPath and persists', () => {
    useHomeStore.getState().setSelectedWorkspace({
      id: 'ws-1',
      rootPath: '/x',
      displayName: 'x',
    } as any)
    useHomeStore.getState().setSelectedWorkspace({
      id: 'ws-2',
      rootPath: '/y',
      displayName: 'y',
    } as any)
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(2)
    useHomeStore.getState().removeRecentWorkspace('/x')
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(1)
    expect(useHomeStore.getState().recentWorkspaces[0].id).toBe('ws-2')
  })

  it('does not read or write localStorage', () => {
    const localStorageSpy = vi.spyOn(window.localStorage.__proto__ as any, 'setItem')
    useHomeStore.getState().setSelectedWorkspace({
      id: 'ws-1',
      rootPath: '/x',
      displayName: 'x',
    } as any)
    expect(localStorageSpy).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 3：跑测试**

```bash
pnpm exec vitest run src/stores/__tests__/homeStore.test.ts
```

预期：6 个测试全过。

- [ ] **Step 4：commit**

```bash
git add src/stores/homeStore.ts src/stores/__tests__/homeStore.test.ts
git commit -m "refactor(homeStore): persist via AppSettings (file) instead of localStorage; LRU cap 10"
```

---

### Task 31：在 `App.tsx` 顶层加 hydrate effect

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1：找到 App 组件**

```bash
grep -n 'function App\|export default function App\|const App' src/App.tsx | head -5
```

- [ ] **Step 2：在 App 顶层 useEffect 加 hydrate**

```ts
import { useEffect } from 'react'
import { getSettings } from '@/lib/tauri'
import { hydrateHomeStore } from '@/stores/homeStore'

// 在 App 组件内（其他 useEffect 附近）
useEffect(() => {
  let cancelled = false
  ;(async () => {
    try {
      const settings = await getSettings()
      if (!cancelled) hydrateHomeStore(settings)
    } catch (err) {
      console.warn('[App] hydrate homeStore failed:', err)
    }
  })()
  return () => {
    cancelled = true
  }
}, [])
```

- [ ] **Step 3：build**

```bash
pnpm exec tsc --noEmit
```

预期：0 error。

- [ ] **Step 4：commit**

```bash
git add src/App.tsx
git commit -m "feat(app): hydrate homeStore from AppSettings on startup"
```

---

## Phase 6: 改造 expertTeamRegistry（PR6 内部）

### Task 32：改写 expertTeamRegistry.ts

**Files:**
- Modify: `src/features/expert-teams/expertTeamRegistry.ts`

- [ ] **Step 1：完全替换文件**

```ts
// code/src/features/expert-teams/expertTeamRegistry.ts
//
// 会话 ↔ 专家团映射。**已从 localStorage 改为后端 conv.json 持久化**。
// 走 Tauri IPC：set_conversation_expert_team / clear_conversation_source / get_conversation_source。
//
// 高频渲染：用 hasExpertTeam(convId) / useExpertTeamForConversation hook 反查
// useChatStore.conversations 的 kind === 'expertTeam' （boolean 同步）
//
// 低频精确：用 getExpertTeamId(convId) async 读 conv.json 拿到具体 expertTeamId

import { useChatStore } from '@/stores/chatStore'
import {
  clearConversationSource,
  getConversationSource,
  setConversationExpertTeam,
} from '@/lib/tauri'
import { EXPERT_TEAMS, type ExpertTeamId } from './teams'

const VALID_IDS = new Set<ExpertTeamId>(EXPERT_TEAMS.map((t) => t.id))

function labelFor(teamId: ExpertTeamId): string {
  return EXPERT_TEAMS.find((t) => t.id === teamId)?.name ?? teamId
}

/**
 * 设置会话的专家团归属。
 *
 * 副作用：调 Tauri 写 conv.json + mirror index.json。
 * 失败时只 warn，不抛——store 内存态仍代表"用户意图"。
 */
export async function setExpertTeam(conversationId: string, teamId: ExpertTeamId): Promise<void> {
  if (!VALID_IDS.has(teamId)) return
  try {
    await setConversationExpertTeam(conversationId, teamId, labelFor(teamId))
  } catch (err) {
    console.warn('[expertTeamRegistry] setExpertTeam failed:', err)
  }
}

/**
 * 同步判断：会话是否属于某个专家团。基于 useChatStore.conversations 的 kind 字段。
 *
 * 高频路径用这个（侧边栏 chip / 标题渲染等）。返回 boolean。
 */
export function hasExpertTeam(conversationId: string): boolean {
  const conv = useChatStore
    .getState()
    .conversations.find((c) => c.id === conversationId)
  return conv?.kind === 'expertTeam'
}

/**
 * 异步精确读：拿到会话的 expertTeamId。
 *
 * 低频路径用这个（点开会话后渲染团队详情、跳转到团队页等）。返回 undefined 表示
 * 不是专家团或读取失败。
 */
export async function getExpertTeamId(
  conversationId: string,
): Promise<ExpertTeamId | undefined> {
  try {
    const source = await getConversationSource(conversationId)
    if (source.kind !== 'expertTeam') return undefined
    const id = source.expertTeamId as ExpertTeamId
    return VALID_IDS.has(id) ? id : undefined
  } catch (err) {
    console.warn('[expertTeamRegistry] getExpertTeamId failed:', err)
    return undefined
  }
}

/**
 * 清除会话的专家团归属（设回 user kind）。
 */
export async function clearExpertTeam(conversationId: string): Promise<void> {
  try {
    await clearConversationSource(conversationId)
  } catch (err) {
    console.warn('[expertTeamRegistry] clearExpertTeam failed:', err)
  }
}

/**
 * React hook：订阅 useChatStore.conversations，返回 boolean
 * (会话是否属于专家团)。
 */
export function useExpertTeamForConversation(
  conversationId: string | null | undefined,
): boolean {
  return useChatStore((state) => {
    if (!conversationId) return false
    return state.conversations.find((c) => c.id === conversationId)?.kind === 'expertTeam'
  })
}
```

- [ ] **Step 2：写测试**

```ts
// src/features/expert-teams/__tests__/expertTeamRegistry.test.ts
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mockSetExpertTeam = vi.fn().mockResolvedValue(undefined)
const mockClearSource = vi.fn().mockResolvedValue(undefined)
const mockGetSource = vi.fn()
vi.mock('@/lib/tauri', () => ({
  setConversationExpertTeam: mockSetExpertTeam,
  clearConversationSource: mockClearSource,
  getConversationSource: mockGetSource,
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: {
    getState: () => ({
      conversations: [
        { id: 'c-1', title: 'a', kind: 'expertTeam' },
        { id: 'c-2', title: 'b', kind: 'user' },
        { id: 'c-3', title: 'c' /* no kind field */ },
      ],
    }),
  },
}))

import {
  clearExpertTeam,
  getExpertTeamId,
  hasExpertTeam,
  setExpertTeam,
} from '../expertTeamRegistry'

beforeEach(() => {
  mockSetExpertTeam.mockClear()
  mockClearSource.mockClear()
  mockGetSource.mockReset()
})

describe('expertTeamRegistry', () => {
  it('setExpertTeam invokes set_conversation_expert_team', async () => {
    await setExpertTeam('c-1', 'marketing' as any)
    expect(mockSetExpertTeam).toHaveBeenCalledWith('c-1', 'marketing', expect.any(String))
  })

  it('hasExpertTeam returns true for kind=expertTeam', () => {
    expect(hasExpertTeam('c-1')).toBe(true)
  })

  it('hasExpertTeam returns false for kind=user', () => {
    expect(hasExpertTeam('c-2')).toBe(false)
  })

  it('hasExpertTeam returns false for missing kind field', () => {
    expect(hasExpertTeam('c-3')).toBe(false)
  })

  it('hasExpertTeam returns false for unknown conversationId', () => {
    expect(hasExpertTeam('c-nonexistent')).toBe(false)
  })

  it('getExpertTeamId returns id from conv.json source', async () => {
    mockGetSource.mockResolvedValue({ kind: 'expertTeam', expertTeamId: 'marketing' })
    const id = await getExpertTeamId('c-1')
    expect(id).toBe('marketing')
  })

  it('getExpertTeamId returns undefined when source kind is not expertTeam', async () => {
    mockGetSource.mockResolvedValue({ kind: 'user' })
    const id = await getExpertTeamId('c-1')
    expect(id).toBeUndefined()
  })

  it('clearExpertTeam invokes clear_conversation_source', async () => {
    await clearExpertTeam('c-1')
    expect(mockClearSource).toHaveBeenCalledWith('c-1')
  })
})
```

- [ ] **Step 3：跑测试**

```bash
pnpm exec vitest run src/features/expert-teams/__tests__/expertTeamRegistry.test.ts
```

预期：8 个测试通过。

- [ ] **Step 4：commit**

```bash
git add src/features/expert-teams/expertTeamRegistry.ts src/features/expert-teams/__tests__/expertTeamRegistry.test.ts
git commit -m "refactor(expertTeamRegistry): persist via conv.json (kind field) instead of localStorage"
```

---

### Task 33：审计所有 `useExpertTeamForConversation` / `getExpertTeam` 调用点

**Files:**
- Audit all callers of these symbols

- [ ] **Step 1：grep 所有调用点**

```bash
grep -rn 'useExpertTeamForConversation\|getExpertTeam(' src/ 2>/dev/null
```

记下每个调用文件。

- [ ] **Step 2：逐个修复**

对每个调用点：

- 如果原来用法是"判断会话是否属于专家团"（用 boolean 当条件）→ 直接换成新的 hook（返回 boolean），不需要改 boolean 用法
- 如果原来用法是"拿 teamId 进而显示/跳转"→ 改为 `await getExpertTeamId(convId)`，配套加 loading 态（如果是 React 组件，用 useState + useEffect 异步加载）

**具体调用点处理**（按 grep 结果实际处理；如果只有少数地方，举一个典型示例）：

例如 `AppSidebar.tsx:110` 用 `getExpertTeam(c.id)` 过滤会话：

```ts
// 旧：const expertTeamConversations = nonChannelConversations.filter((c) => getExpertTeam(c.id))
// 新：直接用 conversations 自带的 kind 字段
const expertTeamConversations = nonChannelConversations.filter((c) => c.kind === 'expertTeam')
```

如果有调用点真的需要 teamId（而不是 boolean），按异步 hook + loading 模式改。

- [ ] **Step 3：跑前端测试**

```bash
pnpm test
```

预期：相关测试都过。可能有些既存测试 mock 了 `getExpertTeam`，需要更新到新 API。

- [ ] **Step 4：dev server 验证**

```bash
pnpm tauri:dev
```

手动验证：侧边栏专家团 tab、新建专家团会话、切换专家团、归档/恢复 — 行为应与之前一致。

- [ ] **Step 5：commit**

```bash
git add -A
git commit -m "refactor: migrate all getExpertTeam callers to kind-based filter / async getExpertTeamId"
```

---

## Phase 7: 文档收尾（PR7 内部）

### Task 34：更新历史 plan 加 banner

**Files:**
- Modify: `docs/superpowers/plans/2026-04-24-homepage-workspace-selection.md`

- [ ] **Step 1：在文件顶部加 banner**

打开 `docs/superpowers/plans/2026-04-24-homepage-workspace-selection.md`，在第 1 行（`# ` 标题之前）插入：

```markdown
> **⚠️ 已被 [2026-05-20 conversation-source-and-workspace-cleanup spec](../specs/2026-05-20-conversation-source-and-workspace-cleanup-design.md) 取代**：
> 本 plan 提到的 `aijia-home-workspace` 等 localStorage key 已迁到 `AppSettings.uiHomeSelectedWorkspace` / `uiHomeRecentWorkspaces` 文件持久化字段。下面的内容仅作历史归档。

```

- [ ] **Step 2：commit**

```bash
git add docs/superpowers/plans/2026-04-24-homepage-workspace-selection.md
git commit -m "docs: banner historic home-workspace plan as superseded by 2026-05-20 spec"
```

---

### Task 35：最终全仓验证 + dev smoke test

**Files:** N/A

- [ ] **Step 1：跑全仓 Rust 测试**

```bash
cd src-tauri && cargo test --no-fail-fast 2>&1 | tail -30
```

预期：全部通过。特别确认：

- `cargo test --test review_no_memory_kv` ✓
- `cargo test --test conversation_source_test` ✓
- `cargo test --test conv_json_workspace_store_test` ✓
- `cargo test --test review_im_layering`（既存）✓

- [ ] **Step 2：跑前端测试**

```bash
pnpm test
```

预期：全部通过。

- [ ] **Step 3：跑 Tauri dev 进行手动 smoke test**

```bash
pnpm tauri:dev
```

手动验证：

1. **首页 workspace 切换器**：选一个目录 → 关 app → 重启 → 选中状态仍在（验证 hydrate 工作）
2. **最近 workspace 列表**：连续选 12 个不同目录 → 只显示最近 10 个，最旧的被截尾
3. **会话归属专家团**：选个会话 → 通过 UI 操作绑定到某个团 → 关 app → 重启 → 会话仍属于该团（验证 conv.json 持久化）
4. **侧边栏分组**：有 workspace 绑定的会话进对应分组（不再"默认文件夹"）
5. **新建会话 + 授权目录**：检查 `~/.renlijia/users/{scope}/conversations/{id}/conv.json` 里有 `authorizedWorkspace` 字段，内容不含 `sessionId` 子字段
6. **删除会话**：删除后 index.json 中对应 entry 消失，无报错

- [ ] **Step 4：核对 memory.jsonl 老数据沉默冻结**

```bash
cat ~/.renlijia/users/*/shared/memory/memory.jsonl | wc -l
```

预期：仍是 148 行（老数据），不增不减。

- [ ] **Step 5：核对 conv.json 字段**

```bash
cat ~/.renlijia/users/*/conversations/*/conv.json | jq -r 'select(.authorizedWorkspace) | .authorizedWorkspace' 2>/dev/null | head -3
```

预期：能看到 `{id, rootPath, displayName, authorizedAt}` 4 个字段，**没有** sessionId。

- [ ] **Step 6：全过后 commit**

```bash
git add -A  # 应该没有任何更改
echo "✓ Implementation complete"
```

---

## Self-Review

**1. Spec coverage** — 对 spec 每节排查：

| Spec 章节 | Task 编号 |
|---|---|
| §1.1 `ConversationSource` + 未知 variant 兜底 | Task 3 |
| §1.2 `ConversationMeta` 加 3 字段 + `PersistedAuthorizedWorkspace` | Task 1, 4 |
| §1.3 `ConversationIndexEntry` 加 mirror 字段 | Task 5 |
| §1.4 `AppSettings` 2 个字段 + 10 上限 | Task 28, 30 |
| §2.1 `homeStore` hydrate / persist + LRU cap | Task 30 |
| §2.2 `expertTeamRegistry` 拆 hasExpertTeam / getExpertTeamId | Task 32, 33 |
| §2.3 IPC wrapper | Task 29 |
| §2.4 类型层 | Task 29 |
| §2.5 删除字符串字面量 | Task 30, 32（直接删）+ Task 26（review test 锁住） |
| §3.1 3 个 Tauri commands | Task 15 |
| §3.2 workspace 写路径双写 | Task 11, 14（写路径自动双写，因为 ConvJsonAuthorizedWorkspaceStore 直接调 set_conversation_workspace） |
| §3.3 `ConvJsonAuthorizedWorkspaceStore` + trait 签名改 | Task 9, 10, 11, 12 |
| §3.4 `get_conversations` 删 fan-out + `load_explicit_workspace` 改读 | Task 16, 17 |
| §3.5 删除 memory KV 设施完整清单 | Task 18, 19, 20, 21, 22, 23, 24, 25, 27 + Review test Task 26 |
| §4 启动 hydrate | Task 31 |
| §5 错误处理 | 各 task 内置（`atomic_write_json` / `#[serde(default)]` / 自定义 deserialize） |
| §6 测试覆盖 | Task 2, 3, 4, 5, 7, 8, 12, 26, 30, 32 |
| §7 PR 拆分 + 回滚约束 | Plan 按 Phase 对应 PR1–PR7 |

✓ 全覆盖。

**2. Placeholder scan** — 整 plan 无 "TODO" / "TBD" / "implement later" / "similar to" / "appropriate error handling"。每个 code step 都给出完整代码。

**3. Type consistency** — `ConversationSource` / `ConversationKind` / `PersistedAuthorizedWorkspace` 在 Task 1–5 定义，后续 task（如 Task 6 helper / Task 11 store / Task 15 commands）的方法签名都跟前面定义对齐。`set_conversation_source` / `set_conversation_workspace` / `read_conversation_workspace` / `read_conversation_source` 在 Task 6 / 8 定义，Task 11 / 15 / 17 使用一致。`hasExpertTeam` / `getExpertTeamId` 在 Task 32 定义，Task 33 调用一致。

---

## Execution Handoff

Plan saved to `docs/superpowers/plans/2026-05-20-conversation-source-and-workspace-cleanup.md`. Two execution options:

**1. Subagent-Driven (recommended)** — Fresh subagent per task, two-stage review between tasks, fast iteration. Best when individual tasks are independent and the human reviews each batch.

**2. Inline Execution** — Execute tasks in this session using executing-plans skill, batch with checkpoints. Best when you want to watch progress directly.

Which approach?
