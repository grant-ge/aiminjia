# Phase 1 Session Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 `SessionId`/`RunId`/`AgentId`/`ToolCallId` 身份模型、Identity Mapping、`TurnState`、`SessionRuntime`、`QueryEngine` 和 `TauriEventAdapter`，并直接替换 `chat.rs` 主编排路径。

**Architecture:** runtime 成为主编排中心，`chat.rs` 退化为 transport adapter。内部只使用结构化 `RuntimeEvent`，外部前端继续消费 legacy Tauri events。`LlmGateway` 降级为 provider adapter。

**Tech Stack:** Rust, Tauri, cargo test, legacy event adapter, file-based storage bridge

## 当前实际状态（2026-04-10）

- 状态：大部分完成
- 已落地：`runtime/ids.rs`、`runtime/identity.rs`、`runtime/state.rs`、`runtime/events.rs`、`runtime/event_bus.rs`、`runtime/session_runtime.rs`、`runtime/query_engine.rs`、`transport/tauri_event_adapter.rs`
- 已落地：`commands/chat.rs` 已瘦身为 command adapter，`transport/tauri_commands/chat.rs` 已成为真实 transport 入口
- 已落地：`llm/gateway.rs` 已通过 `RuntimeRunRegistry` 下放 busy/cancel 真相源
- 已验证：`runtime_identity_mapping_test`、`turn_state_test`、`tauri_event_adapter_test`、`send_message_runtime_path_test`、`session_runtime_executor_test` 已通过
- 未完成：真实发送主循环仍主要在 `transport/tauri_commands/chat/chat_runtime_impl.rs`，还没完全由 `QueryEngine` 接管

---

**Phase constraints:**
- 第 1~2 期 `SessionId` 在值上直接复用现有 `conversation_id` 字符串，避免前两期立即改存储键。
- 新 runtime 模块、`RunController`、`RuntimeEvent`、新增 store 只能传 `SessionId`，不能把裸 `conversation_id` 当运行态真相源。
- transport / legacy payload / 老文件键仍可继续携带 `conversation_id`，但只允许通过显式 compatibility field 传递。
- Runtime 层禁止引入 `tauri::*`；需要 `AppHandle` 的地方必须通过 adapter 或 trait 注入。
- 所有 TDD 示例都必须因 unresolved import、missing method、或行为错误而真实失败。

---

### Task 1: 建立 identity types 与 Identity Mapping

**Files:**
- Create: `src-tauri/src/runtime/mod.rs`
- Create: `src-tauri/src/runtime/ids.rs`
- Create: `src-tauri/src/runtime/identity.rs`
- Test: `src-tauri/tests/runtime_identity_mapping_test.rs`

- [x] **Step 1: 写失败测试，要求 Identity Mapping 规则可编译表达**

```rust
use app_lib::runtime::identity::IdentityMapping;

#[test]
fn phase1_reuses_legacy_conversation_id_as_session_id() {
    let mapping = IdentityMapping::from_legacy_conversation_id("conv-1".to_string());

    assert_eq!(mapping.session_id.as_str(), "conv-1");
    assert_eq!(mapping.legacy_conversation_id.as_deref(), Some("conv-1"));
}
```

- [x] **Step 2: 再写一个失败测试，约束 runtime 模块不能再要求裸 `conversation_id`**

```rust
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::identity::RuntimeIdentity;

#[test]
fn runtime_identity_uses_session_id_as_primary_key() {
    let identity = RuntimeIdentity::new(
        SessionId::new("conv-1".to_string()),
        RunId::new("run-1".to_string()),
    );

    assert_eq!(identity.session_id().as_str(), "conv-1");
    assert_eq!(identity.run_id().as_str(), "run-1");
}
```

- [x] **Step 3: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_identity_mapping_test -- --nocapture`
Expected: FAIL with unresolved import `app_lib::runtime::identity`

- [x] **Step 4: 写最小 identity types 与 mapping 实现**

```rust
// src-tauri/src/runtime/ids.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(String);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RunId(String);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentId(String);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ToolCallId(String);

// src-tauri/src/runtime/identity.rs
pub struct IdentityMapping {
    pub session_id: SessionId,
    pub legacy_conversation_id: Option<String>,
}

pub struct RuntimeIdentity {
    session_id: SessionId,
    run_id: RunId,
}
```

- [x] **Step 5: 在文档和代码注释中固化 Identity Mapping 约束**

```text
- Phase 1~2: SessionId.value == legacy conversation_id
- Runtime logs/events/stores use SessionId
- Legacy payload keeps conversation_id as compatibility field
- New runtime modules must not take `conversation_id: String` as required input
```

- [x] **Step 6: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_identity_mapping_test -- --nocapture`
Expected: PASS

- [x] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/mod.rs src-tauri/src/runtime/ids.rs src-tauri/src/runtime/identity.rs src-tauri/tests/runtime_identity_mapping_test.rs
git commit -m "feat: add runtime identity mapping model"
```

### Task 2: 引入 `TurnState` 并落实运行态真相源

**Files:**
- Create: `src-tauri/src/runtime/state.rs`
- Modify: `src-tauri/src/runtime/identity.rs`
- Test: `src-tauri/tests/turn_state_test.rs`

- [x] **Step 1: 写失败测试，要求 `TurnState` 以 runtime identity 为真相源**

```rust
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::state::TurnState;

#[test]
fn turn_state_keeps_legacy_conversation_id_only_as_compatibility_field() {
    let mapping = IdentityMapping::from_legacy_conversation_id("conv-1".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-1".to_string()), "hello".into());

    assert_eq!(turn.session_id().as_str(), "conv-1");
    assert_eq!(turn.legacy_conversation_id(), Some("conv-1"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test turn_state_test -- --nocapture`
Expected: FAIL because `TurnState` does not exist yet

- [x] **Step 3: 写最小 `TurnState`**

```rust
use crate::runtime::ids::{AgentId, RunId, SessionId, ToolCallId};
use crate::runtime::identity::IdentityMapping;
use tokio_util::sync::CancellationToken;

pub struct TurnState {
    session_id: SessionId,
    run_id: RunId,
    legacy_conversation_id: Option<String>,
    agent_id: Option<AgentId>,
    user_input: String,
    pending_assistant_output: String,
    active_tool_call: Option<ToolCallId>,
    cancellation: CancellationToken,
}
```

- [x] **Step 4: 明确禁止继续把 `conversation_id` 当运行态真相源的模块**

```text
至少以下模块禁止新增 `conversation_id` 主键入参：
- src-tauri/src/runtime/**
- future RunStore / TaskStore / ToolCallStore
- SessionRuntime / QueryEngine / RuntimeEvent

允许保留的兼容边界：
- src-tauri/src/commands/chat.rs transport payload
- legacy Tauri event payload
- old storage key read path
```

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test turn_state_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/state.rs src-tauri/src/runtime/identity.rs src-tauri/tests/turn_state_test.rs
git commit -m "feat: add turn state and runtime truth-source rules"
```

### Task 3: 引入 `RuntimeEventBus` 与 `TauriEventAdapter`

**Files:**
- Create: `src-tauri/src/runtime/events.rs`
- Create: `src-tauri/src/runtime/event_bus.rs`
- Create: `src-tauri/src/transport/tauri_event_adapter.rs`
- Test: `src-tauri/tests/tauri_event_adapter_test.rs`

- [x] **Step 1: 写失败测试，验证内部事件映射到 legacy 事件**

```rust
use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

#[test]
fn maps_runtime_stream_delta_to_legacy_streaming_delta() {
    let event = RuntimeEvent::stream_delta(
        SessionId::new("conv-1".into()),
        RunId::new("run-1".into()),
        "hi".into(),
    );

    let mapped = map_runtime_event(&event).expect("legacy adapter should expose stream delta");

    assert_eq!(mapped.name, "streaming:delta");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tauri_event_adapter_test -- --nocapture`
Expected: FAIL with unresolved `event_bus` or `map_runtime_event`

- [x] **Step 3: 写最小事件模型与 adapter**

```rust
pub enum RuntimeEventKind {
    RunStarted,
    StreamDelta { content: String },
    StreamDone,
    ToolCallExecuting { tool_name: String, tool_call_id: ToolCallId },
    ToolCallCompleted { tool_call_id: ToolCallId },
    MessagePersisted { message_id: String },
    AgentIdle { agent_id: AgentId },
}

pub struct RuntimeEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub kind: RuntimeEventKind,
}
```

- [x] **Step 4: 把 legacy 事件名锁定为 Phase 0 审计结果**

```text
- streaming:delta
- streaming:done
- tool:executing
- tool:completed
- message:updated
- agent:idle
```

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tauri_event_adapter_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/events.rs src-tauri/src/runtime/event_bus.rs src-tauri/src/transport/tauri_event_adapter.rs src-tauri/tests/tauri_event_adapter_test.rs
git commit -m "feat: add runtime event bus and tauri event adapter"
```

### Task 4: 直接替换 `chat.rs` 主编排路径，并降级 `LlmGateway`

**Files:**
- Create: `src-tauri/src/runtime/session_runtime.rs`
- Create: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/llm/gateway.rs`
- Test: `src-tauri/tests/send_message_runtime_path_test.rs`

- [x] **Step 1: 写失败测试，要求 `send_message` 通过 `SessionRuntime` 发出 legacy 事件**

```rust
use app_lib::commands::chat::testsupport::run_send_message_through_runtime;

#[tokio::test]
async fn send_message_emits_legacy_events_via_runtime_adapter() {
    let trace = run_send_message_through_runtime("conv-1", "hello")
        .await
        .expect("runtime path should execute");

    assert_eq!(
        trace.event_names(),
        vec!["streaming:delta", "message:updated", "streaming:done"]
    );
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test send_message_runtime_path_test -- --nocapture`
Expected: FAIL because `SessionRuntime` path and testsupport harness are not wired

- [x] **Step 3: 写最小 `SessionRuntime` / `QueryEngine` 并接入 `chat.rs`**

```rust
pub struct SessionRuntime {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
}

impl SessionRuntime {
    pub async fn run_turn(&self, turn: &mut TurnState) -> anyhow::Result<()> {
        self.query_engine.run(turn, &self.event_bus).await
    }
}
```

- [x] **Step 4: 将 `chat.rs` 收缩为 transport adapter**

```text
chat.rs 只保留：
- payload 解析
- auth / permission 前置检查调用
- runtime input 构建
- 调用 SessionRuntime
- 将 RuntimeEvent 交给 TauriEventAdapter 发出 legacy 事件
```

- [x] **Step 5: 降级 `LlmGateway` 为 provider adapter**

```text
迁移掉以下运行态职责：
- active_tasks
- busy / set_busy
- cancel routing

保留：
- provider 选择
- request/stream 调用
- provider-level response normalization
```

- [x] **Step 6: 运行 Phase 1 回归**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_identity_mapping_test turn_state_test tauri_event_adapter_test send_message_runtime_path_test -- --nocapture`
Expected: PASS

- [x] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/session_runtime.rs src-tauri/src/runtime/query_engine.rs src-tauri/src/commands/chat.rs src-tauri/src/llm/gateway.rs src-tauri/tests/send_message_runtime_path_test.rs
git commit -m "refactor: route send_message through session runtime"
```

## Definition of Done

- Identity Mapping 已明确：前两期 `SessionId` 复用 `conversation_id` 值，但 runtime 真相源已经切到 `SessionId`。
- `TurnState`、`RuntimeEventBus`、`SessionRuntime` 可独立于 Tauri 类型工作。
- `chat.rs` 不再承担主编排逻辑。
- `LlmGateway` 不再持有 run/busy/cancel 等 runtime 状态。
