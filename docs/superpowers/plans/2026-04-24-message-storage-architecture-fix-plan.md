# 消息系统全链路重构计划（修订版 v3）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对标 claude-code-best 架构原则，修复 AIjia 消息系统的存储一致性、历史重建正确性、compact 有效性。全程保持 OpenAI-compatible 消息格式，前端大改版问题留待后续处理。

**Architecture:** 单文件 JSONL + UUID dedup（存储层）→ Runtime history.rs 接管历史重建 + round-based 裁剪 → compact 后 tail round 完整保留 → SessionMessageStore 作为 session 级真相源。

**Tech Stack:** Rust 1.77+、Tauri 2.x、serde_json、JSONL append-only、OpenAI Chat Completions API

---

## 1. 背景

### 1.1 产品定位

AIjia（AI小家）是 Tauri 2.x 桌面应用，核心能力是 agentic 多轮工具调用。LLM 使用 **OpenAI-compatible API**（DeepSeek、Qwen、Volcano 等），全程不切换到 Anthropic 格式。运行时数据根目录：`~/.renlijia/`。

### 1.2 最新代码（2026-04-24 拉取后）已实现的能力

下列功能已在最新代码中实现，**本计划不重复实现**：

| 功能 | 实现位置 | 状态 |
|---|---|---|
| assistant with tool_calls 持久化 | `chat.rs:775 persist_iteration_assistant_message` | ✅ 已完成 |
| tool result 消息持久化 | `chat.rs:801 persist_tool_messages` | ✅ 已完成 |
| 历史重建支持 role=tool / assistant-with-toolCalls | `chat.rs:114-184 build_history_from_compact_boundary` | ✅ 已完成 |
| compact_boundaries.jsonl 加载 | `chat.rs:1189-1203 load_history` | ✅ 已完成 |
| 前端 MessageRole 包含 'tool' | `types/message.ts:6` | ✅ 已完成 |

### 1.3 仍然存在的问题

经最新代码复查，以下问题**仍未解决**：

#### P0：直接影响正确性

| # | 问题 | 位置 |
|---|---|---|
| P0-1 | 分片存储 seq 复用竞态：append 成功但 `_current` 未更新时崩溃，下次 insert 复用同一 seq，后条被 dedup 静默丢弃 | `messages.rs:140-141` |
| P0-2 | `HISTORY_LIMIT = 50` 固定条数截断，不保证 OpenAI tool pair 完整性（截断可能产生孤立 role=tool → API 400） | `chat.rs:1177` |
| P0-3 | compact 后下个 turn 仍全量加载历史（`get_recent_messages` 取 50 条），compact 前消息可能再次进入上下文，compact 实际失效 | `chat.rs:1179-1209` |
| P0-4 | `compact_messages_via_llm` 只保留 `latest_user_message`，不保留完整 tail round：compact 发生在工具调用中间时，assistant.tool_calls 被丢弃，剩余 tool result 孤立 → 下轮 API 400 | `compaction.rs:289-318` |
| P0-5 | `serde_json::from_value(...).ok()` 静默丢弃反序列化失败的历史消息，历史上下文被隐式截断 | `chat.rs:294-299` |

#### P1：架构问题

| # | 问题 | 位置 |
|---|---|---|
| P1-1 | `StoredMessage` 仍用 `content: serde_json::Value` opaque blob 存 tool 字段（`toolCallId`/`toolCalls` 嵌在 content JSON 里），无顶级结构化字段 | `types.rs:52` |
| P1-2 | 历史加载（HISTORY_LIMIT/裁剪/tool-pair 校验）在 Transport 层，Runtime 层 `load_history()` 默认空 Vec | `chat.rs:1173` |
| P1-3 | `token_budget` 默认 4096，普通 daily 对话不走 skill 时输出被截断 | `chat_turn_driver.rs:666` |
| P1-4 | `agentName` 前端传 IPC，Rust command 不接收，静默丢失 | `commands/chat.rs:21-31` |
| P1-5 | `update_message_content` max_rev 只扫原 shard，重复 update 产生相同 rev，结果不确定 | `messages.rs:244-263` |
| P1-6 | 同一 id 重复 insert 产生两条独立消息，无幂等保护 | `messages.rs:108` |

#### P2：次要问题（可后续处理）

| # | 问题 | 位置 |
|---|---|---|
| P2-1 | `StoredMessage` 无 `schema_version`，未来加字段无迁移挂钩 | `types.rs:52` |
| P2-2 | `StoredMessage` 无 `run_id`，多并发 turn 消息无法归因 | `types.rs:52` |
| P2-3 | `chunk_timeout_secs: 90` 硬编码无配置通道 | `chat_turn_driver.rs:667` |
| P2-4 | user message id `msg-{uuid}`、assistant 裸 UUID，格式不一致 | `chat.rs:676/723` |
| P2-5 | `streaming:done` payload 无 messageId，前端类型声明却有（类型撒谎） | `tauri_event_adapter.rs:29-35` |

#### 前端问题（大改版时统一处理，本计划不做）

S4-1（streaming:done messageId）、S4-2（非 active 对话 message:updated 丢弃）、S4-3（tool 事件无 run_id）、S5-2（MessageContent 无 toolCalls/toolResult）、乐观消息不回滚、切换对话闪空 → **全部留待前端大改版**。

---

## 2. 对标 claude-code-best 的架构原则

`/Users/a20250311/github/claude-code-best` 核心可借鉴原则（不依赖 Anthropic 格式）：

| 原则 | claude-code-best | AIjia 目标 | 当前差距 |
|---|---|---|---|
| 单文件 JSONL + UUID dedup | `<sessionId>.jsonl` | `messages.jsonl` | 仍是分片，有 seq/rev 竞态 |
| compact boundary 视图 | `getMessagesAfterCompactBoundary()` 每轮截断 | `load_history` 取全量再截 | compact 后下轮仍加载旧历史 |
| compact 保留 tail round | `buildPostCompactMessages` 保留完整 preserved segment | 只保留 latest_user | P0-4 |
| round-based 历史裁剪 | 以消息 pair 为原子单位 | 固定 50 条 | P0-2 |
| Runtime 拥有历史 | `QueryEngine.mutableMessages` | Transport 层 `HISTORY_LIMIT` | P1-2 |
| per-call ToolUseContext | 每次 submitMessage 重建 | 无直接等价 | P1 级 |

---

## 3. 修复后的目标状态

```
存储层：
  ~/.renlijia/conversations/{id}/messages.jsonl  ← 单文件 append-only，UUID last-writer-wins
  StoredMessage 有顶级 tool_calls / tool_call_id / name / run_id / schema_version

历史加载：
  runtime/chat/history.rs::build_chat_history()
    → 从 messages.jsonl 加载（已含 tool 消息）
    → 应用 compact_boundaries.jsonl（只返回 boundary 之后视图）
    → 过滤非法 OpenAI tool pair（双向校验）
    → round-based 裁剪（max_rounds=30 + char_budget=120k）

Compact：
  compact_messages_via_llm 保留完整 preserved tail round
  compact 后 messages.jsonl 仍保留全量历史
  下轮 load_history 只看 boundary 之后视图

LLM 调用：
  filter_map(...ok()) 改为显式 warn + 不静默丢弃
  token_budget 从 settings/模型配置读取，不写死 4096

入口：
  agentName 从前端透传到 ChatTurnRequest
```

---

## 4. 文件变更清单

| 动作 | 文件 | 变更内容 |
|---|---|---|
| **修改** | `src-tauri/src/storage/file_store/types.rs` | StoredMessage 加 tool_calls、tool_call_id、name、run_id、schema_version；保留 seq/rev 为 `#[serde(default)] Option` 兼容旧数据 |
| **重写** | `src-tauri/src/storage/file_store/messages.rs` | 去分片，改单文件 UUID dedup；保留旧分片迁移；修复 insert 幂等 |
| **修改** | `src-tauri/src/storage/file_store/mod.rs` | 暴露 v2 API；启动时调用分片迁移 |
| **新建** | `src-tauri/src/runtime/chat/history.rs` | build_chat_history：boundary 截断 + tool pair 双向校验 + round-based 裁剪 |
| **修改** | `src-tauri/src/runtime/chat/compaction.rs` | compact 后保留完整 preserved tail round，不只保留 latest_user |
| **修改** | `src-tauri/src/transport/tauri_commands/chat.rs` | load_history 委托 history.rs；删除 HISTORY_LIMIT=50；filter_map 改显式；agentName 透传 |
| **修改** | `src-tauri/src/commands/chat.rs` | send_message 增加 `agent_name: Option<String>` |
| **修改** | `src-tauri/src/runtime/chat/chat_turn_driver.rs` | token_budget 从 settings 读；agentName 写入 ChatTurnRequest |
| **新建** | `src-tauri/tests/common/mod.rs` | 公共测试 fixture |
| **新建** | `src-tauri/tests/message_storage_v2_test.rs` | 单文件 JSONL、UUID dedup、分片迁移 |
| **新建** | `src-tauri/tests/history_rebuild_test.rs` | compact boundary、tool pair 校验、round 裁剪 |

---

## Phase A：存储单文件化（去分片 + Schema 扩展）

**解决：P0-1、P1-1、P1-5、P1-6、P2-1、P2-2**

### Task A0：测试公共脚手架

**Files:**
- Create: `src-tauri/tests/common/mod.rs`
- Modify: `src-tauri/Cargo.toml`（加 `indexmap = "2"`）

- [ ] **Step 1：创建公共 fixture 模块**

新建 `src-tauri/tests/common/mod.rs`：

```rust
use lotus_app::storage::file_store::types::StoredMessage;

pub fn make_user(id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(), conversation_id: "c1".into(),
        role: "user".into(),
        content: serde_json::json!({"text": text}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: None, tool_call_id: None, name: None,
        run_id: None, schema_version: Some(2),
        seq: None, rev: None,
    }
}

pub fn make_assistant_with_tc(id: &str, tc_id: &str, tool: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(), conversation_id: "c1".into(),
        role: "assistant".into(),
        content: serde_json::json!({"text": ""}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: Some(vec![serde_json::json!({
            "id": tc_id, "type": "function",
            "function": {"name": tool, "arguments": "{}"}
        })]),
        tool_call_id: None, name: None,
        run_id: None, schema_version: Some(2),
        seq: None, rev: None,
    }
}

pub fn make_tool_result(id: &str, tc_id: &str, tool: &str, content: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(), conversation_id: "c1".into(),
        role: "tool".into(),
        content: serde_json::json!({"text": content}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: None,
        tool_call_id: Some(tc_id.into()),
        name: Some(tool.into()),
        run_id: None, schema_version: Some(2),
        seq: None, rev: None,
    }
}

pub fn make_assistant(id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(), conversation_id: "c1".into(),
        role: "assistant".into(),
        content: serde_json::json!({"text": text}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: None, tool_call_id: None, name: None,
        run_id: None, schema_version: Some(2),
        seq: None, rev: None,
    }
}
```

- [ ] **Step 2：Cargo.toml 加依赖**

```toml
[dev-dependencies]
indexmap = "2"
```

- [ ] **Step 3：Commit**

```bash
git add src-tauri/tests/common/mod.rs src-tauri/Cargo.toml
git commit -m "test: add common test fixtures and indexmap dev dependency"
```

---

### Task A1：扩展 StoredMessage Schema

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`
- Test: `src-tauri/tests/message_storage_v2_test.rs`

- [ ] **Step 1：写 schema 兼容性测试**

```rust
// src-tauri/tests/message_storage_v2_test.rs
mod common;
use lotus_app::storage::file_store::types::StoredMessage;

#[test]
fn new_fields_serialize_correctly() {
    let msg = common::make_assistant_with_tc("1", "tc_1", "execute_python");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("toolCalls"));
    assert!(json.contains("tc_1"));
    assert!(!json.contains("\"seq\"")); // seq 不序列化
}

#[test]
fn old_v1_message_deserializes_without_new_fields() {
    let old = r#"{"id":"m1","conversationId":"c1","role":"user","content":{"text":"hi"},"createdAt":"2026-04-24T00:00:00Z"}"#;
    let msg: StoredMessage = serde_json::from_str(old).unwrap();
    assert!(msg.tool_calls.is_none());
    assert!(msg.run_id.is_none());
    assert!(msg.schema_version.is_none());
}
```

- [ ] **Step 2：运行测试，确认失败**

```bash
cd src-tauri && cargo test new_fields_serialize --test message_storage_v2_test -- --nocapture
```

- [ ] **Step 3：修改 StoredMessage**

`src-tauri/src/storage/file_store/types.rs` 中将 `StoredMessage` 改为：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: serde_json::Value,
    pub created_at: String,

    // tool 字段（OpenAI 格式，全部 Option + default，兼容旧数据）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    // 归因与迁移
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,

    // sequence：单调递增，排序稳定性保证（非 dedup 键）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,

    // v1 遗留字段：读取时忽略，写入时不序列化
    #[serde(default, skip_serializing)]
    pub seq: Option<u64>,
    #[serde(rename = "_rev", default, skip_serializing)]
    pub rev: Option<u32>,
}

impl StoredMessage {
    /// 提取消息正文文本，兼容 v1 / v2。
    pub fn text(&self) -> &str {
        self.content.get("text")
            .and_then(|v| v.as_str())
            .or_else(|| self.content.as_str())
            .unwrap_or("")
    }
}
```

- [ ] **Step 4：运行测试，确认通过**

```bash
cd src-tauri && cargo test --test message_storage_v2_test -- --nocapture
```

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/storage/file_store/types.rs src-tauri/tests/message_storage_v2_test.rs
git commit -m "feat(storage): extend StoredMessage with tool_calls/tool_call_id/run_id/schema_version/sequence"
```

---

### Task A2：单文件 JSONL + UUID dedup

**Files:**
- Modify: `src-tauri/src/storage/file_store/messages.rs`
- Test: `src-tauri/tests/message_storage_v2_test.rs`

- [ ] **Step 1：写单文件 insert + read + dedup 测试**

```rust
#[test]
fn insert_and_get_single_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let base = dir.path();
    super::conversations::create_conversation(base, "c1", "T").unwrap();

    let msg = common::make_user("1", "hello");
    insert_message_v2(base, &msg).unwrap();
    insert_message_v2(base, &msg).unwrap(); // 重复 insert

    let msgs = get_messages_v2(base, "c1").unwrap();
    assert_eq!(msgs.len(), 1); // UUID dedup
}

#[test]
fn update_via_same_id_last_wins() {
    let dir = tempfile::TempDir::new().unwrap();
    let base = dir.path();
    super::conversations::create_conversation(base, "c1", "T").unwrap();

    let mut msg = common::make_user("1", "original");
    insert_message_v2(base, &msg).unwrap();
    msg.content = serde_json::json!({"text": "updated"});
    insert_message_v2(base, &msg).unwrap();

    let msgs = get_messages_v2(base, "c1").unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text(), "updated");
}

#[test]
fn messages_ordered_by_sequence_then_created_at() {
    let dir = tempfile::TempDir::new().unwrap();
    let base = dir.path();
    super::conversations::create_conversation(base, "c1", "T").unwrap();

    // 同一秒内多条消息，靠 sequence 稳定排序
    let mut m1 = common::make_user("1", "first");
    m1.created_at = "2026-04-24T00:00:00Z".into();
    m1.sequence = Some(1);
    let mut m2 = common::make_user("2", "second");
    m2.created_at = "2026-04-24T00:00:00Z".into(); // 同一秒
    m2.sequence = Some(2);

    insert_message_v2(base, &m2).unwrap(); // 先插 2
    insert_message_v2(base, &m1).unwrap(); // 后插 1

    let msgs = get_messages_v2(base, "c1").unwrap();
    assert_eq!(msgs[0].text(), "first");  // sequence 小的在前
    assert_eq!(msgs[1].text(), "second");
}
```

- [ ] **Step 2：运行测试，确认失败**

```bash
cd src-tauri && cargo test insert_and_get_single_file --test message_storage_v2_test -- --nocapture
```

- [ ] **Step 3：在 messages.rs 新增单文件函数**

```rust
// src-tauri/src/storage/file_store/messages.rs

fn messages_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("messages.jsonl")
}

/// 追加消息（同 id last-writer-wins）。
pub fn insert_message_v2(base_dir: &Path, msg: &StoredMessage) -> StorageResult<()> {
    let path = messages_path(base_dir, &msg.conversation_id);
    append_jsonl(&path, msg)?;
    // 更新 conv updatedAt
    let conv_path = conv_dir(base_dir, &msg.conversation_id).join("conv.json");
    if conv_path.exists() {
        if let Ok(mut meta) = super::io::read_json_safe::<super::types::ConversationMeta>(&conv_path) {
            meta.updated_at = chrono::Utc::now().to_rfc3339();
            let _ = super::io::atomic_write_json(&conv_path, &meta);
        }
    }
    Ok(())
}

/// 读取所有消息，UUID dedup（last-writer-wins），按 (sequence, created_at) 排序。
pub fn get_messages_v2(base_dir: &Path, conversation_id: &str) -> StorageResult<Vec<StoredMessage>> {
    let path = messages_path(base_dir, conversation_id);
    let all: Vec<StoredMessage> = read_jsonl(&path).unwrap_or_default();
    // last-writer-wins by id
    let mut map: std::collections::HashMap<String, StoredMessage> = std::collections::HashMap::new();
    for msg in all {
        map.insert(msg.id.clone(), msg);
    }
    let mut result: Vec<StoredMessage> = map.into_values().collect();
    // (sequence, created_at) 双键排序，保证同毫秒消息顺序稳定
    result.sort_by(|a, b| {
        let seq_cmp = a.sequence.unwrap_or(u64::MAX)
            .cmp(&b.sequence.unwrap_or(u64::MAX));
        if seq_cmp != std::cmp::Ordering::Equal {
            return seq_cmp;
        }
        a.created_at.cmp(&b.created_at)
    });
    Ok(result)
}

/// 读取最近 N 条（dedup 后取末尾）。
pub fn get_recent_messages_v2(
    base_dir: &Path,
    conversation_id: &str,
    limit: usize,
) -> StorageResult<Vec<StoredMessage>> {
    let all = get_messages_v2(base_dir, conversation_id)?;
    let start = all.len().saturating_sub(limit);
    Ok(all.into_iter().skip(start).collect())
}
```

- [ ] **Step 4：运行测试，确认通过**

```bash
cd src-tauri && cargo test --test message_storage_v2_test -- --nocapture
```

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/storage/file_store/messages.rs
git commit -m "feat(storage): single-file JSONL with UUID last-writer-wins and sequence-stable sort"
```

---

### Task A3：旧分片迁移 + AppStorage v2 API

**Files:**
- Modify: `src-tauri/src/storage/file_store/messages.rs`
- Modify: `src-tauri/src/storage/file_store/mod.rs`

- [ ] **Step 1：写迁移测试**

```rust
#[test]
fn migrates_old_shards_to_single_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let base = dir.path();
    super::conversations::create_conversation(base, "c1", "T").unwrap();
    let conv_dir = base.join("conversations").join("c1");

    // 写旧格式分片（含 \t✓ 尾行）
    for (i, (id, text)) in [("m1","hello"),("m2","world")].iter().enumerate() {
        let v = serde_json::json!({
            "seq": i+1, "_rev": 1, "id": id, "conversationId": "c1",
            "role": "user", "content": {"text": text},
            "createdAt": format!("2026-04-24T00:00:0{}Z", i)
        });
        let line = format!("{}\t✓\n", v);
        std::fs::write(conv_dir.join(format!("messages.{}.jsonl", i+1)), &line).unwrap();
    }
    std::fs::write(conv_dir.join("_current"), "2:3").unwrap();

    // 验证 read_jsonl 能处理 \t✓ 尾行
    let records: Vec<serde_json::Value> =
        super::io::read_jsonl(&conv_dir.join("messages.1.jsonl")).unwrap();
    assert_eq!(records.len(), 1, "read_jsonl 必须能处理 \\t✓ 尾行");

    migrate_shards_to_single_file(base, "c1").unwrap();

    assert!(!conv_dir.join("messages.1.jsonl").exists());
    assert!(!conv_dir.join("messages.2.jsonl").exists());
    assert!(!conv_dir.join("_current").exists());

    let msgs = get_messages_v2(base, "c1").unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].id, "m1");
}
```

- [ ] **Step 2：实现迁移函数**

```rust
pub fn migrate_shards_to_single_file(base_dir: &Path, conversation_id: &str) -> StorageResult<()> {
    let dir = conv_dir(base_dir, conversation_id);
    let mut shards: Vec<(u32, PathBuf)> = std::fs::read_dir(&dir)
        .map_err(StorageError::Io)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("messages.") && name.ends_with(".jsonl") {
                let mid = &name["messages.".len()..name.len()-".jsonl".len()];
                mid.parse::<u32>().ok().map(|n| (n, e.path()))
            } else { None }
        })
        .collect();
    if shards.is_empty() { return Ok(()); }
    shards.sort_by_key(|(n, _)| *n);

    let target = messages_path(base_dir, conversation_id);
    let mut seq: u64 = 0;
    for (_, shard) in &shards {
        let records: Vec<serde_json::Value> =
            read_jsonl::<serde_json::Value>(shard).unwrap_or_default();
        for v in records {
            seq += 1;
            let msg = StoredMessage {
                id: v["id"].as_str().unwrap_or("unknown").into(),
                conversation_id: conversation_id.into(),
                role: v["role"].as_str().unwrap_or("user").into(),
                content: v["content"].clone(),
                created_at: v["createdAt"].as_str()
                    .or_else(|| v["created_at"].as_str())
                    .unwrap_or("").into(),
                tool_calls: None, tool_call_id: None, name: None,
                run_id: None, schema_version: None,
                sequence: Some(seq),
                seq: None, rev: None,
            };
            append_jsonl(&target, &msg).map_err(StorageError::Io)?;
        }
        std::fs::remove_file(shard).ok();
    }
    std::fs::remove_file(dir.join("_current")).ok();
    std::fs::remove_file(dir.join("_current.tmp")).ok();
    Ok(())
}
```

- [ ] **Step 3：AppStorage::new 中调用迁移，暴露 v2 API**

```rust
// mod.rs AppStorage::new 末尾
let storage = Self { /* ... */ };
storage.run_startup_migrations();
Ok(storage)

fn run_startup_migrations(&self) {
    let conv_base = self.base_dir.join("conversations");
    if let Ok(entries) = std::fs::read_dir(&conv_base) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let conv_id = entry.file_name().to_string_lossy().to_string();
            let has_shards = entry.path().join("messages.1.jsonl").exists();
            let has_new = entry.path().join("messages.jsonl").exists();
            if has_shards && !has_new {
                if let Err(e) = messages::migrate_shards_to_single_file(
                    &self.base_dir, &conv_id,
                ) {
                    log::warn!("[migration] shard→single for {} failed: {}", conv_id, e);
                }
            }
        }
    }
}

// 新增 v2 公开 API
pub fn insert_chat_message_record(&self, msg: &StoredMessage) -> Result<()> {
    let _lock = self.write_lock.lock().unwrap();
    messages::insert_message_v2(&self.base_dir, msg)?;
    Ok(())
}

pub fn get_messages_v2(&self, conversation_id: &str) -> Result<Vec<StoredMessage>> {
    messages::get_messages_v2(&self.base_dir, conversation_id)
}
```

- [ ] **Step 4：运行全量测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/storage/file_store/messages.rs src-tauri/src/storage/file_store/mod.rs src-tauri/tests/message_storage_v2_test.rs
git commit -m "feat(storage): auto-migrate shards to single messages.jsonl, expose v2 API"
```

---

## Phase B：Runtime 接管历史加载（history.rs）

**解决：P0-2、P0-3、P1-2**

### Task B1：history.rs — boundary 截断 + tool pair 校验 + round 裁剪

**Files:**
- Create: `src-tauri/src/runtime/chat/history.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/tests/history_rebuild_test.rs`

- [ ] **Step 1：写核心测试**

```rust
// src-tauri/tests/history_rebuild_test.rs
mod common;
use lotus_app::runtime::chat::history::{build_chat_history, HistoryConfig};

#[test]
fn valid_tool_pair_passes_through() {
    let stored = vec![
        common::make_user("1", "hello"),
        common::make_assistant_with_tc("2", "tc_1", "exec"),
        common::make_tool_result("3", "tc_1", "exec", "result"),
        common::make_assistant("4", "done"),
    ];
    let history = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    assert_eq!(history.len(), 4);
}

#[test]
fn orphan_tool_dropped() {
    let stored = vec![
        common::make_user("1", "hi"),
        common::make_tool_result("2", "tc_99", "exec", "orphan"), // 无对应 assistant
        common::make_assistant("3", "ok"),
    ];
    let history = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    assert!(!history.iter().any(|m| m.role == "tool"));
}

#[test]
fn assistant_without_result_tool_calls_cleared() {
    // assistant 声明 tool_calls 但无对应 tool result → tool_calls 置空
    let stored = vec![
        common::make_user("1", "hi"),
        common::make_assistant_with_tc("2", "tc_1", "exec"), // tc_1 无 result
        common::make_assistant("3", "done"),
    ];
    let history = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    // 无孤立 tool，assistant 的 tool_calls 应被清空
    let asst = history.iter().find(|m| m.role == "assistant" && m.tool_calls.is_some());
    assert!(asst.is_none(), "无对应 result 的 tool_calls 应被清空");
}

#[test]
fn round_based_trim_respects_max_rounds() {
    // 5 轮 user→assistant，max_rounds=2，只保留最后 2 轮
    let mut stored = Vec::new();
    for i in 0..5u64 {
        stored.push(common::make_user(&(i*2).to_string(), &format!("q{}", i)));
        stored.push(common::make_assistant(&(i*2+1).to_string(), &format!("a{}", i)));
    }
    let config = HistoryConfig { char_budget: usize::MAX, max_rounds: 2 };
    let history = build_chat_history(&stored, None, &config).unwrap();
    // 保留最后 2 个 user 开头的 round
    assert_eq!(history.iter().filter(|m| m.role == "user").count(), 2);
}
```

- [ ] **Step 2：运行测试，确认失败**

```bash
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture
```

- [ ] **Step 3：实现 history.rs**

新建 `src-tauri/src/runtime/chat/history.rs`：

```rust
use crate::llm::streaming::ChatMessage;
use crate::storage::file_store::types::StoredMessage;
use crate::runtime::chat::compaction::CompactBoundaryRecord;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    /// 字符预算上限（约 token * 4），超出时整 round 丢弃。
    pub char_budget: usize,
    /// 最多保留多少个完整 round。
    /// round = 一条 user 消息 + 其后所有 assistant/tool，直到下一条 user。
    /// 不是条数上限，避免截断 tool pair 导致 OpenAI 400。
    pub max_rounds: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self { Self { char_budget: 120_000, max_rounds: 30 } }
}

/// 从 StoredMessage 列表构建合法的 OpenAI ChatMessage 历史。
pub fn build_chat_history(
    stored: &[StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
    config: &HistoryConfig,
) -> anyhow::Result<Vec<ChatMessage>> {
    // 1. compact boundary 截断：只返回 boundary 之后视图
    let relevant = apply_boundary(stored, boundary);

    // 2. StoredMessage → ChatMessage
    let mut messages: Vec<ChatMessage> = relevant.iter().map(stored_to_chat).collect();

    // 3. 双向过滤非法 OpenAI tool pair
    messages = filter_invalid_tool_pairs(messages);

    // 4. round-based 裁剪
    messages = trim_to_budget(messages, config);

    // 5. compact summary 插在开头
    if let Some(b) = boundary {
        if !b.summary_text.is_empty() {
            messages.insert(0, ChatMessage::text(
                "user",
                format!("<context>\n{}\n</context>", b.summary_text),
            ));
        }
    }

    Ok(messages)
}

fn apply_boundary<'a>(
    stored: &'a [StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
) -> &'a [StoredMessage] {
    if let Some(b) = boundary {
        if let Some(tail_id) = b.tail_message_id.as_deref().filter(|s| !s.is_empty()) {
            match stored.iter().position(|m| m.id == tail_id) {
                Some(idx) => return &stored[idx..],
                None => {
                    // tail_id 找不到：降级返回全量，不返回空切片
                    log::warn!("[history] compact boundary tail_id='{}' not found, using full history", tail_id);
                    return stored;
                }
            }
        }
    }
    stored
}

fn stored_to_chat(m: &StoredMessage) -> ChatMessage {
    ChatMessage {
        role: m.role.clone(),
        content: m.text().to_string(),
        tool_calls: m.tool_calls.as_ref().map(|tcs| {
            tcs.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
        }),
        tool_call_id: m.tool_call_id.clone(),
        name: m.name.clone(),
    }
}

/// 双向过滤非法 tool pair（满足 OpenAI 规则 3/4/5）。
fn filter_invalid_tool_pairs(msgs: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let responded_ids: HashSet<String> = msgs.iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.tool_call_id.clone())
        .collect();

    let declared_ids: HashSet<String> = msgs.iter()
        .filter(|m| m.role == "assistant")
        .flat_map(|m| m.tool_calls.iter().flatten().map(|tc| tc.id.clone()))
        .collect();

    msgs.into_iter().filter(|m| {
        // 规则 3：孤立 role=tool 丢弃
        if m.role == "tool" {
            return m.tool_call_id.as_ref()
                .map(|id| declared_ids.contains(id))
                .unwrap_or(false);
        }
        true
    }).map(|mut m| {
        // 规则 4：assistant.tool_calls 有未响应的 → 清空 tool_calls（降级保留文本）
        if m.role == "assistant" {
            if let Some(ref tcs) = m.tool_calls.clone() {
                let all_responded = tcs.iter().all(|tc| responded_ids.contains(&tc.id));
                if !all_responded {
                    m.tool_calls = None;
                }
            }
        }
        m
    }).collect()
}

/// round-based 裁剪：以 user 消息为 round 起点，整 round 保留或丢弃。
fn trim_to_budget(msgs: Vec<ChatMessage>, config: &HistoryConfig) -> Vec<ChatMessage> {
    let rounds = split_into_rounds(&msgs);
    let mut kept: Vec<&[ChatMessage]> = rounds.iter().map(|r| r.as_slice()).collect();

    loop {
        let total_chars: usize = kept.iter()
            .flat_map(|r| r.iter())
            .map(|m| m.content.len())
            .sum();
        if kept.len() <= config.max_rounds && total_chars <= config.char_budget {
            break;
        }
        if kept.is_empty() { break; }
        kept.remove(0); // 丢弃最老的整 round
    }

    kept.into_iter().flat_map(|r| r.iter().cloned()).collect()
}

fn split_into_rounds(msgs: &[ChatMessage]) -> Vec<Vec<ChatMessage>> {
    let mut rounds: Vec<Vec<ChatMessage>> = Vec::new();
    let mut current: Vec<ChatMessage> = Vec::new();

    for msg in msgs {
        if msg.role == "user" && !current.is_empty() {
            rounds.push(current);
            current = Vec::new();
        }
        current.push(msg.clone());
    }
    if !current.is_empty() {
        rounds.push(current);
    }
    rounds
}
```

- [ ] **Step 4：在 mod.rs 导��**

```rust
// runtime/chat/mod.rs
pub mod history;
```

- [ ] **Step 5：运行测试**

```bash
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/chat/history.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/history_rebuild_test.rs
git commit -m "feat(runtime): history.rs — boundary truncation, OpenAI tool pair validation, round-based trim"
```

---

### Task B2：load_history 切换到 history.rs

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1：替换 load_history 实现**

将 `TauriLegacyTurnExecutor::load_history`（`chat.rs:1173`）改为：

```rust
async fn load_history(
    &self,
    conversation_id: &str,
) -> Result<Vec<serde_json::Value>, TurnError> {
    // 使用 v2 API 加载全量消息（已含 tool 消息）
    let stored = self.services.db
        .get_messages_v2(conversation_id)
        .map_err(|e| TurnError::PersistenceError(format!("load_history failed: {}", e)))?;

    let latest_boundary = self.services.db
        .list_compact_boundaries(conversation_id)
        .map_err(|e| TurnError::PersistenceError(format!("load boundaries failed: {}", e)))?
        .into_iter()
        .last();

    let config = crate::runtime::chat::history::HistoryConfig::default();
    let chat_msgs = crate::runtime::chat::history::build_chat_history(
        &stored,
        latest_boundary.as_ref(),
        &config,
    ).map_err(|e| TurnError::PersistenceError(e.to_string()))?;

    log::info!(
        "[load_history] conv={} loaded {} messages via history.rs (rounds≤{}, budget≤{})",
        conversation_id, chat_msgs.len(), config.max_rounds, config.char_budget,
    );

    // 转为 serde_json::Value 供 turn driver 的 initial_messages 使用
    Ok(chat_msgs.iter().map(|m| {
        let mut v = serde_json::json!({"role": m.role, "content": m.content});
        if let Some(tcs) = &m.tool_calls {
            if let Ok(val) = serde_json::to_value(tcs) {
                v["toolCalls"] = val;
            }
        }
        if let Some(id) = &m.tool_call_id { v["toolCallId"] = id.clone().into(); }
        if let Some(n) = &m.name { v["name"] = n.clone().into(); }
        v
    }).collect())
}
```

删除旧的 `HISTORY_LIMIT` 常量和 `build_history_from_compact_boundary` 直接调用（保留函数本身，因为可能有其他调用）。

- [ ] **Step 2：修复 filter_map 静默丢弃（P0-5）**

在 `chat.rs:294-299` 的 `run_llm_step` 里：

```rust
// 旧：
let chat_messages: Vec<ChatMessage> = input
    .messages
    .iter()
    .filter_map(|v| serde_json::from_value(v.clone()).ok())
    .collect();

// 改为：
let chat_messages: Vec<ChatMessage> = input
    .messages
    .iter()
    .filter_map(|v| {
        serde_json::from_value(v.clone()).map_err(|e| {
            log::warn!("[run_llm_step] Failed to deserialize message: {} — value: {}",
                e, serde_json::to_string(v).unwrap_or_default());
            e
        }).ok()
    })
    .collect();

if chat_messages.len() < input.messages.len() {
    log::error!(
        "[run_llm_step] conv={} DROPPED {} messages during deserialization — context may be incomplete",
        input.conversation_id,
        input.messages.len() - chat_messages.len()
    );
}
```

- [ ] **Step 3：运行全量测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(history): load_history delegates to history.rs, replace HISTORY_LIMIT=50, fix silent message drop"
```

---

## Phase C：修复 compact tail round（P0-4）

**解决：P0-4**

### Task C1：compact 后保留完整 tail round

**Files:**
- Modify: `src-tauri/src/runtime/chat/compaction.rs`

- [ ] **Step 1：理解现状**

读 `compaction.rs:289-318`，当前 `compact_messages_via_llm` 的消息结构：

```rust
// 当前（有问题）：
[compact_boundary_system, summary_user, latest_user_message]
```

问题：如果 latest_user_message 前面还有一个未完成的 tool round（assistant.tool_calls + tool_results），它被丢弃，导致 tool pair 不完整。

- [ ] **Step 2：改为保留完整 tail round**

在 `compact_messages_via_llm` 中，找出 latest user message，并向后包含其后所有消息（保留完整 tail round）：

```rust
// 修改后逻辑：
// 1. 找最后一条 user 消息的 index
// 2. 取该 user 消息及其后所有消息（完整 tail round）
// 3. 新消息结构：[compact_boundary, summary_user, ...tail_round]

let tail_start = messages.iter().rposition(|m| {
    m.get("role").and_then(|v| v.as_str()) == Some("user")
        && m.get("isCompactSummary").is_none()
}).unwrap_or(messages.len().saturating_sub(1));

let tail_round = messages[tail_start..].to_vec();

let new_messages = vec![
    // compact boundary 标记
    serde_json::json!({"role": "system", "content": "[compact_boundary]", "isCompactBoundary": true}),
    // LLM 生成的摘要
    serde_json::json!({"role": "user", "content": format!("<context>\n{}\n</context>", summary_text), "isCompactSummary": true}),
]
.into_iter()
.chain(tail_round)
.collect::<Vec<_>>();
```

- [ ] **Step 3：写测试验证**

在 `history_rebuild_test.rs` 新增：

```rust
#[test]
fn compact_preserves_tail_tool_round() {
    // compact 后如果 tail 有完整 tool round，必须保留
    let stored = vec![
        common::make_user("1", "q1"),
        common::make_assistant("2", "a1"),
        // compact 后保留的 tail：
        common::make_user("3", "q2"),
        common::make_assistant_with_tc("4", "tc_1", "exec"),
        common::make_tool_result("5", "tc_1", "exec", "result"),
    ];
    // 模拟带有 compact boundary 的场景
    use lotus_app::runtime::chat::compaction::CompactBoundaryRecord;
    let boundary = CompactBoundaryRecord {
        id: "b1".into(),
        conversation_id: "c1".into(),
        trigger: lotus_app::runtime::chat::compaction::CompactTrigger::Auto,
        pre_tokens: 1000, post_tokens: 100,
        messages_summarized: 2,
        created_at: "2026-04-24T00:00:00Z".into(),
        summary_text: "摘要".into(),
        tail_message_id: Some("3".into()), // boundary 从 user "3" 开始
    };
    let history = build_chat_history(&stored, Some(&boundary), &HistoryConfig::default()).unwrap();
    // 应包含：summary + q2 + assistant(tc_1) + tool_result
    assert!(history.iter().any(|m| m.role == "tool"), "tool result 必须保留");
    assert!(history.iter().any(|m| m.tool_calls.is_some()), "tool_calls 必须保留");
}
```

- [ ] **Step 4：运行测试**

```bash
cd src-tauri && cargo test compact_preserves_tail --test history_rebuild_test -- --nocapture
```

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/chat/compaction.rs
git commit -m "fix(compact): preserve complete tail tool round after compact, not just latest_user"
```

---

## Phase D：修复配置与入口问题（P1-3、P1-4）

### Task D1：token_budget 从配置读取 + agentName 透传

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1：token_budget 改为从 LLM settings 读取**

`chat_turn_driver.rs:666` 的：

```rust
token_budget: overrides.token_budget.unwrap_or(4096),
```

改为：

```rust
token_budget: overrides.token_budget
    .or_else(|| {
        // 从 llm_settings 里读模型的 max_output_tokens
        llm_settings.max_tokens.map(|t| t as usize)
    })
    .unwrap_or(8192), // 默认值从 4096 改为 8192，覆盖主流模型
```

- [ ] **Step 2：agentName 加入 Rust command**

`commands/chat.rs:21-31` 改为：

```rust
#[tauri::command]
pub async fn send_message(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
    permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
    agent_name: Option<String>,  // 新增
) -> Result<(), String> {
    adapter
        .send_message(conversation_id, content, file_ids, permission_mode, agent_name)
        .await
}
```

`transport/tauri_commands/chat.rs` 中 `TauriChatCommandAdapter::send_message` 签名同步增加 `agent_name: Option<String>`，并写入 `ChatTurnRequest`。

- [ ] **Step 3：运行全量测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 4：Commit**

注意：`src-tauri/src/transport/tauri_commands/chat.rs` 与本计划文件当前同时承载
`Task B2` / `Task D1` 的改动；这里提交时只纳入 `agent_name` 透传和 D1 相关计划更新，
不要把 `load_history` / `filter_map` 的 B2 变更混进本 commit。

```bash
git add src-tauri/src/plugin/builtin/skills/daily_assistant.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/commands/chat.rs \
        src-tauri/src/runtime/tools/catalog.rs \
        src-tauri/tests/agent_registry_test.rs \
        src-tauri/tests/review_session_id_newtype_propagation_test.rs \
        src-tauri/tests/review_agent_name_passthrough_wiring_test.rs
git add -p src-tauri/src/transport/tauri_commands/chat.rs
git add -p docs/superpowers/plans/2026-04-24-message-storage-architecture-fix-plan.md
git commit -m "fix(chat): raise daily token budget to 8192 and preserve agent_name in runtime request"
```

---

## 5. 测试矩阵

| 测试文件 | 覆盖内容 |
|---|---|
| `message_storage_v2_test.rs` | 单文件 JSONL、UUID dedup、旧分片迁移、sequence 稳定排序、v1 兼容 |
| `history_rebuild_test.rs` | compact boundary、孤立 tool 过滤、assistant tool_calls 双向校验、round-based 裁剪、compact tail round 保留 |
| `tests/common/mod.rs` | 公共 fixture（make_user/make_assistant_with_tc/make_tool_result/make_assistant） |

---

## 6. 执行顺序

```
Task A0（测试脚手架）
    ↓
Phase A（存储单文件化 + Schema 扩展）
    ↓  cargo test 全绿
Phase B（history.rs + load_history 切换）
    ↓  cargo test 全绿
Phase C（compact tail round 修复）
    ↓  cargo test 全绿
Phase D（token_budget + agentName）
    ↓  cargo test 全绿
合并
```

---

## 7. 非目标（本轮不做）

- 前端消息大改版相关：乐观消息 id 对齐、message:updated 非 active 对话、streaming:done messageId、MessageContent toolCalls 字段、切换对话闪空 → **等前端大改版统一处理**
- SessionMessageStore（session 级 mutableMessages）→ 前端改版后架构更清晰再引入
- ChatMessage.id 字段 → 同上，依赖 SessionMessageStore 确定所有权
- 不迁移到 Anthropic ContentBlock 格式
- 不移动 `~/.renlijia/` 根目录
- 不引入 SQLite
- 不做 reply/引用消息功能
- 不合并 subagent transcript 到主对话 JSONL

---

## 8. 执行前必须校验的"已实现"功能

> ⚠️ 以下功能基于代码 grep 推断为"已实现"，但**未经测试验证**。
> 执行计划前，先运行以下校验测试。若失败，说明实现不完整，需要补充实现后再继续。

### 校验步骤

- [ ] **Step 1：校验 tool transcript 持久化**

```bash
cd src-tauri && cargo test persist_iteration_assistant_message \
  persist_tool_messages -- --nocapture 2>&1 | grep -E "PASS|FAIL|error"
```

若无对应测试，手动写一个快速验证：
检查 `chat_turn_driver.rs:1092-1111` 调用路径是否在实际 tool round 执行后被触发，
检查 `persist_iteration_assistant_message` 是否真正写入 DB（检查 `insert_message` 是否被调用）。

- [ ] **Step 2：校验历史重建包含 tool 消息**

```bash
cd src-tauri && cargo test build_history_from_compact_boundary -- --nocapture
```

预期：`build_history_from_compact_boundary` 测试覆盖 `role=tool` 消息被正确重建为 `{role:"tool", toolCallId:..., name:...}` 格式。

若无测试，检查 `chat.rs:114-184` 的 `role == "tool"` 分支是否在旧的 `get_recent_messages`（分片方案）下能读到 tool 消息——**注意：旧的 `insert_message` 只接受 `role` 和 `content` 参数，`persist_tool_messages` 用的仍是旧接口，tool 消息能被正确存入并读回吗？**

- [ ] **Step 3：校验旧接口存储 tool 消息的完整性**

读 `chat.rs:832-844`：`persist_tool_messages` 调用的是 `self.services.db.insert_message(msg_id, conversation_id, "tool", content_json)`。

验证：`insert_message` 存的 `content_json` 是 `{"toolCallId":..., "name":..., "content":...}` 格式，而 `build_history_from_compact_boundary` 里读取 tool 消息时（`chat.rs:118-144`）取的是 `content.toolCallId`、`content.name`、`content.content`——**两者格式必须一致，否则历史重建拿到空值**。

```bash
# 确认字段名一致性
grep -n "toolCallId\|toolName\|tool_call_id" src-tauri/src/transport/tauri_commands/chat.rs | head -30
```

若发现不一致（比如存的是 `toolCallId` 但读的是 `tool_call_id`），记录为 Bug 并在 Phase B Task B2 中一并修复。

| 功能 | 推断状态 | 校验结果（执行时填写）|
|---|---|---|
| assistant with tool_calls 持久化 | 推断已实现 | ⬜ 待校验 |
| tool result 消息持久化 | 推断已实现 | ⬜ 待校验 |
| 历史重建支持 role=tool | 推断已实现 | ⬜ 待校验 |
| persist_tool_messages 字段名与 build_history 一致 | 推断一致 | ⬜ 待校验 |
| compact_boundaries.jsonl 加载正确 | 推断已实现 | ⬜ 待校验 |
| 前端 MessageRole 包含 'tool' | 已在代码里确认 | ✅ 已确认 |
