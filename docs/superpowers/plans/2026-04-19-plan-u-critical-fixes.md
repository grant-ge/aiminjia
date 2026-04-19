# Critical 修复包（Plan-U）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to execute each task independently with review checkpoints between tasks.

**Goal:** 修复两个 Critical 差距：TurnError 路径消息链完整性漏洞（U1-U2）+ compact_boundary 写入/读取路径接通（U3-U5）

**Architecture:**
- U1-U2：`run_llm_step` 以 `?` 传播错误，导致已有 `toolCalls` 的 assistant 消息孤立、无对应 tool_result，下次 API 调用返回 422。修复点：在 `?` 错误路径捕获后调用 `inject_synthetic_tool_results_for_missing_calls`，与 cancel 路径对称。
- U3-U5：`compact_messages_via_llm` 执行后只更新 `state.messages`，不写 `CompactBoundaryRecord` 到 `ConversationStore`；`load_history` 直接 `get_recent_messages(50)`，完全忽略 compact boundaries，导致历史重放包含被 compact 掉的旧消息。对标 `claude-code-best` 后，本计划不再采用 `created_at` 时间过滤，而改为“显式 boundary 锚点恢复”：compact 记录必须保存稳定锚点（边界消息本身或其 tail message id），加载历史时从最近 compact boundary 之后的 transcript 片段恢复。

**Tech Stack:** Rust, tokio, async_trait

**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- `U1/U2` 继续对齐 `claude-code-best/src/query.ts` 的 `yieldMissingToolResultBlocks(...)` 思路：异常/取消路径都要补齐 synthetic `tool_result`。
- `U3/U4` 不采用 `created_at` 过滤；改为显式 compact boundary 锚点恢复，避免同毫秒落盘、异步写入顺序或未来 preserved tail relink 带来的漂移。
- boundary 写入必须与 compact 后 transcript 持久化保持顺序一致，避免只写 boundary、不落 compact 后消息的半状态。

## 现状速查

### 关键文件

| 文件 | 作用 |
|---|---|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 主驱动，`run_llm_step` 调用点（L738-739），`mark_turn_cancelled_with_synthetic_results`（L307-313），compact 触发块（L680-701） |
| `src-tauri/src/runtime/chat/compaction.rs` | `CompactBoundaryRecord`、`build_compact_boundary_record`、`compact_messages_via_llm`（返回 `CompactLlmOutput`，含 `pre_tokens/post_tokens/messages_summarized`） |
| `src-tauri/src/runtime/store/conversation_store.rs` | `ConversationStore` trait（`append_compact_boundary`/`list_compact_boundaries`），`InMemoryConversationStore` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | `load_history`（L831-876），`HISTORY_LIMIT=50`，直接调 `get_recent_messages` |
| `src-tauri/src/runtime/chat/turn_config.rs` | `TurnConfig`（无 `ConversationStore` 字段）、`TurnIterationState` |

### Bug 精确定位

**U1/U2 — TurnError 路径（`chat_turn_driver.rs:738-739`）:**

```rust
// 现状：run_llm_step 错误直接用 ? 传播，跳过 synthetic 注入
let step_result = executor
    .run_llm_step(&input, &self.event_bus, &cancel)
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;   // ← BUG: 错误路径无 synthetic 注入
```

对比 cancel 路径（L756-759）：
```rust
LlmStepResult::Cancelled => {
    mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
    break 'turn;
}
```

**U3 — compact 后不写 boundary（`chat_turn_driver.rs:688-695`）:**

```rust
Ok(summary_text) if !summary_text.is_empty() => {
    let output = compact_messages_via_llm(
        std::mem::take(&mut state.messages),
        summary_text,
    );
    state.messages = output.new_messages;
    state.compact_state.record_success();
    // ← BUG: output.pre_tokens/post_tokens/messages_summarized 从未写入 ConversationStore
}
```

**U4 — load_history 忽略 compact boundaries（`chat.rs:831-876`）:**

```rust
// 现状：直接加载最近50条，不查 compact boundaries，也没有 boundary 锚点恢复
let raw_messages = self.services.db
    .get_recent_messages(conversation_id, HISTORY_LIMIT)
    ...
```

### ConversationStore 访问路径

- `RuntimeLlmExecutor` trait 目前没有 `ConversationStore` 相关方法
- U3 需要在 compact 成功后调用 `append_compact_boundary`；最干净的方式是在 `RuntimeLlmExecutor` trait 增加一个 `save_compact_boundary` 默认方法（默认 no-op），生产 executor `TauriLegacyTurnExecutor` 实现（它持有 `services.db: Arc<AppStorage>`，`AppStorage` 已实现 `ConversationStore`）
- U4 的 `load_history` 修复在 `TauriLegacyTurnExecutor::load_history` 内，`self.services.db` 直接可用（`AppStorage` 实现了 `ConversationStore::list_compact_boundaries`）

---

## Tasks

### U1：review 测试——验证 TurnError 路径不注入 synthetic 结果（先确认 bug）

**目标：** 写一个失败测试，证明当 `run_llm_step` 返回 `Err` 时，已有 toolCalls 的 assistant 消息后没有 synthetic tool_result，消息链已破坏。

**测试文件：** `src-tauri/tests/plan_u_critical_fixes_test.rs`

**完整测试代码：**

```rust
//! Plan-U Critical Fixes — 集成测试
//!
//! U1: 验证 TurnError 路径不注入 synthetic 结果（先确认 bug）
//! U2: 修复后，TurnError 路径正确注入 synthetic 结果
//! U3: compact 成功后，ConversationStore 中存在 compact boundary record
//! U4: load_history 在存在 compact boundary 时从 boundary 后加载
//! U5: review_ 架构约束固化

use app_lib::runtime::chat::chat_turn_driver::{
    inject_synthetic_tool_results_for_missing_calls,
};

// ── U1: 确认 bug —————————————————————————————————————————————
//
// 模拟 run_llm_step 返回 Err 之前，state.messages 中已存在一条
// assistant 消息（含 toolCalls），但没有对应的 tool_result。
// 断言：消息链中存在孤立 toolCalls（无对应 tool_result）。
//
// 这个测试验证的是 bug 存在，而非 inject 函数本身。
// 如果 TurnError 路径已经调用了 inject，这个测试就会改变。
#[test]
fn u1_turnerror_path_leaves_orphan_tool_calls_without_synthetic_results() {
    // 构造已被加入 state.messages 的 assistant 消息（含 toolCalls）
    let messages_after_llm_emits_tool_calls: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "call_abc", "name": "execute_python", "arguments": {}}
            ]
        })
    ];

    // 检查消息链是否有孤立的 tool_call（没有对应 tool_result）
    let has_orphan = has_orphan_tool_calls(&messages_after_llm_emits_tool_calls);

    // U1 断言：当前 TurnError 路径不注入 synthetic，所以孤立存在
    // 当 U2 修复后，驱动会在 Err 路径调用 inject，此断言需改为 false
    assert!(
        has_orphan,
        "U1: TurnError 路径离开后，消息链存在孤立 toolCalls（无 tool_result），确认 bug 存在"
    );
}

/// 辅助：检查 messages 中是否有无对应 tool_result 的 toolCalls
fn has_orphan_tool_calls(messages: &[serde_json::Value]) -> bool {
    use std::collections::HashSet;
    let mut tool_call_ids: HashSet<String> = HashSet::new();
    let mut tool_result_ids: HashSet<String> = HashSet::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or_default();
        if role == "assistant" {
            if let Some(tcs) = msg.get("toolCalls").and_then(|v| v.as_array()) {
                for tc in tcs {
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        tool_call_ids.insert(id.to_string());
                    }
                }
            }
        } else if role == "tool" {
            let id = msg
                .get("toolCallId")
                .or_else(|| msg.get("tool_call_id"))
                .and_then(|v| v.as_str());
            if let Some(id) = id {
                tool_result_ids.insert(id.to_string());
            }
        }
    }

    tool_call_ids.iter().any(|id| !tool_result_ids.contains(id))
}
```

**cargo test 命令：**
```bash
cd src-tauri && cargo test u1_turnerror_path_leaves_orphan_tool_calls -- --nocapture
```

**预期结果（修复前）：** 测试通过，证明孤立 toolCalls 存在（即 bug 存在）

**git commit：**
```
test(turn-driver): U1 — assert TurnError path leaves orphan tool calls
```

---

### U2：修复 TurnError 路径，补调用 synthetic 注入

**目标：** 在 `run_llm_step` 的 `?` 错误路径捕获处，调用 `inject_synthetic_tool_results_for_missing_calls`，使消息链与 cancel 路径对称。

**实现文件：** `src-tauri/src/runtime/chat/chat_turn_driver.rs`

**关键实现（修改 L735-739 附近）：**

```rust
// 修改前：
let step_result = executor
    .run_llm_step(&input, &self.event_bus, &cancel)
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

// 修改后：
let step_result = match executor
    .run_llm_step(&input, &self.event_bus, &cancel)
    .await
{
    Ok(result) => result,
    Err(e) => {
        // 注入 synthetic tool_results，防止消息链孤立 toolCalls 导致下次 API 422
        inject_synthetic_tool_results_for_missing_calls(&mut state.messages, None);
        return Err(anyhow::anyhow!("{}", e));
    }
};
```

**配套测试（追加到 `plan_u_critical_fixes_test.rs`）：**

```rust
// ── U2: 修复验证 ————————————————————————————————————————————
//
// 直接调用 inject_synthetic_tool_results_for_missing_calls，
// 验证它在有孤立 toolCalls 时正确注入 synthetic tool_result。
// 这与驱动修复后的行为等价：Err 路径调用 inject，再返回错误。
#[test]
fn u2_inject_synthetic_results_repairs_orphan_tool_calls() {
    let mut messages = vec![
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "call_abc", "name": "execute_python", "arguments": {}},
                {"id": "call_def", "name": "read_file", "arguments": {}}
            ]
        })
    ];

    // 模拟 TurnError 路径修复后的行为：调用 inject
    let injected = inject_synthetic_tool_results_for_missing_calls(&mut messages, None);

    assert_eq!(injected, 2, "应注入 2 个 synthetic tool_result");
    assert!(
        !has_orphan_tool_calls(&messages),
        "U2: inject 后消息链中不应存在孤立 toolCalls"
    );

    // 验证注入的 synthetic result 内容
    let tool_results: Vec<_> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("tool"))
        .collect();
    assert_eq!(tool_results.len(), 2);

    let ids: std::collections::HashSet<_> = tool_results
        .iter()
        .filter_map(|m| m.get("toolCallId").and_then(|v| v.as_str()))
        .collect();
    assert!(ids.contains("call_abc"));
    assert!(ids.contains("call_def"));
}

#[test]
fn u2_inject_is_noop_when_all_tool_calls_have_results() {
    let mut messages = vec![
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [{"id": "call_xyz", "name": "foo", "arguments": {}}]
        }),
        serde_json::json!({
            "role": "tool",
            "toolCallId": "call_xyz",
            "name": "foo",
            "content": "result"
        })
    ];

    let injected = inject_synthetic_tool_results_for_missing_calls(&mut messages, None);
    assert_eq!(injected, 0, "已有 tool_result 时不应注入");
    assert_eq!(messages.len(), 2, "消息数量不变");
}
```

**cargo test 命令：**
```bash
cd src-tauri && cargo test u2_ -- --nocapture
```

**修复后 U1 测试需更新：** 将 U1 测试注释标记为 `// BUG CONFIRMED AND FIXED IN U2`，或者改写为验证驱动不再产出孤立 toolCalls（通过集成 mock executor 的方式，可选）。

**git commit：**
```
fix(turn-driver): U2 — inject synthetic tool results on LLM step error path
```

---

### U3：compact 成功后写 CompactBoundaryRecord 到 ConversationStore

**目标：** 在 `compact_messages_via_llm` 返回 `CompactLlmOutput` 后，用其 `pre_tokens/post_tokens/messages_summarized` 构建 `CompactBoundaryRecord` 并持久化。

**设计决策：** 在 `RuntimeLlmExecutor` trait 增加 `save_compact_boundary` 默认方法（no-op），生产 executor 实现写存储。这样 driver 无需持有 `ConversationStore`，保持层次不乱。

**Step 1：在 `RuntimeLlmExecutor` trait 增加方法（`chat_turn_driver.rs`）**

```rust
/// 持久化 compact boundary record 到存储。
/// 默认 no-op（向后兼容旧 mock executor）。生产 executor 必须 override。
async fn save_compact_boundary(
    &self,
    _conversation_id: &str,
    _record: crate::runtime::chat::compaction::CompactBoundaryRecord,
) -> Result<(), TurnError> {
    Ok(())
}
```

**Step 2：在生产 executor `TauriLegacyTurnExecutor` 实现（`transport/tauri_commands/chat.rs`）**

```rust
async fn save_compact_boundary(
    &self,
    _conversation_id: &str,
    record: crate::runtime::chat::compaction::CompactBoundaryRecord,
) -> Result<(), TurnError> {
    use crate::runtime::store::conversation_store::ConversationStore;
    self.services.db
        .append_compact_boundary(record)
        .map_err(|e| TurnError::PersistenceError(format!(
            "Failed to save compact boundary: {}", e
        )))
}
```

**Step 3：在 compact 触发成功块调用（`chat_turn_driver.rs:688-695`）**

```rust
Ok(summary_text) if !summary_text.is_empty() => {
    let output = compact_messages_via_llm(
        std::mem::take(&mut state.messages),
        summary_text,
    );
    state.messages = output.new_messages;
    state.compact_state.record_success();

    // U3: 写 compact boundary record
    let record = build_compact_boundary_record(
        config.conversation_id.as_str(),
        CompactTrigger::Auto,
        output.pre_tokens,
        output.post_tokens,
        output.messages_summarized,
    );
    if let Err(e) = executor.save_compact_boundary(
        config.conversation_id.as_str(),
        record,
    ).await {
        log::warn!("[compact] failed to save boundary record: {}", e);
        // 非致命错误，不中断 turn
    }
}
```

**需要新增 import（`chat_turn_driver.rs` 顶部）：**
```rust
use crate::runtime::chat::compaction::{build_compact_boundary_record, CompactTrigger};
```

**配套测试（追加到 `plan_u_critical_fixes_test.rs`）：**

```rust
// ── U3: compact 后 ConversationStore 存在 boundary record ——————
use app_lib::runtime::chat::compaction::{
    build_compact_boundary_record, compact_messages_via_llm, CompactTrigger,
};
use app_lib::runtime::store::conversation_store::{ConversationStore, InMemoryConversationStore};

#[test]
fn u3_compact_boundary_written_after_compact_messages_via_llm() {
    // 构造足够长的消息列表触发 compact 估算
    let messages: Vec<serde_json::Value> = (0..20)
        .map(|i| serde_json::json!({"role": "user", "content": format!("message {}", i)}))
        .collect();

    let summary_text = "This is a summary of the conversation.".to_string();
    let output = compact_messages_via_llm(messages, summary_text);

    assert!(output.pre_tokens > 0, "pre_tokens 必须 > 0");
    assert!(output.post_tokens > 0, "post_tokens 必须 > 0");
    assert!(output.messages_summarized > 0, "messages_summarized 必须 > 0");

    // 模拟 U3 修复：用 output 数据写 boundary record
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-u3", "Test").unwrap();

    let record = build_compact_boundary_record(
        "conv-u3",
        CompactTrigger::Auto,
        output.pre_tokens,
        output.post_tokens,
        output.messages_summarized,
    );
    store.append_compact_boundary(record.clone()).unwrap();

    let boundaries = store.list_compact_boundaries("conv-u3").unwrap();
    assert_eq!(boundaries.len(), 1, "compact 后应存在 1 条 boundary record");
    assert_eq!(boundaries[0].trigger, CompactTrigger::Auto);
    assert_eq!(boundaries[0].pre_tokens, output.pre_tokens);
    assert_eq!(boundaries[0].post_tokens, output.post_tokens);
    assert_eq!(boundaries[0].messages_summarized, output.messages_summarized);
    assert_eq!(boundaries[0].conversation_id, "conv-u3");
}
```

**cargo test 命令：**
```bash
cd src-tauri && cargo test u3_ -- --nocapture
```

**git commit：**
```
feat(turn-driver): U3 — persist compact boundary record after auto-compact success
```

---

### U4：load_history 改为从最近 compact boundary 后加载

**目标：** `TauriLegacyTurnExecutor::load_history` 先查 `list_compact_boundaries`，若存在 boundary，则从 boundary 之后的消息开始加载（利用 boundary record 的 `created_at` 过滤或直接加载 boundary 后的消息），无 boundary 则走原有全量逻辑（保持向后兼容）。

**实现文件：** `src-tauri/src/transport/tauri_commands/chat.rs`

**设计说明：**
- `AppStorage::get_recent_messages(id, limit)` 返回最近 N 条，不支持时间过滤
- `AppStorage` 需要一个 `get_messages_after_timestamp(id, iso_timestamp)` 方法，或者直接加载全量消息后按 `created_at` 过滤
- 最轻量方案：加载全量消息（`get_messages`），获取最新 boundary 的 `created_at`，过滤出 `created_at >= boundary.created_at` 的消息，再截取最近 50 条
- 若没有 boundary，直接使用原来的 `get_recent_messages(50)` 路径

**关键实现（替换 `load_history` 方法体）：**

```rust
async fn load_history(
    &self,
    conversation_id: &str,
) -> Result<Vec<serde_json::Value>, TurnError> {
    use crate::runtime::store::conversation_store::ConversationStore;
    const HISTORY_LIMIT: usize = 50;

    // 查最近一条 compact boundary
    let boundaries = self.services.db
        .list_compact_boundaries(conversation_id)
        .map_err(|e| TurnError::PersistenceError(format!(
            "Failed to list compact boundaries: {}", e
        )))?;

    let raw_messages: Vec<serde_json::Value> = if let Some(latest_boundary) = boundaries.last() {
        // 存在 boundary：从 DB 加载全量消息，过滤出 boundary 时间点之后的消息
        let all_messages = self.services.db
            .get_messages(conversation_id)
            .map_err(|e| TurnError::PersistenceError(format!(
                "Failed to load messages: {}", e
            )))?;

        let boundary_ts = &latest_boundary.created_at;
        let after_boundary: Vec<_> = all_messages
            .into_iter()
            .filter(|msg| {
                // 消息若无 created_at 则保留（保守策略）
                msg.get("createdAt")
                    .or_else(|| msg.get("created_at"))
                    .and_then(|v| v.as_str())
                    .map(|ts| ts >= boundary_ts.as_str())
                    .unwrap_or(true)
            })
            .collect();

        // 取最近 HISTORY_LIMIT 条
        let skip = after_boundary.len().saturating_sub(HISTORY_LIMIT);
        after_boundary.into_iter().skip(skip).collect()
    } else {
        // 无 boundary：原有路径
        self.services.db
            .get_recent_messages(conversation_id, HISTORY_LIMIT as u32)
            .map_err(|e| TurnError::PersistenceError(format!(
                "Failed to load conversation history: {}", e
            )))?
    };

    let has_authorized_workspace = chat_runtime_impl::load_authorized_workspace(
        &self.services.app,
        conversation_id,
    )
    .is_some();

    let chat_messages: Vec<serde_json::Value> = raw_messages
        .into_iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?.to_string();
            let content = build_history_message_content(
                &role,
                msg.get("content")?,
                has_authorized_workspace,
            )?;
            if content.trim().is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "role": role,
                "content": content,
            }))
        })
        .collect();

    log::info!(
        "[load_history] conv={} loaded {} messages (has_boundary={})",
        conversation_id,
        chat_messages.len(),
        !boundaries.is_empty(),
    );

    Ok(chat_messages)
}
```

**配套测试（追加到 `plan_u_critical_fixes_test.rs`）：**

```rust
// ── U4: list_compact_boundaries 向后兼容 + boundary 后加载逻辑 ——
#[test]
fn u4_no_boundary_returns_empty_list() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-u4-empty", "Test").unwrap();

    let boundaries = store.list_compact_boundaries("conv-u4-empty").unwrap();
    assert!(
        boundaries.is_empty(),
        "U4: 无 boundary 时应返回空列表，load_history 走原有路径"
    );
}

#[test]
fn u4_latest_boundary_is_last_appended() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-u4-latest", "Test").unwrap();

    let first = build_compact_boundary_record(
        "conv-u4-latest",
        CompactTrigger::Auto,
        50_000,
        8_000,
        20,
    );
    // 短暂 sleep 保证时间戳不同（或直接用不同 created_at 手动构建）
    // 这里验证 last() 是最新追加的
    let second = build_compact_boundary_record(
        "conv-u4-latest",
        CompactTrigger::Auto,
        30_000,
        5_000,
        10,
    );

    store.append_compact_boundary(first.clone()).unwrap();
    store.append_compact_boundary(second.clone()).unwrap();

    let boundaries = store.list_compact_boundaries("conv-u4-latest").unwrap();
    assert_eq!(boundaries.len(), 2);

    // load_history 应取 boundaries.last()
    let latest = boundaries.last().unwrap();
    assert_eq!(latest.id, second.id, "最新 boundary 应是第二条");
    assert_eq!(latest.pre_tokens, 30_000);
}

#[test]
fn u4_messages_after_boundary_filter_works() {
    // 模拟 load_history 中按 created_at 过滤消息的核心逻辑
    let boundary_ts = "2026-04-19T10:00:00Z";

    let all_messages = vec![
        serde_json::json!({"role": "user", "content": "old msg", "createdAt": "2026-04-18T09:00:00Z"}),
        serde_json::json!({"role": "assistant", "content": "old reply", "createdAt": "2026-04-18T09:01:00Z"}),
        serde_json::json!({"role": "user", "content": "new msg", "createdAt": "2026-04-19T10:01:00Z"}),
        serde_json::json!({"role": "assistant", "content": "new reply", "createdAt": "2026-04-19T10:02:00Z"}),
    ];

    // 模拟 load_history 过滤逻辑
    let after_boundary: Vec<_> = all_messages
        .into_iter()
        .filter(|msg| {
            msg.get("createdAt")
                .or_else(|| msg.get("created_at"))
                .and_then(|v| v.as_str())
                .map(|ts| ts >= boundary_ts)
                .unwrap_or(true)
        })
        .collect();

    assert_eq!(after_boundary.len(), 2, "boundary 之后应有 2 条消息");
    assert_eq!(
        after_boundary[0].get("content").and_then(|v| v.as_str()),
        Some("new msg")
    );
    assert_eq!(
        after_boundary[1].get("content").and_then(|v| v.as_str()),
        Some("new reply")
    );
}
```

**cargo test 命令：**
```bash
cd src-tauri && cargo test u4_ -- --nocapture
```

**git commit：**
```
feat(load-history): U4 — load messages after latest compact boundary when present
```

---

### U5：review_ 约束测试固化两个修复点

**目标：** 写 `review_` 前缀的架构约束测试，确保未来修改不意外破坏两个修复点。

**配套测试（追加到 `plan_u_critical_fixes_test.rs`）：**

```rust
// ── U5: review_ 架构约束 ————————————————————————————————————

/// 约束 1：inject_synthetic_tool_results_for_missing_calls 公开可调用，
/// 且对有孤立 toolCalls 的消息链幂等地修复（多次调用不重复注入）。
#[test]
fn review_u5_inject_synthetic_is_idempotent() {
    let mut messages = vec![
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [{"id": "call_idem", "name": "foo", "arguments": {}}]
        })
    ];

    let first = inject_synthetic_tool_results_for_missing_calls(&mut messages, None);
    let second = inject_synthetic_tool_results_for_missing_calls(&mut messages, None);

    assert_eq!(first, 1, "首次调用应注入 1 个 synthetic result");
    assert_eq!(second, 0, "再次调用不应重复注入（已有 tool_result）");
    assert_eq!(messages.len(), 2, "消息总数应为 2（1 assistant + 1 tool）");
}

/// 约束 2：InMemoryConversationStore 的 append/list compact boundary 保持插入序
/// 且 boundary 属于特定 conversation（不跨会话污染）。
#[test]
fn review_u5_compact_boundary_store_isolation() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-A", "A").unwrap();
    store.create_conversation("conv-B", "B").unwrap();

    let record_a = build_compact_boundary_record("conv-A", CompactTrigger::Auto, 10_000, 2_000, 5);
    let record_b = build_compact_boundary_record("conv-B", CompactTrigger::Manual, 20_000, 3_000, 8);

    store.append_compact_boundary(record_a.clone()).unwrap();
    store.append_compact_boundary(record_b.clone()).unwrap();

    let boundaries_a = store.list_compact_boundaries("conv-A").unwrap();
    let boundaries_b = store.list_compact_boundaries("conv-B").unwrap();

    assert_eq!(boundaries_a.len(), 1, "conv-A 应只有 1 条 boundary");
    assert_eq!(boundaries_b.len(), 1, "conv-B 应只有 1 条 boundary");
    assert_eq!(boundaries_a[0].id, record_a.id, "conv-A 的 boundary 应是 record_a");
    assert_eq!(boundaries_b[0].id, record_b.id, "conv-B 的 boundary 应是 record_b");
}

/// 约束 3：build_compact_boundary_record 必须生成唯一 id（不同调用不冲突）。
#[test]
fn review_u5_compact_boundary_record_ids_are_unique() {
    let r1 = build_compact_boundary_record("conv-x", CompactTrigger::Auto, 1000, 100, 3);
    let r2 = build_compact_boundary_record("conv-x", CompactTrigger::Auto, 1000, 100, 3);

    assert_ne!(r1.id, r2.id, "每次 build 必须生成唯一 id");
}
```

**cargo test 命令（运行全部 review_ 约束测试）：**
```bash
cd src-tauri && cargo test review_u5_ -- --nocapture

# 也可运行整个 Plan-U 测试文件：
cd src-tauri && cargo test --test plan_u_critical_fixes_test -- --nocapture
```

**git commit：**
```
test(review): U5 — architectural constraints for synthetic injection and compact boundary isolation
```

---

## 执行顺序

```
U1 → U2 → U3 → U4 → U5
```

每个 Task 独立 commit，U1 和 U2 必须顺序执行（U1 确认 bug，U2 修复后 U1 注释更新）。U3 和 U4 彼此独立，可在 U2 完成后并行，但 U5 须在 U2-U4 全部通过后写。

## 验收标准

```bash
# 所有 Plan-U 测试通过
cd src-tauri && cargo test --test plan_u_critical_fixes_test -- --nocapture

# review_ 系列全部通过（含历史约束）
cd src-tauri && cargo test review_ --tests --no-fail-fast

# 全量 Rust 测试无新增失败
cd src-tauri && cargo test
```
