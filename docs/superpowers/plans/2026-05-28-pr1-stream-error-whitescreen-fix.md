# PR1: 修白屏（Stream Error → Step 6-8 emit）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 chunk timeout / network error / PromptTooLong 后对话区白屏的 bug —— 在 `chat_turn_driver.rs` 的两个错误分支（`Err(err)` / `Err(TurnError::PromptTooLong)`）补 `MessagePersisted + StreamDone + AgentIdle` 三件套，让前端对话区显示错误占位文本（最小必要修复，不引入数据模型变更）。

**Architecture:** 提取一个 `emit_terminal_error_message_and_idle` 私有方法，复用于两个错误分支；错误以纯字符串形式塞入 `MessagePersisted.content.text`（PR2 才扩展为结构化 `error` 字段）。前端零改动 —— 现有 `message:updated` handler 把它当普通 assistant 文本渲染。

**Tech Stack:** Rust 1.75+ (RPITIT), Tokio async, anyhow, `RuntimeEventBus` / `RuntimeEvent` 事件系统, `cargo test --test s4_driver_loop_test`

**Spec:** [`docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md`](../specs/2026-05-28-streaming-error-handling-design.md) §四 PR1

---

## 文件结构

| 文件 | 改动类型 | 责任 |
|---|---|---|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Modify | 在 `Err(err)` 分支（行 2071-2078）和 `Err(TurnError::PromptTooLong)` 分支（行 2069 `return Err`）的现有错误退出路径前，调用新的 `emit_terminal_error_message_and_idle` 方法补三件套 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Add | 新增 `RuntimeChatTurnDriver::emit_terminal_error_message_and_idle` 私有方法（在 `mark_idle_and_maybe_emit_pending` 同区域），统一封装错误终态的事件序列 |
| `src-tauri/tests/review_stream_error_terminal_events.rs` | Create | 新增 `review_*` 集成测试，验证 mock executor 返回 `Err(TurnError::LlmError(...))` 时 driver 仍 emit 三件套 + 整条流程不破坏正常路径 |

**为什么提取一个方法**：现有两个错误分支（`Err(err)` 和 `Err(TurnError::PromptTooLong)`）都需要补同一组事件 + 内容占位逻辑。直接在两处复制代码会造成 ~50 行重复。提取一个方法保证两条路径行为一致，PR2 扩展时也只改一处。

**为什么不动 `finalize_content` / 不引入 `error` 字段**：spec 明确 PR1 最小范围，把数据模型变更留给 PR2。本期错误占位字符串直接写到 `state.full_content`，让 Step 6-8 的现有 emit 逻辑（行 2502-2510）自然消费它。

---

## Task 1: 新增 review 测试 — Err(LlmError) 终态三件套

**Files:**
- Create: `src-tauri/tests/review_stream_error_terminal_events.rs`

- [ ] **Step 1: 写失败的测试**

参考 `src-tauri/tests/s4_driver_loop_test.rs` 行 285-353（`make_test_turn` + `driver_s4_loop_content_complete`）作为模板。新建文件：

```rust
// src-tauri/tests/review_stream_error_terminal_events.rs
//! Architecture review: when `RuntimeLlmExecutor::run_llm_step` returns
//! `Err(TurnError::*)`, `RuntimeChatTurnDriver::run_chat_turn` MUST still
//! emit `MessagePersisted + StreamDone + AgentIdle` so the frontend chat
//! area renders an assistant bubble (instead of going white).
//!
//! Bug background (2026-05-28 客户白屏):
//!   `chat_turn_driver.rs:2071-2078` 的 `Err(err)` 分支直接 `return`，
//!   跳过 Step 6-8 → 前端 chatStore 没 assistant message → 白屏。
//!
//! See: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{
    ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;
use std::sync::Arc;

/// Mock executor that always returns `Err(TurnError::LlmError(...))`,
/// simulating a chunk-timeout / network error after retries are exhausted.
struct ErrLlmExecutor {
    error_message: String,
}

#[async_trait]
impl RuntimeLlmExecutor for ErrLlmExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Err(TurnError::LlmError(self.error_message.clone()))
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
        _thinking_blocks: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-error-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }
}

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(
        mapping,
        app_lib::runtime::ids::RunId::new("test-run-error"),
        "hi".to_string(),
    )
}

#[tokio::test]
async fn driver_emits_message_persisted_when_run_llm_step_errors() {
    // 模拟 chunk timeout / network error 经 MAX_STREAM_RETRIES 耗尽后,
    // run_llm_step 返回 Err(TurnError::LlmError(...)).
    let executor = Arc::new(ErrLlmExecutor {
        error_message: "Chunk timeout (90s) after 10 retries".to_string(),
    });
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-stream-err");
    let request = ChatTurnRequest::new("conv-stream-err", "hello", vec![]);

    // Driver 当前会返回 Err（错误向上传播是合理的，但事件必须先 emit）
    let _result = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();

    // 关键不变式：不论 driver 返回 Ok 还是 Err，三件套必须已发出
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::MessagePersisted { .. }
        )),
        "missing MessagePersisted on stream error — frontend will see white screen"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            RuntimeEventKind::StreamDone
        )),
        "missing StreamDone on stream error"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::AgentIdle { .. }
        )),
        "missing AgentIdle on stream error — agent will appear stuck"
    );
}

#[tokio::test]
async fn message_persisted_payload_contains_error_text_on_stream_error() {
    // 错误 message 的 content.text 应包含错误文案占位（PR1 用纯字符串，
    // PR2 改为结构化 error 字段）.
    let executor = Arc::new(ErrLlmExecutor {
        error_message: "Chunk timeout (90s) after 10 retries".to_string(),
    });
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-stream-err-text");
    let request = ChatTurnRequest::new("conv-stream-err-text", "hello", vec![]);

    let _result = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();
    let persisted = events
        .iter()
        .find_map(|e| match &e.kind {
            RuntimeEventKind::MessagePersisted { content, .. } => Some(content),
            _ => None,
        })
        .expect("MessagePersisted must be emitted on stream error");

    let text = persisted
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    assert!(
        !text.is_empty(),
        "MessagePersisted.content.text must NOT be empty on stream error \
         (else frontend renders empty bubble) — got: {:?}",
        persisted
    );
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events -- --nocapture
```

Expected: **FAIL**，两个测试都报缺失 `MessagePersisted` / `StreamDone` / `AgentIdle`。原因是当前 `Err(err)` 分支直接 return（`chat_turn_driver.rs:2077`）。

- [ ] **Step 3: 提交失败的测试**

```bash
git add src-tauri/tests/review_stream_error_terminal_events.rs
git commit -m "test(stream-error): add failing review test for terminal events on Err

Captures the bug where Err branch in run_chat_turn_s4 (line 2071-2078)
returns early, skipping Step 6-8 (MessagePersisted / StreamDone /
AgentIdle), causing chat area to go white when stream fails.

Will pass once PR1 fix is in place."
```

---

## Task 2: 提取 `emit_terminal_error_message_and_idle` 方法

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:2070-2078`（新增方法定义）

**为什么这步先做**：把"补三件套"的逻辑封装成独立方法 + 自带单元覆盖，避免 Task 3 / Task 4 改两个错误分支时各自抄一遍代码不一致。

- [ ] **Step 1: 在 `RuntimeChatTurnDriver` impl 块新增私有方法**

在 `chat_turn_driver.rs` 的 `impl RuntimeChatTurnDriver` 块里（紧挨着 `mark_idle_and_maybe_emit_pending` 方法），新增：

```rust
    /// 在错误终态（stream error / PromptTooLong）下补发 Step 6-8 的三件套
    /// 事件，避免前端 chat 区白屏。
    ///
    /// 行为对齐 `run_chat_turn_s4` 主路径的 Step 8（行 2502-2510）+
    /// Step 8 末尾的 AgentIdle emit（行 2596-2604）：
    ///
    /// 1. `MessagePersisted` — 让前端把错误占位文本作为 assistant 消息渲染
    /// 2. `StreamDone`        — 让前端 streamingState.isStreaming 复位
    /// 3. `AgentIdle`         — 让前端解锁输入框，agent 不再"思考中"
    ///
    /// PR1 范围内 `error_text` 是纯字符串占位，直接写入 `MessagePersisted`
    /// 的 `content.text`。PR2 会扩为结构化 `error: Option<MessageError>`
    /// 字段（spec §3.1）。
    ///
    /// `message_id` 通过 `executor.persist_assistant_message` 落盘后取得 —
    /// 与正常路径完全一致，保证 messages.jsonl 不丢条。
    async fn emit_terminal_error_message_and_idle(
        &self,
        executor: &dyn RuntimeLlmExecutor,
        session_id: &SessionId,
        run_id: &RunId,
        conversation_id: &str,
        error_text: &str,
    ) -> anyhow::Result<()> {
        // Step 7：持久化 error 占位为一条 assistant message（与正常路径同模式）
        let message_id = executor
            .persist_assistant_message(
                conversation_id,
                error_text,
                &[],
                &[],
                &[],
                &[],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Step 8a：MessagePersisted（前端 message:updated 渲染气泡）
        self.event_bus
            .emit(RuntimeEvent::message_persisted(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": error_text }),
            ))
            .await?;

        // Step 8b：StreamDone（前端 streamingState.isStreaming = false）
        self.event_bus
            .emit(RuntimeEvent::stream_done(
                session_id.clone(),
                run_id.clone(),
            ))
            .await?;

        // Step 8c：AgentIdle（前端解锁输入框）
        // scope 与正常路径行 2601 一致：Primary
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::AgentIdle {
                    agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                    scope: AgentIdleScope::Primary,
                },
            ))
            .await?;

        Ok(())
    }
```

注意：
- 新方法签名拿 `executor: &dyn RuntimeLlmExecutor` 是因为 `Err` 分支里 `executor` 已是 `&dyn` ref（行 1996 的 `executor.as_ref()` 传下来），不需要 `Arc` 克隆。
- `AgentId::new(format!("agent-{}", run_id.as_str()))` 这种构造方式与行 2599-2601 完全一致，不引入新模式。
- 不调用 `mark_idle_and_maybe_emit_pending` —— 错误路径不应该让 supervisor 标 idle 触发 path-A continuation（错误不是"成功完成的 turn"）。这与 spec D' 哲学"错误不发回 LLM"一致。

- [ ] **Step 2: 加方法所需的 use 导入（如果还没有）**

文件顶部已导入 `AgentIdleScope, RunningTool, RuntimeEvent, RuntimeEventKind`（行 32），不用改。但需确认 `SessionId` / `RunId` 已在 scope 里：

```bash
grep -n "use crate::runtime::ids" src-tauri/src/runtime/chat/chat_turn_driver.rs | head -3
```

Expected: 输出包含 `SessionId` / `RunId` 的 import（行号视当前文件而定）。如果没有，在 `use` 段补：

```rust
use crate::runtime::ids::{AgentId, RunId, SessionId};
```

注意 `AgentId` 也要在导入列表里（构造 `AgentIdle` 时用）。

- [ ] **Step 3: cargo check 验证编译**

```bash
cd src-tauri && cargo check
```

Expected: PASS（新方法暂时是 `dead_code`，但应通过编译）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "refactor(stream-error): extract emit_terminal_error_message_and_idle helper

Encapsulate Step 6-8 emit triplet (MessagePersisted / StreamDone /
AgentIdle) so the two error branches (Err and Err(PromptTooLong)) can
share a single fallthrough path in the next commit.

No call sites yet — pure addition, dead code warning suppressed by
upcoming wiring."
```

---

## Task 3: 接通 `Err(err)` 分支（修主白屏）

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:2071-2078`

- [ ] **Step 1: 替换 `Err(err)` 分支实现**

将原代码（行 2071-2078）：

```rust
                Err(err) => {
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );
                    return Err(anyhow::anyhow!("{}", err));
                }
```

改为：

```rust
                Err(err) => {
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );

                    // PR1 修白屏：补发 Step 6-8 三件套，让前端 chat 区显示错误占位
                    // 而不是空白。占位文本 PR2 会换成结构化 error 字段。
                    // spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR1
                    let error_text = format!(
                        "抱歉，AI 服务暂时无法响应（已自动尝试多次）。请稍后再试，或换个方式提问。\n\n[错误详情：{}]",
                        err
                    );
                    if let Err(emit_err) = self
                        .emit_terminal_error_message_and_idle(
                            executor,
                            &session_id,
                            &run_id,
                            conversation_id.as_str(),
                            &error_text,
                        )
                        .await
                    {
                        log::error!(
                            "[chat_turn_driver] failed to emit terminal error events on stream Err: {}",
                            emit_err
                        );
                    }

                    return Err(anyhow::anyhow!("{}", err));
                }
```

注意：
- emit 失败用 `log::error!` 而不是 `?` 传播 —— 因为我们仍要 `return Err(原错误)`（保持 driver 错误传播契约不变）。如果 emit 也失败，至少保留原错误信息。
- `error_text` 的中文文案对齐 spec §3.3 callout 范例（PR2 callout 渲染时会把 `[错误详情：...]` 部分隐藏到展开区）。
- `executor` / `session_id` / `run_id` / `conversation_id` 都是这个 scope 内已可用的变量（参考行 1996, 1323-1500 之间的 `let session_id = turn.session_id().clone()` 等绑定）。

- [ ] **Step 2: 跑 review 测试确认通过**

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events -- --nocapture
```

Expected: **PASS**（两个测试都应该通过 —— `Err(err)` 分支现在 emit 三件套了）。

- [ ] **Step 3: 跑现有 review 套件确认未退化**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

Expected: 全部 PASS。重点关注：
- `review_chat_history_persistence_test` — 错误消息也会落盘，确认 history persist 行为不变
- `review_tauri_event_adapter_test` — 三件套事件映射不变
- `s4_driver_loop_test::driver_s4_loop_content_complete` — 正常路径（成功）不受影响

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "fix(stream-error): emit MessagePersisted/StreamDone/AgentIdle before Err return

修复客户白屏 bug：当 run_llm_step 返回 Err（chunk timeout 重试耗尽 /
network error / 其他 LLM 错误）时，driver 现在会先调
emit_terminal_error_message_and_idle 补 Step 6-8 三件套，让前端
chat 区显示错误占位 assistant 气泡，再 return Err 向上传播原错误。

行为契约保持不变：
- driver 仍返回 Err（错误传播给上层）
- emit 失败不掩盖原错误（log::error! 而非 ? 传播）

Repro:
  1. echo 'true' > /tmp/aijia_hang_stream  (debug stash 中的复现脚本)
  2. pnpm dev:with-pilot
  3. 发消息 → 90s × 10 chunk timeout 后, 对话区有错误气泡, 不再白屏

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR1
Bug context: docs/debug/streaming-error-whitescreen-fix-context.md"
```

---

## Task 4: 接通 `Err(TurnError::PromptTooLong)` 分支

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:2054-2069`（在 emit StreamError 之后、return 之前补三件套）

PromptTooLong 分支已经有 emit `StreamError` 的逻辑（行 2055-2064），但**也**���了 Step 6-8 三件套，导致 PromptTooLong 场景一样白屏。

- [ ] **Step 1: 在 PromptTooLong 分支 return 前补 emit 三件套**

将原代码（行 2054-2069）：

```rust
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            session_id.clone(),
                            run_id.clone(),
                            RuntimeEventKind::StreamError {
                                error: message.clone(),
                                raw_error: Some("prompt_too_long".to_string()),
                            },
                        ))
                        .await?;
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );
                    return Err(anyhow::anyhow!(message));
```

改为：

```rust
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            session_id.clone(),
                            run_id.clone(),
                            RuntimeEventKind::StreamError {
                                error: message.clone(),
                                raw_error: Some("prompt_too_long".to_string()),
                            },
                        ))
                        .await?;
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );

                    // PR1 修白屏：补发 Step 6-8 三件套（与 Err(err) 分支同模式）
                    // PromptTooLong 场景的占位文案要明确告诉用户怎么恢复。
                    // spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR1
                    let error_text = format!(
                        "对话上下文已超出模型限制。请新建会话或精简历史后再试。\n\n[错误详情：{}]",
                        message
                    );
                    if let Err(emit_err) = self
                        .emit_terminal_error_message_and_idle(
                            executor,
                            &session_id,
                            &run_id,
                            conversation_id.as_str(),
                            &error_text,
                        )
                        .await
                    {
                        log::error!(
                            "[chat_turn_driver] failed to emit terminal error events on PromptTooLong: {}",
                            emit_err
                        );
                    }

                    return Err(anyhow::anyhow!(message));
```

注意：
- 占位文案与通用 `Err(err)` 不同 —— PromptTooLong 是用户可恢复的错误（新建会话 / 精简历史），文案要给指引
- emit 顺序：先 StreamError（保持原有 raw_error 标记给前端区分类型）→ 然后三件套（让 chat 区有气泡）

- [ ] **Step 2: 给 PromptTooLong 也加 review 测试**

在 `src-tauri/tests/review_stream_error_terminal_events.rs` 末尾追加：

```rust
struct PromptTooLongExecutor;

#[async_trait]
impl RuntimeLlmExecutor for PromptTooLongExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Err(TurnError::PromptTooLong(
            "Context too long: 250000 / 200000 tokens".to_string(),
        ))
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
        _thinking_blocks: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-ptl-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn driver_emits_message_persisted_on_prompt_too_long() {
    // PromptTooLong 触发 reactive compact 链路；最终 compact 也救不回来时，
    // driver 必须 emit 三件套，让前端 chat 区显示"上下文超限"占位气泡而不是白屏.
    let executor = Arc::new(PromptTooLongExecutor);
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-prompt-too-long");
    let request = ChatTurnRequest::new("conv-prompt-too-long", "long input", vec![]);

    let _result = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();

    // PromptTooLong 场景应该既 emit StreamError（已有，区分错误类型）也 emit 三件套（PR1 新增）
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::StreamError { raw_error, .. } if raw_error.as_deref() == Some("prompt_too_long")
        )),
        "PromptTooLong should still emit StreamError with raw_error=prompt_too_long"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::MessagePersisted { .. }
        )),
        "PromptTooLong should emit MessagePersisted (PR1 fix)"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            RuntimeEventKind::StreamDone
        )),
        "PromptTooLong should emit StreamDone (PR1 fix)"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::AgentIdle { .. }
        )),
        "PromptTooLong should emit AgentIdle (PR1 fix)"
    );
}
```

- [ ] **Step 3: 跑测试确认全部通过**

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events -- --nocapture
```

Expected: 3 个测试全 PASS。

- [ ] **Step 4: 跑全量 review 套件确认未退化**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/tests/review_stream_error_terminal_events.rs
git commit -m "fix(stream-error): emit terminal events on PromptTooLong path

PromptTooLong 分支之前虽 emit StreamError 但也漏了 Step 6-8，
同样会白屏。修复方式与 Err(err) 分支一致：emit StreamError
之后追加 emit_terminal_error_message_and_idle 调用。

文案区别：通用错误说'AI 暂时无法响应'，PromptTooLong 说
'上下文超出限制，请新建会话或精简历史'，给用户明确恢复路径。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR1"
```

---

## Task 5: 全量回归测试 + 编译检查

**Files:**
- 无（验证步骤）

- [ ] **Step 1: cargo check 全工程**

```bash
cd src-tauri && cargo check --all-targets 2>&1 | tail -20
```

Expected: PASS（无 warning，无 dead_code 提示 —— 新方法已被两处调用）。

- [ ] **Step 2: 跑 review_ 全套件**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tee /tmp/pr1-review-test.log | tail -50
```

Expected: 全部 PASS。重点关注以下不变式：
- `review_chat_history_persistence_test` — 错误占位 message 也走 persist_assistant_message 落盘，messages.jsonl 写入路径不变
- `review_backend_event_payload_test` — `message:updated` payload 结构不变（PR1 不改 schema）
- `review_tauri_event_adapter_test` — `MessagePersisted` / `StreamDone` / `AgentIdle` 三件套映射规则不变

- [ ] **Step 3: 跑 s4_driver_loop_test 确认主循环未退化**

```bash
cd src-tauri && cargo test --test s4_driver_loop_test -- --nocapture 2>&1 | tail -30
```

Expected: 全部 PASS（含 `driver_s4_loop_content_complete` / `driver_s4_loop_cancelled` / `driver_s4_loop_tool_calls_then_content_complete`）。

- [ ] **Step 4: 提交（如有 fixup）**

如果上述任一步发现新问题，**回到 Phase 1 of systematic-debugging**，不直接打补丁。

如果都通过了，无需新 commit。

---

## Task 6: 手测复现 + 文档收尾

**Files:**
- 无新增文件（手测验证）

- [ ] **Step 1: 启动 dev server**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm tauri:dev
```

等待 Tauri 窗口打开 + 编译完成（约 30s-2min）。

- [ ] **Step 2: 触发 chunk timeout 复现脚本**

参考 `docs/debug/streaming-error-whitescreen-fix-context.md` 行 41-48 的复现方法。**注意**：复现脚本本身在 stash 里（`stash@{0}: WIP on main: wip: chunk timeout repro debug code`），不在主仓 —— 不要恢复 stash 到分支上，PR1 的验证可以用更轻的方式：

替代方案：直接用真实网络环境测一个**已知会触发 chunk timeout** 的场景：
- 给 AI 发"帮我处理这段大文档"+ 长文本（贴入 50KB+ 的纯文本）
- 让 LLM 工具调用 + 长输出场景，等待中途 chunk timeout（90s × 10 = 6.4min 退避后触发）

或者更可控的方式：在 dev 环境**临时**改 `transport/tauri_commands/chat.rs:51` `MAX_STREAM_RETRIES = 1` + `chunk_timeout_secs = 5` 让重试快速耗尽。**这种修改不要 commit，仅本地验证**。

- [ ] **Step 3: 验收对话区表现**

| 期望 | 实际 |
|---|---|
| chunk timeout 重试耗尽后，对话区出现一条 assistant 气泡 | ✅ / ❌ |
| 气泡内容是中文错误占位文案（"AI 服务暂时无法响应..."） | ✅ / ❌ |
| 输入框解锁，可继续输入新消息 | ✅ / ❌ |
| 不再白屏（之前 `last-reply` 返回 None / msgs=3 无 assistant 的现象消失） | ✅ / ❌ |
| 后续消息仍能正常发送（agent 不卡死） | ✅ / ❌ |

- [ ] **Step 4: 复制错误日志确认 emit 顺序正常**

打开 dev server 终端日志，搜索：

```bash
# 在 dev server 输出里找
grep -E "MessagePersisted|StreamDone|AgentIdle|run_chat_turn finished" path/to/dev.log | tail -10
```

Expected: 看到顺序为 `MessagePersisted → StreamDone → AgentIdle �� run_chat_turn finished ok=false`（顺序不能变）。

- [ ] **Step 5: 收尾 commit**

如果手测全过，写一条手测确认 commit（不改代码，只补一个简短记录）：

```bash
git log --oneline -5
echo "PR1 manual verification passed: chunk timeout reproduced, error bubble displayed, no white screen."
```

不需要新 commit，PR1 就此完成。

---

## 自审清单

- [x] **Spec coverage**：spec §四 PR1 列了 5 项要求，本 plan 覆盖：
  - "补 Err 分支三件套" → Task 3
  - "补 PromptTooLong 分支三件套" → Task 4
  - "文本占位（不引入 error 字段）" → Task 2/3/4 都用 `error_text: String`
  - "不动 finalize_content 签名" → 本 plan 完全没碰 `post_process.rs`
  - "复现脚本验证不再白屏" → Task 6
- [x] **Placeholder scan**：每个 Step 都有完整代码块或具体命令，无 TBD/TODO/"以此类推"
- [x] **Type consistency**：
  - `emit_terminal_error_message_and_idle` 在 Task 2 定义、Task 3/4 调用，签名一致
  - `error_text: &str` 参数在所有调用点都是 `&error_text`（owned String 借用）
  - `executor: &dyn RuntimeLlmExecutor` 与现有 `run_chat_turn_s4` 行 1996 同模式
  - `AgentId::new(format!("agent-{}", run_id.as_str()))` 与正常路径行 2599-2601 完全一致

---

## 风险与回滚策略

**风险 1**：`emit_terminal_error_message_and_idle` 内部 emit 失败（如 RuntimeEventBus 已 dropped），但通过 `log::error!` 兼容了 —— 不掩盖原 driver Err，回滚不影响主路径。

**风险 2**：`persist_assistant_message` 把错误占位写盘可能让 messages.jsonl 多一条"假 assistant 消息"。这是**期望行为** —— claude-code-best 也持久化错误消息（"伤疤"），spec §3.2 守卫规则明确"持久化 → 写盘保留"。PR2 会把它升级为带 `error` 字段的结构化消息。

**回滚**：只需 revert Task 3 + Task 4 的代码 commit，保留 Task 1（review 测试）+ Task 2（helper 方法）作为后续 PR 重做的基础。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-28-pr1-stream-error-whitescreen-fix.md`. Two execution options:

**1. Subagent-Driven (recommended)** - 每 task 派一个新 subagent 执行 + task 间 review，迭代速度快

**2. Inline Execution** - 用 executing-plans 在本会话顺序跑，带 checkpoint review

Which approach?
