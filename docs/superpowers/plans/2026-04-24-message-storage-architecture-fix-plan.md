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
  token_budget 保持由 turn overrides / skill token_budget 决定；
  daily 默认 skill 提升到 8192，不依赖仓库里并不存在的 llm_settings.max_tokens 字段

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
| **修改** | `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `ChatTurnRequest` 增加 `agent_name` 字段 |
| **修改** | `src-tauri/src/plugin/builtin/skills/daily_assistant.rs` | daily 默认 `token_budget` 从 4096 提升到 8192 |
| **修改** | `src-tauri/tests/common.rs` | 复用现有公共测试模块并补充 message storage/history fixture |
| **新建** | `src-tauri/tests/message_storage_v2_test.rs` | 单文件 JSONL、UUID dedup、分片迁移 |
| **新建** | `src-tauri/tests/history_rebuild_test.rs` | compact boundary、tool pair 校验、round 裁剪 |

---

## Phase A：存储单文件化（去分片 + Schema 扩展）

**解决：P0-1、P1-1、P1-5、P1-6、P2-1、P2-2**

### Task A0：测试公共脚手架

**Files:**
- Modify: `src-tauri/tests/common.rs`（补充 message storage/history fixture）

- [x] **Step 1：创建公共 fixture 模块**

修改现有 `src-tauri/tests/common.rs`，追加以下 fixture：

```rust
use app_lib::storage::file_store::types::StoredMessage;

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

- [x] **Step 3：Commit**

```bash
git add src-tauri/tests/common.rs
git commit -m "test: extend common test fixtures for message storage and history"
```

---

### Task A1：扩展 StoredMessage Schema

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`
- Test: `src-tauri/tests/message_storage_v2_test.rs`

- [x] **Step 1：写 schema 兼容性测试**

```rust
// src-tauri/tests/message_storage_v2_test.rs
mod common;
use app_lib::storage::file_store::types::StoredMessage;

#[test]
fn new_fields_serialize_correctly() {
    let mut msg = common::make_tool_result("1", "tc_1", "execute_python", "done");
    msg.run_id = Some("run_1".into());
    msg.schema_version = Some(2);
    msg.sequence = Some(42);
    // A2 完成前，旧分片存储仍依赖 seq/_rev 落盘参与 dedup/update。
    msg.seq = Some(7);
    msg.rev = Some(3);

    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["toolCallId"], "tc_1");
    assert_eq!(json["name"], "execute_python");
    assert_eq!(json["runId"], "run_1");
    assert_eq!(json["schemaVersion"], 2);
    assert_eq!(json["sequence"], 42);
    assert_eq!(json["seq"], 7);
    assert_eq!(json["_rev"], 3);
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

- [x] **Step 2：运行测试，确认失败**

```bash
cd src-tauri && cargo test new_fields_serialize --test message_storage_v2_test -- --nocapture
```

- [x] **Step 3：修改 StoredMessage**

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

    // v1/v1.5 过渡字段：A2 单文件化完成前仍需写回旧分片存储
    // 以维持 dedup/update 语义；A2 落地后再移除写回要求。
    #[serde(default)]
    pub seq: Option<u64>,
    #[serde(rename = "_rev", default)]
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

- [x] **Step 4：补行为回归测试，锁住旧分片语义与顶级字段兼容**

优先在 `src-tauri/tests/message_storage_v2_test.rs` 中补集成测试（避免当前仓库无关的 lib test 编译阻塞）：

```rust
#[test]
fn legacy_shard_records_still_persist_seq_and_rev_for_dedup() {
    let dir = tempfile::TempDir::new().unwrap();
    let base = dir.path();
    app_lib::storage::file_store::conversations::create_conversation(base, "c1", "T").unwrap();

    app_lib::storage::file_store::messages::insert_message(base, "m1", "c1", "user", r#"{"text":"hello"}"#).unwrap();
    app_lib::storage::file_store::messages::update_message_content(base, "m1", "c1", r#"{"text":"updated"}"#).unwrap();

    let msgs = app_lib::storage::file_store::messages::get_messages(base, "c1").unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["content"]["text"], "updated");

    let shard = base.join("conversations").join("c1").join("messages.1.jsonl");
    let raw: Vec<StoredMessage> = app_lib::storage::file_store::io::read_jsonl(&shard).unwrap();
    assert!(raw.iter().all(|m| m.seq.is_some()));
    assert!(raw.iter().all(|m| m.rev.is_some()));
}
```

再补两条兼容回归：

```rust
#[test]
fn missing_seq_records_do_not_collapse_into_one_dedup_bucket() {
    // 模拟回归窗口中已经落盘的坏数据：多条消息缺少 seq/_rev。
    // 期望：读路径不能把它们全部按 seq=0 合并丢失，至少要按 id 区分。
}

#[test]
fn top_level_tool_fields_survive_get_messages_read_path() {
    // 构造带顶级 tool_calls/tool_call_id/name 的 StoredMessage JSONL 记录，
    // 验证 get_messages() / message_to_json() 会优先读顶级字段，
    // 不会因为 content 里没有 toolCalls/toolCallId 而静默丢失。
}

#[test]
fn repeated_updates_on_missing_seq_records_still_keep_latest_content() {
    // 模拟回归窗口中的缺 seq 旧记录先被 update 到后续 shard，
    // 再次 update 时仍必须沿同一兼容 dedup key（至少 id）继续递增，
    // 不能回到旧 shard 原始版本上导致 newer write 被静默覆盖。
}
```

实现要求同步补充：

- `dedup_messages()` / `get_recent_messages()` 在 `seq` 缺失时，不能统一回退到同一个 `0`；
  过渡期应至少按 `id` 区分，避免坏窗口数据互相吞并。
- `update_message_content()` 遇到 `seq` 缺失的旧坏记录时，不能沿用“统一回退 0”逻辑制造新的碰撞；
  需要沿同一兼容键（至少 `id`）保持 last-writer-wins。
- 对缺 `seq` 记录的重复 update，不能只在命中的单个 shard 内找 `max_rev`；
  必须基于同一兼容 dedup key 跨 shard 取最新版本与全局 `max_rev`，
  否则会把第二次 update 静默回滚成第一次 update。
- `message_to_json()` / `get_messages()` 对 `tool_calls`、`tool_call_id`、`name`
  必须兼容顶级字段与旧 `content` 内嵌字段两种来源；A1 不能只把字段加到 struct 上而不接读链路。

- [x] **Step 5：运行测试，确认通过**

```bash
cd src-tauri && cargo test --test message_storage_v2_test -- --nocapture
```

- [x] **Step 6：Commit**

```bash
git add src-tauri/src/storage/file_store/types.rs src-tauri/src/storage/file_store/messages.rs src-tauri/tests/message_storage_v2_test.rs
git commit -m "feat(storage): extend StoredMessage schema while preserving legacy shard seq/rev semantics"
```

---

### Task A2：单文件 JSONL + UUID dedup

**Files:**
- Modify: `src-tauri/src/storage/file_store/messages.rs`
- Test: `src-tauri/tests/message_storage_v2_test.rs`

- [x] **Step 1：写单文件 insert + read + dedup 测试**

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

- [x] **Step 2：运行测试，确认失败**

```bash
cd src-tauri && cargo test insert_and_get_single_file --test message_storage_v2_test -- --nocapture
```

- [x] **Step 3：在 messages.rs 新增单文件函数**

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

- [x] **Step 4：运行测试，确认通过**

```bash
cd src-tauri && cargo test --test message_storage_v2_test -- --nocapture
```

- [x] **Step 5：Commit（已并入 Phase A 统一提交）**

```bash
git add src-tauri/src/storage/file_store/messages.rs
git commit -m "feat(storage): land transcript v2 with single-file migration and compatibility"
```

---

### Task A3：旧分片迁移 + AppStorage v2 API

**Files:**
- Modify: `src-tauri/src/storage/file_store/messages.rs`
- Modify: `src-tauri/src/storage/file_store/mod.rs`

- [x] **Step 1：写迁移测试**

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

- [x] **Step 2：实现迁移函数**

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

- [x] **Step 3：AppStorage::new 中调用迁移，暴露 v2 API**

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

- [x] **Step 4：运行阶段验证，并记录当前仓库的全量测试阻塞**

```bash
cd src-tauri && cargo test --test message_storage_v2_test -- --nocapture
```

说明：

- 当前仓库仍存在与本阶段无关的全量编译阻塞（例如
  `src/runtime/chat/chat_turn_driver.rs` trait 签名变更引出的
  `src/runtime/session_runtime.rs`、`tests/review_masking_level_settings_test.rs`
  不匹配），因此 A3 完成门槛继续以 `message_storage_v2_test` 为准。
- 不要为了通过本阶段去顺手修复这些无关全量测试错误；待对应后续计划收口时再统一处理。

- [x] **Step 5：Commit**

```bash
git add src-tauri/src/storage/file_store/messages.rs src-tauri/src/storage/file_store/mod.rs src-tauri/tests/message_storage_v2_test.rs
git commit -m "feat(storage): land transcript v2 with single-file migration and compatibility"
```

---

## Phase B：Runtime 接管历史加载（history.rs）

**解决：P0-2、P0-3、P1-2**

### Task B1：history.rs — boundary 截断 + tool pair 校验 + round 裁剪

**Files:**
- Create: `src-tauri/src/runtime/chat/history.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/tests/history_rebuild_test.rs`

- [x] **Step 1：写核心测试**

```rust
// src-tauri/tests/history_rebuild_test.rs
mod common;
use app_lib::runtime::chat::history::{build_chat_history, HistoryConfig};

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

#[test]
fn user_history_with_uploaded_files_preserves_file_hints() {
    // 当前 transport 层已有行为：user.content.files 会展开成
    // "[已上传文件] ... load_file(file_id)" 提示，B1 迁到 history.rs 后不能回归。
}
```

- [x] **Step 2：运行测试，确认失败**

```bash
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture
```

- [x] **Step 3：实现 history.rs**

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
    /// 是否把 user.content.files 展开为面向 LLM 的文件提示。
    /// B1 落地时必须保持当前已有行为默认开启，不能因为迁移到 runtime 层而丢失。
    pub include_uploaded_file_hints: bool,
    /// 当前会话是否已授权工作目录。
    /// 用于保持 build_llm_content 的两套提示文案分支：
    /// 已授权时提示 list_directory / read_workspace_file / search_files；
    /// 未授权时提示 load_file(file_id) + execute_python 变量。
    pub has_authorized_workspace: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            char_budget: 120_000,
            max_rounds: 30,
            include_uploaded_file_hints: true,
            has_authorized_workspace: false,
        }
    }
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
    let mut messages: Vec<ChatMessage> = relevant
        .iter()
        .map(|m| stored_to_chat(m, config))
        .collect();

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

fn stored_to_chat(m: &StoredMessage, config: &HistoryConfig) -> ChatMessage {
    ChatMessage {
        role: m.role.clone(),
        content: build_chat_message_content(m, config),
        tool_calls: normalize_tool_calls(m.tool_calls.as_ref()),
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

补充实现要求：

- `build_chat_message_content()` 对 `role=user` 且 `content.files` 非空时，
  必须复用当前 `build_llm_content(...)` 语义生成 `[已上传文件]` / `load_file(file_id)` 提示；
  不能简单退化成 `m.text()`。
- `build_chat_message_content()` 还必须保留当前按 `has_authorized_workspace` 分支的提示差异，
  不能把“已授权工作目录”的历史提示错误退化成未授权版本。
- `normalize_tool_calls()` 不能直接假设磁盘上的 `tool_calls` 与 `ChatMessage.tool_calls`
  结构完全一致；必须兼容当前仓库已经存在的 `{id,name,arguments}` 形式，以及
  OpenAI 风格 `{id,type,function:{name,arguments}}` 形式，统一归一化成
  `ToolCall { id, name, arguments }`。

- [x] **Step 4：在 mod.rs 导出 history**

```rust
// runtime/chat/mod.rs
pub mod history;
```

- [x] **Step 5：运行测试**

```bash
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture
```

- [x] **Step 6：Commit（已与 B2 合并）**

```bash
git add src-tauri/src/runtime/chat/history.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/history_rebuild_test.rs
git commit -m "feat(history): rebuild chat history from runtime storage pipeline"
```

---

### Task B2：load_history 切换到 history.rs

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [x] **Step 1：替换 load_history 实现**

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

    let config = crate::runtime::chat::history::HistoryConfig {
        has_authorized_workspace,
        ..crate::runtime::chat::history::HistoryConfig::default()
    };
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

同时把 `load_history_via_runtime_history(...)` 改成显式接收
`has_authorized_workspace: bool`，并在 `TauriLegacyTurnExecutor::load_history()`
里复用现有 `load_authorized_workspace(...)` 的结果透传进去，保证
`build_llm_content(...)` 仍能区分“已授权工作区”和“未授权”两套提示文案。

删除旧的 `HISTORY_LIMIT` 常量和 `build_history_from_compact_boundary` 直接调用（保留函数本身，因为可能有其他调用）。

- [x] **Step 2：修复 filter_map 静默丢弃（P0-5）**

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

- [x] **Step 3：运行阶段验证（history_rebuild_test）**

```bash
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture
```

- [x] **Step 4：Commit**

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

- [x] **Step 1：理解现状**

读 `compaction.rs:289-318`，当前 `compact_messages_via_llm` 的消息结构：

```rust
// 当前（有问题）：
[compact_boundary_system, summary_user, latest_user_message]
```

问题：如果 latest_user_message 前面还有一个未完成的 tool round（assistant.tool_calls + tool_results），它被丢弃，导致 tool pair 不完整。

- [x] **Step 2：改为保留完整 tail round**

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

- [x] **Step 3：写测试验证**

优先在现有 compact 相关测试文件（如 `src-tauri/tests/plan_k_llm_compact_test.rs`
或 `src-tauri/tests/review_autocompact_constraints_test.rs`）中直接为
`compact_messages_via_llm()` 补回归测试，而不是绕到 `build_chat_history()`。
这里要锁住的根因是 compact 输出结构本身，而不是 boundary 重建视图。

```rust
#[test]
fn compact_preserves_tail_tool_round() {
    let messages = vec![
        serde_json::json!({ "role": "user", "content": "q1" }),
        serde_json::json!({ "role": "assistant", "content": "a1" }),
        // compact 后必须完整保留的 tail round：
        serde_json::json!({ "role": "user", "content": "q2" }),
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [{ "id": "tc_1", "name": "exec", "arguments": {} }]
        }),
        serde_json::json!({
            "role": "tool",
            "toolCallId": "tc_1",
            "name": "exec",
            "content": "result"
        }),
    ];
    let output = compact_messages_via_llm(messages, "摘要".to_string());

    // 应包含：boundary + summary + q2 + assistant(tc_1) + tool_result
    assert_eq!(output.new_messages.len(), 5);
    assert_eq!(output.new_messages[2]["content"], "q2");
    assert!(output.new_messages[3]["toolCalls"].is_array(), "assistant toolCalls 必须保留");
    assert_eq!(output.new_messages[4]["role"], "tool");
    assert_eq!(output.new_messages[4]["toolCallId"], "tc_1");
}
```

- [x] **Step 4：运行测试**

```bash
cd src-tauri && cargo test --test plan_k_llm_compact_test -- --nocapture
cd src-tauri && cargo test --test review_autocompact_constraints_test -- --nocapture
```

- [x] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/chat/compaction.rs
git commit -m "fix(compact): preserve complete tail round after summary rewrite"
```

---

## Phase D：修复配置与入口问题（P1-3、P1-4）

### Task D1：校正 daily token_budget 默认值 + agentName 透传

**Files:**
- Modify: `src-tauri/src/plugin/builtin/skills/daily_assistant.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

- [x] **Step 1：修正 token_budget 的真实来源设计**

先确认当前仓库事实：

- `ResolvedLlmSettings` / `AppSettings` 里都**没有**
  `max_tokens` / `max_output_tokens` 字段；
- 生产路径的 `token_budget` 真实来源是
  `load_turn_config_overrides()` 返回的 `TurnConfigOverrides.token_budget`
  （即 skill 的 `token_budget()` / runtime patch），不是 `llm_settings`；
- 普通 daily 对话默认命中 `daily-assistant` skill，而它当前把预算写死成了 4096。

因此这里**不要**引入伪代码：

```rust
llm_settings.max_tokens
```

而是改成：

```rust
// src-tauri/src/plugin/builtin/skills/daily_assistant.rs
fn token_budget(&self, _state: &SkillState) -> u32 {
    8192
}
```

`chat_turn_driver.rs` 保持：

```rust
token_budget: overrides.token_budget.unwrap_or(4096),
```

因为这里现在只是兜底分支；真正需要修的是 daily 默认 skill 的 override 值。

- [x] **Step 2：agentName 加入 Rust command 与 ChatTurnRequest**

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

`transport/tauri_commands/chat.rs` 中 `TauriChatCommandAdapter::send_message`
签名同步增加 `agent_name: Option<String>`，并写入 `ChatTurnRequest`。

`src-tauri/src/runtime/chat/chat_turn_driver.rs` 中 `ChatTurnRequest` 增加：

```rust
pub agent_name: Option<String>,
```

并在 `ChatTurnRequest::new(...)` 里默认置为 `None`。

注意：本任务先修复 IPC → Rust command → request 的静默丢失链路，
不在这里额外假设仓库已经完成 `AgentRegistry` 注入和 `allowed_tools`
覆盖逻辑；若后续要恢复 agent definition 约束工具池，应单列任务继续做。

- [x] **Step 3：运行阶段验证**

```bash
cd src-tauri && cargo test --test agent_registry_test -- --nocapture
cd src-tauri && cargo test --test review_session_id_newtype_propagation_test -- --nocapture
cd src-tauri && cargo test --test review_agent_name_passthrough_wiring_test -- --nocapture
```

当前阶段以 D1 直接相关测试为门槛：

- `agent_registry_test`：确认 daily 默认 `token_budget` 已提升到 8192；
- `review_session_id_newtype_propagation_test`：确认 `ChatTurnRequest`
  默认携带 `agent_name = None`；
- `review_agent_name_passthrough_wiring_test`：确认 IPC / command / adapter
  链路不会静默丢失 `agent_name`。

- [x] **Step 4：Commit**

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
| `tests/common.rs` | 公共 fixture（make_user/make_assistant_with_tc/make_tool_result/make_assistant） |

---

## 6. 执行顺序

```
Task A0（测试脚手架）
    ↓
Phase A（存储单文件化 + Schema 扩展）
    ↓  `message_storage_v2_test` 通过
Phase B（history.rs + load_history 切换）
    ↓  `history_rebuild_test` 通过
Phase C（compact tail round 修复）
    ↓  `plan_k_llm_compact_test` / `review_autocompact_constraints_test` 通过
Phase D（token_budget + agentName）
    ↓  `agent_registry_test` / `review_session_id_newtype_propagation_test` / `review_agent_name_passthrough_wiring_test` 通过
完成本计划代码落地
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

## 8. 补充校验记录（已执行）

> 以下校验已在本轮落地后补跑；若校验与旧断言不一致，以更新后的回归测试和当前输出契约为准。

### 校验步骤

- [x] **Step 1：校验 tool transcript 持久化**

```bash
cd src-tauri && cargo test --test review_chat_history_persistence_test -- --nocapture
```

结果：`review_assistant_tool_calls_round_trip` 与 `review_tool_message_round_trip` 通过；
其中 tool 消息的前端返回契约已确认为 `toolResult.{toolCallId,name,content}`。

- [x] **Step 2：校验历史重建包含 tool 消息**

```bash
cd src-tauri && cargo test --test review_chat_history_persistence_test -- --nocapture
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture
```

结果：`review_load_history_restores_tool_messages`、
`review_load_history_restores_legacy_tool_messages` 与 `history_rebuild_test` 通过，
确认 legacy / runtime history 两条读取链路都能恢复 `role=tool` 消息。

- [x] **Step 3：校验旧接口存储 tool 消息的完整性**

```bash
cd src-tauri && cargo test --test review_chat_history_persistence_test -- --nocapture
```

结果：tool message 的磁盘写入、`get_recent_messages()` 前端返回、
以及 `build_history_from_compact_boundary()` 历史重建三段链路均通过。

| 功能 | 推断状态 | 校验结果（执行时填写）|
|---|---|---|
| assistant with tool_calls 持久化 | 已由回归测试覆盖 | ✅ `review_assistant_tool_calls_round_trip` |
| tool result 消息持久化 | 已由回归测试覆盖 | ✅ `review_tool_message_round_trip` |
| 历史重建支持 role=tool | 已由回归测试覆盖 | ✅ `review_load_history_restores_tool_messages` |
| persist_tool_messages 字段名与 build_history 一致 | 已由回归测试覆盖 | ✅ `review_load_history_restores_tool_messages` / `review_load_history_restores_legacy_tool_messages` |
| compact_boundaries.jsonl 加载正确 | 已由回归测试覆盖 | ✅ `history_rebuild_test` |
| 前端 MessageRole 包含 'tool' | 已在代码里确认 | ✅ 已确认 |
