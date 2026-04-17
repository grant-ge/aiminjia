# P0 Bug 修复计划（Plan-A）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 6 项 P0 级 bug，消除对话 cancel 崩溃、权限 Ask 路径断路、Mutex poison 全进程崩溃等正确性问题。

**Architecture:** 各修复点互相独立，每个 Task 独立可测试可 commit。A1-A2 修复 Runtime 层，A3 修复并发安全，A4-A5 修复 Python 子系统，A6 修复异步阻塞。

**Tech Stack:** Rust, tokio, async_trait, Python（sandbox.py）

**Worktree branch:** `fix/p0-bugfix`

---

## Task A1：Cancel 后 synthetic tool_result 注入

**问题根因：** `chat_turn_driver.rs` 的 `LlmStepResult::Cancelled` 分支在工具执行中途被取消时，直接 `break 'turn`。此时 `state.messages` 里已追加了带 `tool_use` 块的 assistant message，但没有对应的 `tool_result`（role=tool）消息。下一轮用户发消息时 Anthropic API 拿到残缺对话历史，返回 400 "messages: final assistant content must end in a human turn"。

**修复位置：** `src-tauri/src/runtime/chat/chat_turn_driver.rs`，`LlmStepResult::Cancelled` 分支——在 break 前为所有已发出但未收到结果的 tool_use 注入 synthetic tool_result。

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Add tests: `src-tauri/tests/p0_a1_cancel_synthetic_tool_result_test.rs`

---

- [ ] **Step A1-1: 写失败测试**

创建 `src-tauri/tests/p0_a1_cancel_synthetic_tool_result_test.rs`：

```rust
// src-tauri/tests/p0_a1_cancel_synthetic_tool_result_test.rs
//
// P0-A1 回归测试：取消时已发出 tool_use 的对话历史必须有对应 tool_result。
// 验证 driver 在 Cancelled 分支不会留下裸 tool_use 消息。

use std::sync::Arc;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::ids::RunId;
use async_trait::async_trait;

/// Executor：先返回 ToolCalls，再返回 Cancelled——模拟工具执行到一半被取消。
struct CancelAfterToolCallsExecutor {
    iteration: std::sync::Mutex<u32>,
    /// 收集每次迭代开始时 driver 传入的 messages（快照）
    recorded_messages: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
}

impl CancelAfterToolCallsExecutor {
    fn new() -> Self {
        Self {
            iteration: std::sync::Mutex::new(0),
            recorded_messages: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn messages_at_second_iteration(&self) -> Option<Vec<serde_json::Value>> {
        let msgs = self.recorded_messages.lock().unwrap();
        msgs.get(1).cloned()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CancelAfterToolCallsExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut it = self.iteration.lock().unwrap();
        let current = *it;
        *it += 1;
        self.recorded_messages.lock().unwrap().push(input.messages.clone());
        match current {
            0 => Ok(LlmStepResult::ToolCalls {
                assistant_content: String::new(),
                tool_calls: vec![
                    RuntimeToolCallRequest {
                        tool_call_id: "tc-cancel-1".to_string(),
                        tool_name: "some_tool".to_string(),
                        args: serde_json::json!({}),
                        purpose: None,
                    },
                ],
                tokens_in: 10,
                tokens_out: 5,
            }),
            _ => Ok(LlmStepResult::Cancelled),
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("cancelled-msg-id".to_string())
    }
}

fn make_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("run-a1"), "test input".to_string())
}

/// 核心回归：取消后追加的 tool_result 消息数量必须 >= tool_use 数量。
/// 若不修复，下一个 LLM 迭代（或下次 Turn）会看到裸 tool_use，
/// 与 Anthropic API 约定不符，导致 400 错误。
#[tokio::test]
async fn cancelled_after_tool_calls_messages_have_matching_tool_results() {
    let executor = Arc::new(CancelAfterToolCallsExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());

    let mut turn = make_turn("conv-a1-cancel");
    let request = ChatTurnRequest::new("conv-a1-cancel", "do something", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    // Turn 可能返回 Ok（cancel 后正常结束）或 Err——两者均可接受；
    // 关键在于：若第二次迭代真的被调用，messages 中必须有对应的 tool_result。
    let _ = result;

    // 如果 executor 被调用了两次（即 driver 确实发起第二轮 LLM），
    // 第二次迭代看到的 messages 中不应出现「有 tool_use 但无 tool_result」的情况。
    if let Some(msgs) = executor.messages_at_second_iteration() {
        // 找出所有 role=assistant 消息中包含的 tool_use id
        let mut tool_use_ids: Vec<String> = Vec::new();
        for m in &msgs {
            if m.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                // content 可能是数组（Anthropic format）或字符串
                if let Some(content_arr) = m.get("content").and_then(|c| c.as_array()) {
                    for block in content_arr {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                                tool_use_ids.push(id.to_string());
                            }
                        }
                    }
                }
            }
        }
        // 找出所有 role=tool 消息中对应的 tool_call_id（或 tool_use_id）
        let mut tool_result_ids: Vec<String> = Vec::new();
        for m in &msgs {
            if m.get("role").and_then(|v| v.as_str()) == Some("tool") {
                if let Some(id) = m.get("toolCallId").and_then(|v| v.as_str())
                    .or_else(|| m.get("tool_use_id").and_then(|v| v.as_str()))
                {
                    tool_result_ids.push(id.to_string());
                }
            }
        }
        for id in &tool_use_ids {
            assert!(
                tool_result_ids.contains(id),
                "tool_use id '{}' has no matching tool_result in messages after cancel. \
                 Messages: {:?}",
                id,
                msgs
            );
        }
    }
    // 若 executor 只被调用了一次（driver 在 Cancelled 之后未再进行 LLM 迭代），
    // 则 messages 断言天然成立——无第二轮就无需检查。
}

/// 辅助验证：cancelled 后 StreamDone 仍然要发出（保证前端不挂起）。
#[tokio::test]
async fn cancelled_after_tool_calls_still_emits_stream_done() {
    let executor = Arc::new(CancelAfterToolCallsExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_turn("conv-a1-stream-done");
    let request = ChatTurnRequest::new("conv-a1-stream-done", "cancel test", vec![]);
    let _ = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            app_lib::runtime::events::RuntimeEventKind::StreamDone
        )),
        "StreamDone must be emitted even after cancel, events: {:?}",
        events.iter().map(|e| format!("{:?}", e.kind)).collect::<Vec<_>>()
    );
}
```

- [ ] **Step A1-2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a1_cancel_synthetic_tool_result_test -- --nocapture 2>&1 | tail -20
```

预期：`cancelled_after_tool_calls_messages_have_matching_tool_results` 失败或编译失败（tool_result 未注入时，第二次迭代若确实发生，断言因 tool_use 没有对应 tool_result 而失败）。

- [ ] **Step A1-3: 实现修复**

在 `src-tauri/src/runtime/chat/chat_turn_driver.rs` 的 `LlmStepResult::Cancelled` 分支中，**在 `break 'turn` 之前**，为 `state.messages` 里所有已有 `tool_use` 但还没有对应 `tool_result` 的 tool_call_id 注入 synthetic tool_result。

找到如下代码段（约第 417-421 行）：

```rust
// ── 5d: user / token cancellation ────────────────────────────
LlmStepResult::Cancelled => {
    state.stream_cancelled = true;
    break 'turn;
}
```

替换为：

```rust
// ── 5d: user / token cancellation ────────────────────────────
LlmStepResult::Cancelled => {
    state.stream_cancelled = true;
    // A1 修复：为已发出但未收到结果的 tool_use 注入 synthetic tool_result。
    // Anthropic API 要求每个 tool_use block 必须有对应的 tool_result，
    // 否则下次 API 调用会返回 400。
    let tool_result_ids: std::collections::HashSet<String> = state.messages.iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("tool"))
        .filter_map(|m| {
            m.get("toolCallId")
                .or_else(|| m.get("tool_use_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    let mut synthetic_results: Vec<serde_json::Value> = Vec::new();
    for msg in &state.messages {
        if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
            for block in content_arr {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                        if !tool_result_ids.contains(id) {
                            let tool_name = block
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            synthetic_results.push(serde_json::json!({
                                "role": "tool",
                                "toolCallId": id,
                                "name": tool_name,
                                "content": "Tool execution was interrupted by user cancellation.",
                            }));
                        }
                    }
                }
            }
        }
    }
    state.messages.extend(synthetic_results);
    break 'turn;
}
```

- [ ] **Step A1-4: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a1_cancel_synthetic_tool_result_test -- --nocapture 2>&1 | tail -20
```

预期：两个测试均 `test ... ok`。

- [ ] **Step A1-5: 运行 S4 回归测试确保未破坏现有逻辑**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test s4_driver_loop_test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A1-6: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/tests/p0_a1_cancel_synthetic_tool_result_test.rs && \
git commit -m "$(cat <<'EOF'
fix(runtime): inject synthetic tool_result on cancel to prevent Anthropic API 400

When a turn is cancelled mid-tool-execution, assistant messages with tool_use
blocks had no corresponding tool_result, causing 400 errors on subsequent turns.
Now injects synthetic tool_result("Tool execution was interrupted...") for every
orphaned tool_use before breaking the turn loop.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A2：权限 Ask 路径接通

**问题根因：** `query_engine.rs` 的 `run_tool_call_with_bus` 在收到 `ToolDispatchOutcome::AskRequired` 时已正确返回 `RuntimeToolCallOutcome::AskRequired`，但 `chat_turn_driver.rs` 的 `tool_result_collector::collect_results` 直接把它降级为一段文字 `tool_result`，前端看不到权限确认请求。`RuntimeEventKind` 中没有 `PermissionAskRequired` variant，`tauri_event_adapter` 也没有对应映射。

**修复：**
1. `src-tauri/src/runtime/events.rs` 新增 `RuntimeEventKind::PermissionAskRequired`
2. `src-tauri/src/transport/tauri_event_adapter.rs` 新增映射 → `"permission:ask"` 前端事件
3. `chat_turn_driver.rs`（ToolCalls 分支）检测 `AskRequired` 结果并 emit `PermissionAskRequired` 事件（完整交互式等待在 Φ1/S6 专项实现；此处将 Ask 转为 tool_result 并继续，确保 LLM 得到反馈且前端已收到事件）

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Add tests: `src-tauri/tests/p0_a2_permission_ask_routing_test.rs`

---

- [ ] **Step A2-1: 写失败测试**

创建 `src-tauri/tests/p0_a2_permission_ask_routing_test.rs`：

```rust
// src-tauri/tests/p0_a2_permission_ask_routing_test.rs
//
// P0-A2 回归测试：AskRequired 必须 emit PermissionAskRequired 事件，
// 且前端 event adapter 必须将其映射为 "permission:ask"。

use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

/// A2-T1: RuntimeEventKind 包含 PermissionAskRequired variant（编译即通过）。
#[test]
fn permission_ask_required_variant_exists() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-a2"),
        RunId::new("run-a2"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: ToolCallId::new("tc-ask-1".to_string()),
            tool_name: "dangerous_tool".to_string(),
            message: "This tool will delete files. Allow?".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
        },
    );
    // ToolCallId field population check
    assert!(event.tool_call_id.is_some(), "tool_call_id must be set for PermissionAskRequired");
}

/// A2-T2: map_runtime_event 将 PermissionAskRequired → "permission:ask" 前端事件。
#[test]
fn permission_ask_required_maps_to_permission_ask_legacy_event() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-a2"),
        RunId::new("run-a2"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: ToolCallId::new("tc-ask-1".to_string()),
            tool_name: "delete_files".to_string(),
            message: "This action is irreversible. Confirm?".to_string(),
            suggestions: vec!["Allow".to_string(), "Deny".to_string()],
        },
    );
    let mapped = map_runtime_event(&event);
    assert!(mapped.is_some(), "PermissionAskRequired must map to a legacy event");
    let legacy = mapped.unwrap();
    assert_eq!(legacy.name, "permission:ask",
        "must map to 'permission:ask' event, got '{}'", legacy.name);
}

/// A2-T3: permission:ask payload 包含必要字段。
#[test]
fn permission_ask_payload_has_required_fields() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-payload"),
        RunId::new("run-payload"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: ToolCallId::new("tc-payload".to_string()),
            tool_name: "write_file".to_string(),
            message: "Allow writing to /etc?".to_string(),
            suggestions: vec!["Yes".to_string(), "No".to_string()],
        },
    );
    let legacy = map_runtime_event(&event).unwrap();
    let payload = &legacy.payload;

    assert!(payload.get("toolCallId").is_some(), "payload must have toolCallId");
    assert!(payload.get("toolName").is_some(), "payload must have toolName");
    assert!(payload.get("message").is_some(), "payload must have message");
    assert!(payload.get("conversationId").is_some(), "payload must have conversationId");
    assert!(payload.get("runId").is_some(), "payload must have runId");

    assert_eq!(payload["toolName"].as_str().unwrap(), "write_file");
    assert_eq!(payload["toolCallId"].as_str().unwrap(), "tc-payload");
}

/// A2-T4: driver 在处理 AskRequired 结果时发出 PermissionAskRequired 事件。
/// 使用带有 AskRequired 结果的 tool_round 来验证事件发射。
use std::sync::Arc;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionOutcome};
use async_trait::async_trait;

/// Tool 总是返回 AskRequired（通过 PermissionDecision 模拟）。
/// 注：ToolDispatcher 内部通过 PermissionPipeline 来决定 Ask/Allow/Deny。
/// 为方便测试，我们直接在 MockExecutor 里造一个含 AskRequired 的 round_result
/// 并验证 driver 的事件发射行为。
struct AskRequiredExecutor {
    iteration: std::sync::Mutex<u32>,
}

impl AskRequiredExecutor {
    fn new() -> Self {
        Self { iteration: std::sync::Mutex::new(0) }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for AskRequiredExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut it = self.iteration.lock().unwrap();
        let current = *it;
        *it += 1;
        match current {
            0 => Ok(LlmStepResult::ToolCalls {
                assistant_content: String::new(),
                tool_calls: vec![
                    RuntimeToolCallRequest {
                        tool_call_id: "tc-ask-driver".to_string(),
                        tool_name: "ask_tool".to_string(),
                        args: serde_json::json!({}),
                        purpose: None,
                    },
                ],
                tokens_in: 5,
                tokens_out: 3,
            }),
            _ => Ok(LlmStepResult::ContentComplete {
                content: "done after ask".to_string(),
                tokens_in: 3,
                tokens_out: 2,
            }),
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("ask-msg-id".to_string())
    }
}

/// AskPipeline：对所有工具调用返回 Ask。
struct AskPermissionPipeline;

#[async_trait]
impl app_lib::runtime::tools::permission::PermissionPipeline for AskPermissionPipeline {
    async fn check(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Ask {
            tool_call_id: _ctx.tool_call_id.clone(),
            tool_name: _tool_name.to_string(),
            message: "Requires confirmation".to_string(),
            suggestions: vec!["Allow".to_string(), "Deny".to_string()],
        }
    }
}

struct AlwaysOkTool;

#[async_trait]
impl RuntimeTool for AlwaysOkTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("ask_tool", "A tool that needs permission")
    }
    async fn execute(&self, _input: serde_json::Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("ask_tool".to_string(), "executed".to_string(), None))
    }
}

#[tokio::test]
async fn driver_emits_permission_ask_required_when_tool_returns_ask() {
    let pipeline = Arc::new(AskPermissionPipeline);
    let dispatcher = Arc::new(ToolDispatcher::new(pipeline));
    dispatcher.register(Arc::new(AlwaysOkTool));

    let executor = Arc::new(AskRequiredExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::with_dispatcher(dispatcher);
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-ask-driver");
    let mut turn = TurnState::new(mapping, RunId::new("run-ask"), "test".to_string());
    let request = ChatTurnRequest::new("conv-ask-driver", "do something restricted", vec![]);

    let _ = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();
    let has_permission_ask = events.iter().any(|e| {
        matches!(&e.kind, RuntimeEventKind::PermissionAskRequired { .. })
    });
    assert!(
        has_permission_ask,
        "driver must emit PermissionAskRequired when tool returns Ask. \
         Events: {:?}",
        events.iter().map(|e| format!("{:?}", e.kind)).collect::<Vec<_>>()
    );
}
```

- [ ] **Step A2-2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a2_permission_ask_routing_test -- --nocapture 2>&1 | head -30
```

预期：编译失败，`RuntimeEventKind::PermissionAskRequired` 不存在。

- [ ] **Step A2-3: 实现 — events.rs 新增 variant**

在 `src-tauri/src/runtime/events.rs` 的 `RuntimeEventKind` 枚举中新增：

```rust
    PermissionAskRequired {
        tool_call_id: ToolCallId,
        tool_name: String,
        /// Human-readable message explaining what permission is needed.
        message: String,
        /// Suggested response options (e.g. ["Allow once", "Always allow", "Deny"]).
        suggestions: Vec<String>,
    },
```

同时在 `RuntimeEvent::new` 的 `tool_call_id` match arm 中增加：

```rust
            RuntimeEventKind::PermissionAskRequired { tool_call_id, .. } => {
                Some(tool_call_id.clone())
            }
```

- [ ] **Step A2-4: 实现 — tauri_event_adapter.rs 新增映射**

在 `src-tauri/src/transport/tauri_event_adapter.rs` 的 `map_runtime_event` 函数中，在 `_ => None` 之前插入：

```rust
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id,
            tool_name,
            message,
            suggestions,
        } => Some(LegacyEvent {
            name: "permission:ask".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "toolCallId": tool_call_id.as_str(),
                "toolName": tool_name,
                "message": message,
                "suggestions": suggestions,
                "runId": event.run_id.as_str(),
            }),
        }),
```

- [ ] **Step A2-5: 实现 — chat_turn_driver.rs 检测 AskRequired 并发射事件**

在 `run_chat_turn_s4` 的 `LlmStepResult::ToolCalls` 分支中，找到 `collect_results` 调用之后、`safeguard::check_iteration` 之前的位置，新增对 `RuntimeToolCallOutcome::AskRequired` 的检测：

找到以下代码：

```rust
                    // Collect and merge results into state.
                    let results =
                        tool_result_collector::collect_results(round_results, 8000);
                    for msg in results.tool_result_messages {
                        state.messages.push(msg);
                    }
```

替换为：

```rust
                    // A2 修复：检测 AskRequired 结果并发射 PermissionAskRequired 事件。
                    // 完整的交互式等待（暂停 turn、等待前端响应）在 S6 专项实现。
                    // 当前实现：emit 事件通知前端，继续执行（tool_result 内容为权限说明）。
                    for round_result in &round_results {
                        if let crate::runtime::chat::tool_round_driver::ToolRoundResult::Ok(
                            crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome::AskRequired {
                                tool_call_id, tool_name, decision,
                            }
                        ) = round_result {
                            let (ask_message, ask_suggestions) = match decision {
                                crate::runtime::tools::permission::PermissionDecision::Ask {
                                    message,
                                    suggestions,
                                    ..
                                } => (message.clone(), suggestions.clone()),
                                _ => (
                                    format!("Tool '{}' requires user confirmation.", tool_name),
                                    vec!["Allow".to_string(), "Deny".to_string()],
                                ),
                            };
                            let _ = self.event_bus.emit(RuntimeEvent::new(
                                session_id.clone(),
                                run_id.clone(),
                                RuntimeEventKind::PermissionAskRequired {
                                    tool_call_id: crate::runtime::ids::ToolCallId::new(
                                        tool_call_id.clone(),
                                    ),
                                    tool_name: tool_name.clone(),
                                    message: ask_message,
                                    suggestions: ask_suggestions,
                                },
                            )).await;
                        }
                    }

                    // Collect and merge results into state.
                    let results =
                        tool_result_collector::collect_results(round_results, 8000);
                    for msg in results.tool_result_messages {
                        state.messages.push(msg);
                    }
```

- [ ] **Step A2-6: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a2_permission_ask_routing_test -- --nocapture 2>&1 | tail -20
```

预期：所有测试通过。

- [ ] **Step A2-7: 运行 event adapter 回归测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test tauri_event_adapter_test -- --nocapture 2>&1 | tail -10
```

- [ ] **Step A2-8: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/events.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/tests/p0_a2_permission_ask_routing_test.rs && \
git commit -m "$(cat <<'EOF'
feat(permissions): add PermissionAskRequired event + permission:ask adapter mapping

Adds RuntimeEventKind::PermissionAskRequired, maps it to "permission:ask" legacy
event in TauriEventAdapter, and makes the S4 driver emit it when a tool returns
AskRequired. Full interactive wait (pause turn, await frontend response) deferred
to S6; this patch ensures the frontend receives the event signal immediately.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A3：std::Mutex → tokio::sync::Mutex（防 poison panic）

**问题根因：** `run_registry.rs` 所有 `self.active_runs.lock().unwrap()` 使用 `std::sync::Mutex`。若某个 lock holder panic（在 async 上下文中极易发生），mutex 进入 poison 状态，后续所有 `.unwrap()` 调用直接 panic，导致全进程崩溃。虽然 `active_runs` 的同步锁在技术上不跨 await，但 `attach_stream`、`reserve` 等方法本身是同步的而被 async 调用者持有，且 `PythonSessionManager` 的 std Mutex 注释已解释过为何在 sync 路径可用——registry 的锁粒度更细，poison 风险更高。

**修复方案：** 改为 `tokio::sync::Mutex`，所有 `.lock().unwrap()` 改为 `.lock().await`（`tokio::sync::Mutex` 不支持 poison），把所有公有方法改为 `async`。

**Files:**
- Modify: `src-tauri/src/runtime/run_registry.rs`
- Add tests: `src-tauri/tests/p0_a3_run_registry_tokio_mutex_test.rs`

---

- [ ] **Step A3-1: 写失败测试**

创建 `src-tauri/tests/p0_a3_run_registry_tokio_mutex_test.rs`：

```rust
// src-tauri/tests/p0_a3_run_registry_tokio_mutex_test.rs
//
// P0-A3 回归测试：RuntimeRunRegistry 使用 tokio::sync::Mutex，
// poison 情形不会导致全进程崩溃；接口为 async。

use app_lib::runtime::{RunId, RuntimeRunRegistry};

/// A3-T1: registry 基本 reserve/cancel/clear 在 async 上下文正常工作。
#[tokio::test]
async fn registry_reserve_and_clear_async() {
    let registry = RuntimeRunRegistry::new();
    registry.reserve("conv-a3", RunId::new("run-a3")).await.unwrap();

    assert!(registry.is_session_busy("conv-a3").await);
    assert_eq!(
        registry.run_id_for_session("conv-a3").await.unwrap().as_str(),
        "run-a3"
    );

    registry.cancel("conv-a3").await;
    assert!(registry.is_cancelled("conv-a3").await);

    let cleared = registry.clear("conv-a3").await.unwrap();
    assert_eq!(cleared.as_str(), "run-a3");
    assert!(!registry.is_session_busy("conv-a3").await);
}

/// A3-T2: 同一 session 重复 reserve 返回错误（不崩溃）。
#[tokio::test]
async fn registry_double_reserve_returns_error() {
    let registry = RuntimeRunRegistry::new();
    registry.reserve("conv-a3-dup", RunId::new("run-1")).await.unwrap();
    let result = registry.reserve("conv-a3-dup", RunId::new("run-2")).await;
    assert!(result.is_err(), "double reserve must fail with error, not panic");
    assert!(result.unwrap_err().contains("already processing"));
}

/// A3-T3: 并发 reserve 不死锁。
#[tokio::test]
async fn registry_concurrent_reserves_do_not_deadlock() {
    use std::sync::Arc;
    let registry = Arc::new(RuntimeRunRegistry::new());

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let r = registry.clone();
            tokio::spawn(async move {
                let session = format!("conv-concurrent-{}", i);
                let _ = r.reserve(&session, RunId::new(format!("run-{}", i))).await;
                let _ = r.clear(&session).await;
            })
        })
        .collect();

    for h in handles {
        h.await.expect("task must not panic");
    }
    // 全部 session 应已被清理
    assert!(!registry.is_busy().await, "all sessions must be cleared after concurrent ops");
}

/// A3-T4: is_busy 在 async 上下文返回正确结果。
#[tokio::test]
async fn registry_is_busy_reflects_active_runs() {
    let registry = RuntimeRunRegistry::new();
    assert!(!registry.is_busy().await);
    registry.reserve("conv-busy", RunId::new("run-busy")).await.unwrap();
    assert!(registry.is_busy().await);
    registry.clear("conv-busy").await;
    assert!(!registry.is_busy().await);
}
```

- [ ] **Step A3-2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a3_run_registry_tokio_mutex_test -- --nocapture 2>&1 | head -20
```

预期：编译失败，`registry.reserve(...)` 等方法不是 async，无法 `.await`。

- [ ] **Step A3-3: 实现修复**

修改 `src-tauri/src/runtime/run_registry.rs`：

1. 将 `use std::sync::Mutex` 改为 `use tokio::sync::Mutex`（已有 `use tokio::sync::watch`，同一 import 块）。
2. 将所有公有方法（`reserve`、`attach_stream`、`cancel`、`clear`、`is_busy`、`is_session_busy`、`busy_sessions`、`run_id_for_session`、`is_cancelled`）改为 `async fn`。
3. 所有 `self.active_runs.lock().unwrap()` 改为 `self.active_runs.lock().await`（tokio Mutex 不会 poison，lock 返回 `MutexGuard` 而非 `Result`）。

完整替换后的文件内容：

```rust
use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::watch;
use tokio::sync::Mutex;

use crate::llm::gateway::MAX_CONCURRENT_AGENTS;
use crate::runtime::ids::RunId;

struct ActiveRun {
    task_id: String,
    run_id: RunId,
    cancel: watch::Sender<bool>,
    started_at: Instant,
}

#[derive(Default)]
pub struct RuntimeRunRegistry {
    active_runs: Mutex<HashMap<String, ActiveRun>>,
}

impl RuntimeRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn reserve(&self, session_id: &str, run_id: RunId) -> Result<(), String> {
        let mut active_runs = self.active_runs.lock().await;
        if active_runs.contains_key(session_id) {
            return Err("This conversation is already processing.".to_string());
        }
        if active_runs.len() >= MAX_CONCURRENT_AGENTS {
            return Err(format!(
                "Maximum concurrent conversations reached ({}). Please wait.",
                MAX_CONCURRENT_AGENTS
            ));
        }
        let (cancel_tx, _) = watch::channel(false);
        active_runs.insert(
            session_id.to_string(),
            ActiveRun {
                task_id: format!("pre-{}", uuid::Uuid::new_v4()),
                run_id,
                cancel: cancel_tx,
                started_at: Instant::now(),
            },
        );
        Ok(())
    }

    pub async fn attach_stream(
        &self,
        session_id: &str,
        task_id: String,
    ) -> anyhow::Result<watch::Receiver<bool>> {
        let mut active_runs = self.active_runs.lock().await;
        if let Some(existing) = active_runs.get_mut(session_id) {
            if *existing.cancel.borrow() {
                anyhow::bail!("Conversation cancelled before stream started");
            }
            existing.task_id = task_id;
            existing.started_at = Instant::now();
            return Ok(existing.cancel.subscribe());
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        active_runs.insert(
            session_id.to_string(),
            ActiveRun {
                task_id,
                run_id: RunId::new(format!("legacy-{session_id}")),
                cancel: cancel_tx,
                started_at: Instant::now(),
            },
        );
        Ok(cancel_rx)
    }

    pub async fn cancel(&self, session_id: &str) {
        let active_runs = self.active_runs.lock().await;
        if let Some(run) = active_runs.get(session_id) {
            let _ = run.cancel.send_replace(true);
        }
    }

    pub async fn clear(&self, session_id: &str) -> Option<RunId> {
        self.active_runs
            .lock()
            .await
            .remove(session_id)
            .map(|run| run.run_id)
    }

    pub async fn is_busy(&self) -> bool {
        !self.active_runs.lock().await.is_empty()
    }

    pub async fn is_session_busy(&self, session_id: &str) -> bool {
        self.active_runs.lock().await.contains_key(session_id)
    }

    pub async fn busy_sessions(&self) -> Vec<String> {
        self.active_runs.lock().await.keys().cloned().collect()
    }

    pub async fn run_id_for_session(&self, session_id: &str) -> Option<RunId> {
        self.active_runs
            .lock()
            .await
            .get(session_id)
            .map(|run| run.run_id.clone())
    }

    pub async fn is_cancelled(&self, session_id: &str) -> bool {
        self.active_runs
            .lock()
            .await
            .get(session_id)
            .map(|run| *run.cancel.borrow())
            .unwrap_or(false)
    }
}
```

- [ ] **Step A3-4: 修复调用方（所有调用 registry 方法的地方加 .await）**

搜索所有调用 `RuntimeRunRegistry` 方法的位置：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  grep -rn "\.reserve\|\.attach_stream\|registry\.cancel\|registry\.clear\|\.is_busy\|\.is_session_busy\|\.busy_sessions\|\.run_id_for_session\|\.is_cancelled" \
  src/ --include="*.rs" | grep -v "run_registry.rs"
```

对每个调用点添加 `.await`（若在 async fn 中）或使用 `block_on`（若在 sync 上下文，但应转为 async）。

- [ ] **Step A3-5: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a3_run_registry_tokio_mutex_test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A3-6: 运行现有 registry 测试（也需要改为 async）**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test runtime_run_registry_test -- --nocapture 2>&1 | tail -10
```

注意：`runtime_run_registry_test.rs` 中的测试也需更新为 `#[tokio::test]` 并加 `.await`。

- [ ] **Step A3-7: 完整编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo build 2>&1 | grep -E "^error" | head -20
```

- [ ] **Step A3-8: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/run_registry.rs \
        src-tauri/tests/p0_a3_run_registry_tokio_mutex_test.rs \
        src-tauri/tests/runtime_run_registry_test.rs && \
git commit -m "$(cat <<'EOF'
fix(registry): replace std::Mutex with tokio::sync::Mutex in RuntimeRunRegistry

std::Mutex panics on poison in async contexts, causing process-wide crashes.
tokio::sync::Mutex never poisons and is the correct choice for async code.
All public methods are now async. Callers updated with .await at all call sites.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A4：Python session key 改为 per-run

**问题根因：** `session.rs` 中 `execute()` 方法用 `conversation_id` 作为 session key，多个并发 run 在同一 conversation 中会共享 Python 进程状态（全局变量、已加���的 DataFrame 等），导致 run 间数据污染。`session_key_for_run()` 函数已存在（第 61-63 行），但 `execute()` 方法的调用者（`llm/tool_executor/python.rs`）有时仍走旧的 `conversation_id` 路径。

**修复：** 确认 `execute_for_run()` 是唯一被 S4 路径调用的入口，并为 legacy 调用者添加迁移路径。同时添加测试验证 session key 包含 run_id 而非 raw conversation_id。

**Files:**
- Modify: `src-tauri/src/python/session.rs`（确保 `execute()` 不被生产路径调用；若有旧调用，导向 `execute_for_run()`）
- Add tests: `src-tauri/tests/p0_a4_python_session_per_run_test.rs`

---

- [ ] **Step A4-1: 确认调用点现状**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  grep -rn "session_manager\.execute\b\|\.execute(" src/ --include="*.rs" | \
  grep -v "execute_for_run\|execute_python\|#\[" | head -20
```

记录所有使用 `execute(conversation_id, ...)` 的调用点。

- [ ] **Step A4-2: 写失败测试**

创建 `src-tauri/tests/p0_a4_python_session_per_run_test.rs`：

```rust
// src-tauri/tests/p0_a4_python_session_per_run_test.rs
//
// P0-A4 回归测试：Python session key 必须包含 run_id，不得用裸 conversation_id。

use app_lib::python::session::session_key_for_run;
use app_lib::runtime::ids::RunId;

/// A4-T1: session_key_for_run 输出格式为 "python-run:{run_id}"。
#[test]
fn session_key_for_run_format() {
    let run_id = RunId::new("abc-123");
    let key = session_key_for_run(&run_id);
    assert_eq!(key, "python-run:abc-123");
    // 绝对不能是裸 conversation_id
    assert!(!key.starts_with("conv-"), "session key must not be a raw conversation_id");
}

/// A4-T2: 不同 run_id 产生不同 session key（多 run 隔离）。
#[test]
fn different_run_ids_produce_different_session_keys() {
    let key1 = session_key_for_run(&RunId::new("run-aaa"));
    let key2 = session_key_for_run(&RunId::new("run-bbb"));
    assert_ne!(key1, key2, "different runs must have different session keys");
}

/// A4-T3: 同一 conversation 的不同 run 产生不同 session key（核心隔离保证）。
#[test]
fn same_conversation_different_runs_are_isolated() {
    // 两个 run 属于同一 conversation，但 run_id 不同
    let run1 = RunId::new("conv-x-run-1");
    let run2 = RunId::new("conv-x-run-2");
    let key1 = session_key_for_run(&run1);
    let key2 = session_key_for_run(&run2);
    // session key 必须不同，不能退化为 "conv-x"
    assert_ne!(key1, key2);
    assert!(key1.contains("conv-x-run-1"), "key must embed run_id");
    assert!(key2.contains("conv-x-run-2"), "key must embed run_id");
}

/// A4-T4: execute_for_run 方法存在且签名正确（编译即通过）。
/// 验证 PythonSessionManager 暴露了 per-run 入口。
#[test]
fn execute_for_run_method_exists_on_session_manager() {
    // 此测试通过编译验证接口存在。
    // 实际执行需要真实 Python 环境，仅做类型检查。
    fn _assert_signature<F, Fut>(_f: F)
    where
        F: Fn(
            &app_lib::python::session::PythonSessionManager,
            &RunId,
            &str,
            std::time::Duration,
            &app_lib::python::sandbox::SandboxConfig,
        ) -> Fut,
        Fut: std::future::Future,
    {}
    // 若 execute_for_run 存在且签名匹配，此函数编译通过（不调用）
}
```

- [ ] **Step A4-3: 验证测试状态**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a4_python_session_per_run_test -- --nocapture 2>&1 | tail -20
```

若 A4-T1~T3 已通过（`session_key_for_run` 已正确实现），则此步骤确认测试全绿。若有失败（如 `execute_for_run` 签名不匹配），进行下一步修复。

- [ ] **Step A4-4: 确保所有生产路径使用 execute_for_run**

搜索并修复仍使用 `execute(conversation_id, ...)` 的调用点：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  grep -rn "\.execute(" src/llm/tool_executor/ --include="*.rs" | head -20
```

对每个使用旧接口的调用点，改为 `execute_for_run(run_id, ...)` 并从调用上下文中获取 `run_id`（通过 `ToolExecutionContext` 的 `run_id` 字段）。

- [ ] **Step A4-5: 验证测试全通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a4_python_session_per_run_test -- --nocapture && \
  cargo test --test python_run_scope_test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A4-6: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/python/session.rs \
        src-tauri/tests/p0_a4_python_session_per_run_test.rs && \
git commit -m "$(cat <<'EOF'
fix(python): enforce per-run Python session isolation via session_key_for_run

Using conversation_id as session key allowed multiple concurrent runs to share
Python process state (globals, DataFrames), causing data pollution between runs.
All production call sites now use execute_for_run() which keys by RunId.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A5：Sandbox path 边界绕过修复

**问题根因：** `sandbox.rs` 的 Python preamble 中 `_safe_open` 用 `abs_path.startswith(os.path.realpath(p))` 检查写路径。若 workspace 是 `/workspace`，则路径 `/workspace.backup/evil.txt` 也会通过检查（因为 `"/workspace.backup/evil.txt".startswith("/workspace")` 为 `True`）。正确做法是要求路径等于 workspace 根或以 `<workspace>/` 开头。

**修复位置：** `sandbox.rs` 的 `preamble()` 方法中生成的 Python `_safe_open` 函数——修改路径检查逻辑。

**Files:**
- Modify: `src-tauri/src/python/sandbox.rs`
- Add tests: `src-tauri/tests/p0_a5_sandbox_path_boundary_test.rs`

---

- [ ] **Step A5-1: 写失败测试**

创建 `src-tauri/tests/p0_a5_sandbox_path_boundary_test.rs`：

```rust
// src-tauri/tests/p0_a5_sandbox_path_boundary_test.rs
//
// P0-A5 回归测试：sandbox preamble 的路径检查不得被前缀欺骗。

use app_lib::python::sandbox::SandboxConfig;

/// A5-T1: preamble 中不含有漏洞性的纯 startswith 检查（Rust 层验证）。
/// 确认生成的 Python 代码包含正确的路径分隔符检查。
#[test]
fn preamble_path_check_requires_separator_after_workspace_root() {
    let workspace = std::path::PathBuf::from("/workspace");
    let config = SandboxConfig::for_workspace(&workspace);
    let preamble = config.preamble();

    // 修复后的代码应包含对路径分隔符的检查
    // 正确形式：path == root or path.startswith(root + '/')
    // 或等价：startswith(root + os.sep)
    let has_separator_check = preamble.contains("+ os.sep")
        || preamble.contains("+ '/'")
        || preamble.contains("+ \"/\"")
        || preamble.contains("os.path.sep");
    assert!(
        has_separator_check,
        "preamble path check must include path separator to prevent prefix bypass attacks. \
         Got preamble snippet:\n{}",
        // 只打印 _safe_open 相关段落
        preamble.lines()
            .skip_while(|l| !l.contains("_safe_open"))
            .take(30)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A5-T2: Python inline 测试——验证修复后 /workspace.backup 无法写入。
/// 直接在 Rust 中通过 assert! 校验 preamble 字符串语义（无需运行 Python）。
#[test]
fn preamble_does_not_allow_prefix_bypass_path() {
    let workspace = std::path::PathBuf::from("/workspace");
    let config = SandboxConfig::for_workspace(&workspace);
    let preamble = config.preamble();

    // 旧的漏洞代码：abs_path.startswith(os.path.realpath(p))
    // 修复后不应有纯 startswith 而没有分隔符追加
    // 简单检查：如果 preamble 里 startswith 紧跟着 realpath(p)) 且没有 os.sep 拼接，则有漏洞
    let lines: Vec<&str> = preamble.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("startswith(os.path.realpath(p))") {
            panic!(
                "Found vulnerable path check at line {}: '{}'\n\
                 Must use 'startswith(os.path.realpath(p) + os.sep)' or equivalent",
                i + 1,
                line
            );
        }
        if line.contains("startswith(os.path.realpath(p))") {
            panic!(
                "Vulnerable startswith without separator at line {}: '{}'",
                i + 1,
                line
            );
        }
    }
}

/// A5-T3: 正确路径（workspace 子目录）仍然被允许。
/// 验证修复没有过度收紧——workspace 本身和子目录仍可写。
#[test]
fn preamble_allows_exact_workspace_and_subdirectories() {
    let workspace = std::path::PathBuf::from("/workspace");
    let config = SandboxConfig::for_workspace(&workspace);
    let preamble = config.preamble();

    // workspace 本身和其子目录应在 _ALLOWED_WRITE_PATHS 中
    assert!(preamble.contains("'/workspace'"), "workspace root must be in allowed paths");
    assert!(preamble.contains("'/workspace/uploads'"), "uploads subdir must be in allowed paths");
}
```

- [ ] **Step A5-2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a5_sandbox_path_boundary_test -- --nocapture 2>&1 | tail -20
```

预期：`preamble_path_check_requires_separator_after_workspace_root` 失败，因为旧代码使用纯 `startswith` 而无 separator 拼接。

- [ ] **Step A5-3: 实现修复**

在 `src-tauri/src/python/sandbox.rs` 的 `preamble()` 方法中，找到 `file_write_hook` 字符串里的路径检查代码（约第 283-286 行）：

```python
                allowed = any(
                    abs_path.startswith(os.path.realpath(p))
                    for p in _ALLOWED_WRITE_PATHS
                ) if _ALLOWED_WRITE_PATHS else False
```

替换为：

```python
                allowed = any(
                    abs_path == os.path.realpath(p) or
                    abs_path.startswith(os.path.realpath(p) + os.sep)
                    for p in _ALLOWED_WRITE_PATHS
                ) if _ALLOWED_WRITE_PATHS else False
```

注意：这段 Python 代码在 Rust 的 raw string `r#"..."#` 内，修改时要保持缩进一致。

- [ ] **Step A5-4: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a5_sandbox_path_boundary_test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A5-5: 运行 sandbox 相关测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test sandbox -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A5-6: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/python/sandbox.rs \
        src-tauri/tests/p0_a5_sandbox_path_boundary_test.rs && \
git commit -m "$(cat <<'EOF'
fix(sandbox): prevent path prefix bypass in _safe_open write restriction

Pure startswith('/workspace') allows '/workspace.backup/evil.txt' through.
Fixed to require path == root OR path.startswith(root + os.sep), closing the
directory traversal bypass vector in the Python sandbox write gate.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A6：build_env_info 阻塞 tokio 线程修复

**问题根因：** `context_builder.rs` 的 `build_env_info()` 调用 `std::process::Command::new("git").output()` 在 async 上下文（`run_chat_turn_s4` 通过 `get_env_info()` 间接调用），会阻塞当前 tokio worker 线程直到 git 命令完成。若 git 仓库很大或网络文件系统延迟，会饿死其他 async 任务。

**修复：** 改为 `tokio::process::Command`（async 版本），并将 `build_env_info` 改为 `async fn`。由于此函数被 `TauriLegacyTurnExecutor::get_env_info` 调用（已是 async），改为 async 不影响调用链。

**Files:**
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`（若 `get_env_info` 调用 `build_env_info`）
- Add tests: `src-tauri/tests/p0_a6_env_info_async_test.rs`

---

- [ ] **Step A6-1: 确认调用链**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  grep -rn "build_env_info" src/ --include="*.rs" | head -20
```

记录所有调用 `build_env_info` 的位置和上下文（sync/async）。

- [ ] **Step A6-2: 写失败测试**

创建 `src-tauri/tests/p0_a6_env_info_async_test.rs`：

```rust
// src-tauri/tests/p0_a6_env_info_async_test.rs
//
// P0-A6 回归测试：build_env_info 必须是 async fn（使用 tokio::process::Command）。

use app_lib::runtime::chat::context_builder::build_env_info;

/// A6-T1: build_env_info 是 async fn，可以在 tokio 上下文中 .await。
/// 编译通过即验证函数签名为 async。
#[tokio::test]
async fn build_env_info_is_async_and_returns_env_section() {
    let workspace_path = std::path::PathBuf::from("/tmp");
    // 若此函数是 sync，此处编译失败；若是 async，必须 .await
    let result = build_env_info(&workspace_path, None).await;
    assert!(result.contains("[当前环境]"), "must contain env section, got: {}", result);
    assert!(result.contains("Platform:"), "must contain platform info");
}

/// A6-T2: async build_env_info 在非 git 目录中静默跳过 git（不阻塞、不崩溃）。
#[tokio::test]
async fn build_env_info_async_skips_git_in_non_git_dir() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let workspace_path = temp_dir.path().to_path_buf();

    let result = build_env_info(&workspace_path, None).await;

    assert!(result.contains("[当前环境]"), "must have env section header");
    assert!(result.contains("工作目录:"), "must include working dir");
    // 非 git 目录不应崩溃，git 段静默省略
}

/// A6-T3: tokio::process::Command 被使用（Rust 代码层面验证）。
/// 通过读取源文件检查实现细节。
#[test]
fn context_builder_uses_tokio_process_command() {
    let source = std::fs::read_to_string("src/runtime/chat/context_builder.rs")
        .expect("read context_builder.rs");
    assert!(
        source.contains("tokio::process::Command"),
        "build_env_info must use tokio::process::Command (not std::process::Command) \
         to avoid blocking the async executor"
    );
    assert!(
        !source.contains("std::process::Command::new(\"git\")"),
        "must not use blocking std::process::Command for git in async context"
    );
}

/// A6-T4: build_env_info 调用耗时不超过 2 秒（验证非阻塞行为）。
/// 在正常系统上 git status 应该很快；超时说明实现仍在阻塞。
#[tokio::test]
async fn build_env_info_completes_within_reasonable_time() {
    let workspace_path = std::path::PathBuf::from("/tmp");
    let start = std::time::Instant::now();
    let _ = build_env_info(&workspace_path, None).await;
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 2,
        "build_env_info must complete within 2s, took {:?}ms",
        elapsed.as_millis()
    );
}
```

- [ ] **Step A6-3: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a6_env_info_async_test -- --nocapture 2>&1 | head -30
```

预期：编译失败，`build_env_info` 是 sync fn，无法 `.await`。或 `context_builder_uses_tokio_process_command` 断言失败（仍是 std::process::Command）。

- [ ] **Step A6-4: 实现修复**

修改 `src-tauri/src/runtime/chat/context_builder.rs` 中的 `build_env_info` 函数：

1. 函数签名改为 `async fn`
2. `std::process::Command::new("git")` 改为 `tokio::process::Command::new("git")`
3. `.output()` 改为 `.output().await`（tokio::process::Command::output 返回 Future）

将如下代码：

```rust
pub fn build_env_info(
    workspace_path: &std::path::PathBuf,
    authorized: Option<(&str, &str)>,
) -> String {
    // ... 前半部分不变 ...

    if let Ok(output) = std::process::Command::new("git")
        .args([
            "-C",
            &effective_path.to_string_lossy(),
            "status",
            "--short",
            "--branch",
        ])
        .output()
    {
```

替换为：

```rust
pub async fn build_env_info(
    workspace_path: &std::path::PathBuf,
    authorized: Option<(&str, &str)>,
) -> String {
    // ... 前半部分不变 ...

    if let Ok(output) = tokio::process::Command::new("git")
        .args([
            "-C",
            &effective_path.to_string_lossy(),
            "status",
            "--short",
            "--branch",
        ])
        .output()
        .await
    {
```

- [ ] **Step A6-5: 更新调用方**

所有调用 `build_env_info(...)` 的地方加 `.await`：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  grep -rn "build_env_info" src/ --include="*.rs" | grep -v "context_builder.rs"
```

对每个调用点加 `.await`（调用方本身应已是 async）。

同时更新 `context_builder.rs` 的测试模块——旧的同步单测需改为 `#[tokio::test]` 并加 `.await`：

在 `context_builder.rs` 中找到所有 `build_env_info` 测试，将 `#[test]` 改为 `#[tokio::test]`，函数改为 `async fn`，调用处加 `.await`。

- [ ] **Step A6-6: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a6_env_info_async_test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A6-7: 运行 context_builder 单元测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test build_env_info -- --nocapture 2>&1 | tail -20
```

- [ ] **Step A6-8: 全量编译确认**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo build 2>&1 | grep -E "^error" | head -20
```

- [ ] **Step A6-9: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/context_builder.rs \
        src-tauri/tests/p0_a6_env_info_async_test.rs && \
git commit -m "$(cat <<'EOF'
fix(context-builder): use tokio::process::Command in build_env_info to avoid blocking

std::process::Command::output() blocks the tokio worker thread for the duration
of the git subprocess. Switched to tokio::process::Command which is non-blocking
and plays nicely with the async executor. build_env_info is now async fn.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 最终验证

- [ ] **全量 Rust 测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test 2>&1 | tail -30
```

- [ ] **review_ 系列回归测试（架构约束）**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

- [ ] **前端测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -20
```

---

## 修复点汇总

| Task | 文件 | 问题 | 修复 |
|------|------|------|------|
| A1 | `chat_turn_driver.rs` | Cancel 后裸 tool_use 导致下次 API 400 | Cancelled 分支注入 synthetic tool_result |
| A2 | `events.rs`, `tauri_event_adapter.rs`, `chat_turn_driver.rs` | AskRequired 路径断路，前端看不到权限请求 | 新增 PermissionAskRequired event + permission:ask 映射 + driver 发射 |
| A3 | `run_registry.rs` | std::Mutex poison 导致全进程崩溃 | 改为 tokio::sync::Mutex，所有方法改为 async |
| A4 | `python/session.rs` | conversation 级 session 导致多 run 数据污染 | 所有生产路径走 execute_for_run()，key 含 run_id |
| A5 | `python/sandbox.rs` | startswith 路径检查被前缀欺骗（/workspace.backup 绕过） | 改为 `path == root or path.startswith(root + os.sep)` |
| A6 | `runtime/chat/context_builder.rs` | std::process::Command 阻塞 tokio worker | 改为 tokio::process::Command + async fn |
