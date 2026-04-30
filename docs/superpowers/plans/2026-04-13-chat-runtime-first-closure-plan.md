# 聊天主链路 Runtime-First 收口 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 lotus-app 的真实 `send_message` 主链路从“`SessionRuntime` 只做 preflight 后再完整委托 legacy executor”收口为“runtime 持有真实聊天编排、transport 只做宿主桥接与兼容 helper”的状态，并保住 Workspace-First 与前端 legacy-compatible 事件协议。

**Architecture:** 保留 `commands/chat.rs -> TauriChatCommandAdapter` 外部入口不变，但让 `SessionRuntime::run_chat_request()` 调用 runtime-owned chat driver 作为真实 orchestrator。第一步先迁回 orchestration ownership，允许 `chat_runtime_impl.rs` 暂留 host-bound / compatibility helper；第二步再继续收拢 tool dispatch、schema surface 和 legacy helper 残留，最终由 runtime + dispatcher + event bus 形成单一真相源。

**Tech Stack:** Rust / Tokio / async_trait / anyhow / serde_json / cargo test / Tauri v2 兼容事件适配

---

## 0. Scope, Truth Source, Migration Boundary

### 0.1 In Scope

- `SessionRuntime::run_chat_request()` 不再是 `preflight + delegate`
- runtime 新增 chat orchestration 归属点（建议 `src-tauri/src/runtime/chat/`）
- 真实聊天中的工具执行主路径收口到 runtime dispatcher / capability contract
- transport 只保留 host wiring / event compatibility / helper provider 角色
- targeted tests 能明确证明“真实主链路已切过来”

### 0.2 Out of Scope

- 不重写整个 LLM provider 层
- 不重做前端 UI 或事件协议
- 不顺手改所有 legacy tool handlers
- 不扩展新的 Workspace-First 功能
- 不做新的 Skill / Workflow 专项

### 0.3 Truth Source After Closure

- **聊天 turn lifecycle**：`src-tauri/src/runtime/session_runtime.rs`
- **聊天 orchestration loop**：`src-tauri/src/runtime/chat/*`
- **工具执行合同**：`src-tauri/src/runtime/query_engine.rs` + `src-tauri/src/runtime/tools/*`
- **结构化事件真相源**：`src-tauri/src/runtime/event_bus.rs`
- **旧前端协议映射**：`src-tauri/src/transport/tauri_event_adapter.rs`

### 0.4 Phase-1 / Phase-2 Migration Boundary

#### 本轮第一步允许暂留在 `chat_runtime_impl.rs` 的内容

- 纯 Tauri host 绑定逻辑
- 与 legacy-compatible 事件协议直接相关的兼容 helper
- 被 runtime 调用的上下文装配辅助函数，只要调用时机由 runtime 决定

#### 本轮第一步必须迁出 transport ownership 的内容

- 聊天主循环的起止控制
- 何时进入工具调用回合
- 何时写回 assistant/tool message
- 何时结束 streaming / terminal state
- 真实 tool execution 主路径的选择权

#### 判定未完成收口的红线

以下任一情况仍算未完成：

- `SessionRuntime::run_chat_request()` 仍是 `turn.mark_executor_backed() -> run_turn() -> executor.run_chat_turn(request)`
- `chat_runtime_impl.rs` 仍持有 full orchestration while runtime only preflights
- transport 仍直接决定真实 tool execution 主路径
- targeted tests 只能证明“输出没变”，不能证明“ownership 已迁移”

---

## 1. File Map

### 1.1 Runtime Core

- Modify: `src-tauri/src/runtime/session_runtime.rs`
  - 当前问题：`run_chat_request()` 在 executor-backed 路径仍是 preflight + full delegate
  - 目标职责：持有 run lifecycle、构造 turn、调用 runtime-owned chat driver、注入 authorized workspace

- Modify: `src-tauri/src/runtime/query_engine.rs`
  - 当前问题：executor-backed 时只发 `StreamStarted`，不是聊天主编排器的一部分
  - 目标职责：作为 runtime chat driver 的工具执行与 capability 注入入口，不再只是 preflight 特例

- Create: `src-tauri/src/runtime/chat/mod.rs`
  - 目标职责：runtime chat orchestration 模块入口、导出 driver 与 helper trait

- Create: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - 目标职责：runtime-owned chat loop；由 runtime 控制 prompt/context build、LLM turn、tool round、message persistence、terminal state

- Create or Modify: `src-tauri/src/runtime/chat/context_builder.rs`
  - 目标职责：承接与 runtime chat driver 直接相关的 context / prompt / visible tool surface 组装逻辑

### 1.2 Transport / Host Bridge

- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
  - 目标职责：仍作为外部命令 adapter，但不再构造“runtime preflight + legacy full delegate”结构

- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
  - 当前问题：实际持有主聊天编排
  - 目标职责：降级为 host-bound / compatibility helper 集合，不能再作为主 orchestrator

- Modify: `src-tauri/src/commands/chat.rs`
  - 目标职责：入口保持不变，只更新 wiring

### 1.3 Tests

- Modify: `src-tauri/tests/review_runtime_executor_bypass_test.rs`
- Modify: `src-tauri/tests/send_message_runtime_path_test.rs`
- Create: `src-tauri/tests/chat_runtime_first_mainline_test.rs`
- Create: `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs`
- Re-run: `src-tauri/tests/workspace_first_agent_golden_path_test.rs`
- Re-run: `src-tauri/tests/builtin_runtime_registration_test.rs`
- Re-run: `src-tauri/tests/tool_runtime_integration_test.rs`

---

## 2. Working Rules

### 2.1 TDD Rule

每个执行单元都必须遵守：

1. 先写或先改失败测试
2. 跑 targeted test，确认当前实现下红灯
3. 只写最小实现让测试变绿
4. 跑该单元最小验证命令
5. 再进入下一单元

### 2.2 Change Discipline

- 不允许在同一单元里同时做 ownership 迁移和大规模 unrelated cleanup
- 不允许通过重命名、包一层 helper、移动函数位置来伪装“ownership 已迁移”
- 不允许只靠 prompt / 文案 / 测试宽松断言让计划看起来通过
- 每个单元只改该单元文件清单内的文件；新增越界文件前先更新计划

### 2.3 Commit Discipline

每个 Task 完成并通过最小验证后提交一次。不要 squash 到最后统一提交。

---

## 3. Acceptance Criteria Mapping

### AC-1：真实聊天主链路不再由 `legacy_send_message_impl()` 直接主导
- 对应 Task 2 / Task 3 / Task 4

### AC-2：`SessionRuntime::run_chat_request()` 不再只是 preflight + delegate
- 对应 Task 2 / Task 3

### AC-3：真实聊天中的工具执行走 runtime dispatcher 合同
- 对应 Task 4

### AC-4：现有兼容事件协议保持可用
- 对应 Task 1 / Task 5

### AC-5：Workspace-First 关键回归继续通过
- 对应 Task 4 / Task 5

### AC-6：需要有能证明“真实主链路已切过来”的测试
- 对应 Task 1 / Task 3 / Task 4 / Task 5

---

## 4. Task Plan

### Task 1: 锁死旧结构与观测面（R1-A / R1-B / R1-C）

**Goal:** 先把当前 `executor-backed preflight + full delegate` 结构写成不能绕过的红灯，并固定 legacy-compatible 事件观测面。

**Files:**
- Modify: `src-tauri/tests/review_runtime_executor_bypass_test.rs`
- Modify: `src-tauri/tests/send_message_runtime_path_test.rs`
- Create: `src-tauri/tests/chat_runtime_first_mainline_test.rs`

**Do Not Do:**
- 不改生产代码
- 不提前新增 runtime/chat 实现
- 不放宽断言，只为了让当前代码“先绿掉”

- [ ] **Step 1: 收紧 bypass review 测试，让它明确反对 full delegate**

在 `src-tauri/tests/review_runtime_executor_bypass_test.rs` 新增或改造断言，目标从“executor-backed path 仍经过 runtime preflight”升级为“executor-backed path 不能把完整 turn orchestration 委托给 legacy executor”。

测试目标至少覆盖：

```rust
#[tokio::test]
async fn executor_backed_chat_path_should_not_delegate_full_turn_to_legacy_executor() {
    // arrange: runtime with recording executor
    // act: run_chat_request(...)
    // assert:
    // 1) legacy executor 不应再成为 full-turn owner
    // 2) runtime 必须拥有 streaming / terminal orchestration 的主控制权
}
```

- [ ] **Step 2: 运行 bypass review 测试，确认当前代码红灯**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test review_runtime_executor_bypass_test -- --nocapture
```

Expected:
- FAIL
- 失败原因应指向当前 `SessionRuntime::run_chat_request()` 仍然 `mark_executor_backed + run_turn + executor.run_chat_turn`

- [ ] **Step 3: 改造 runtime path 测试，让它要求 runtime chat driver 成为真实入口**

在 `src-tauri/tests/send_message_runtime_path_test.rs` 中补一个“主 orchestrator 已切换”的测试，不再只看事件存在，而要看 runtime-owned driver 是否成为入口。

测试目标：

```rust
#[tokio::test]
async fn send_message_runtime_path_should_use_runtime_chat_driver() {
    // assert runtime path 进入 runtime chat driver
    // 而不是 transport full orchestration
}
```

- [ ] **Step 4: 新建 mainline 测试文件，锁住 transport 不再是主 orchestrator**

创建 `src-tauri/tests/chat_runtime_first_mainline_test.rs`，至少包含两类断言：

```rust
#[tokio::test]
async fn runtime_chat_driver_should_own_turn_orchestration() {
    // runtime driver 决定 turn lifecycle
}

#[tokio::test]
async fn transport_chat_runtime_impl_should_not_be_full_orchestrator() {
    // transport 只能作为 helper / bridge
}
```

- [ ] **Step 5: 锁定事件观测面，不允许迁移后协议悄悄变掉**

在 `chat_runtime_first_mainline_test.rs` 或 `send_message_runtime_path_test.rs` 补一个 legacy-compatible event trace 断言，最少覆盖：

```text
RunStarted
StreamStarted
StreamDelta / ToolCallExecuting / ToolCallCompleted（按场景）
MessagePersisted
StreamDone
```

- [ ] **Step 6: 运行第一组 targeted tests，确认至少一组红灯**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test review_runtime_executor_bypass_test -- --nocapture && \
  cargo test --test send_message_runtime_path_test -- --nocapture
```

Expected:
- 至少一组 FAIL
- 失败信息能明确说明“真实主链路仍未切换到 runtime”

- [ ] **Step 7: 提交 Task 1 红灯基线**

Run:
```bash
git add \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_runtime_executor_bypass_test.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/send_message_runtime_path_test.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/chat_runtime_first_mainline_test.rs && \
  git commit -m "test(chat-runtime): lock runtime-first closure red tests"
```

**Exit Criteria:**
- 至少一组测试在当前代码下红灯
- 红灯能证明“ownership 未迁移”而不是普通实现错误
- event compatibility 观测面已固定

---

### Task 2: 新建 runtime chat driver 骨架并切断 `with_executor` full delegate（R2-A）

**Goal:** 创建 runtime-owned chat driver 骨架，让 `SessionRuntime::run_chat_request()` 不再是 `preflight + executor.run_chat_turn(request)`。

**Files:**
- Create: `src-tauri/src/runtime/chat/mod.rs`
- Create: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/session_runtime.rs`

**Do Not Do:**
- 不在这一步收完整 tool dispatch
- 不在这一步重写所有 context/prompt helper
- 不改 `chat_runtime_impl.rs` 里的大块实现细节，只先切 ownership

- [ ] **Step 1: 在 runtime 层声明 chat module 入口**

创建 `src-tauri/src/runtime/chat/mod.rs`，导出 driver：

```rust
pub mod chat_turn_driver;

pub use chat_turn_driver::RuntimeChatTurnDriver;
```

- [ ] **Step 2: 新建最小 runtime chat driver 骨架**

创建 `src-tauri/src/runtime/chat/chat_turn_driver.rs`，先定义最小结构与入口：

```rust
#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
}

impl RuntimeChatTurnDriver {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self { /* ... */ }

    pub async fn run_chat_turn(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
    ) -> Result<()> {
        // 第一轮只接住 orchestration ownership
        // 允许内部暂时调用 helper，但控制流必须在 runtime
        Ok(())
    }
}
```

- [ ] **Step 3: 调整 `SessionRuntime` 持有 runtime chat driver 或能构造它**

在 `src-tauri/src/runtime/session_runtime.rs` 中移除“executor-backed full delegate”路径。目标形态：

```rust
pub async fn run_chat_request(
    &self,
    request: ChatTurnRequest,
) -> std::result::Result<(), String> {
    let mapping = IdentityMapping::from_legacy_conversation_id(request.conversation_id.clone());
    let mut turn = TurnState::new(mapping, RunId::new(uuid::Uuid::new_v4().to_string()), request.content.clone());

    let driver = RuntimeChatTurnDriver::new(
        self.query_engine_for_session(turn.session_id()),
        self.event_bus.clone(),
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .map_err(|err| err.to_string())
}
```

- [ ] **Step 4: 删除或收窄 `with_executor` 路径，使其不再控制真实聊天主链路**

可接受的短期形态二选一：

1. 删除 `RuntimeTurnExecutor` / `turn_executor` 字段；或
2. 保留字段，但只允许 driver 内部以 helper 方式调用，`SessionRuntime` 不得直接 full delegate

不可接受形态：

```rust
turn.mark_executor_backed();
self.run_turn(&mut turn).await?;
executor.run_chat_turn(request).await
```

- [ ] **Step 5: 跑 Task 1 的红灯测试，验证失败点已向“driver 未完成实现”收敛**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test review_runtime_executor_bypass_test -- --nocapture && \
  cargo test --test send_message_runtime_path_test -- --nocapture && \
  cargo test --test chat_runtime_first_mainline_test -- --nocapture
```

Expected:
- 与 full delegate 相关的旧失败应消失或变化
- 新失败应收敛到 driver 细节尚未完成，而不是 ownership 仍在 legacy executor

- [ ] **Step 6: 提交 runtime chat driver 骨架切换**

Run:
```bash
git add \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/mod.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs && \
  git commit -m "refactor(chat-runtime): route chat turns through runtime driver"
```

**Exit Criteria:**
- `SessionRuntime::run_chat_request()` 不再直接 full delegate 到 legacy executor
- runtime chat driver 已成为真实入口
- 失败若仍存在，必须是 driver 逻辑未补齐，而不是 ownership 未迁移

---

### Task 3: 迁回 turn lifecycle、message persistence 与 terminal state 控制权（R2-B / R2-C）

**Goal:** 把聊天主循环的时序控制从 transport 收回 runtime，让 transport 仅保留 helper / bridge。

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

**Do Not Do:**
- 不在这一步完成全部 dispatcher/schema 收口
- 不改前端协议
- 不顺手大规模整理 `legacy_send_message_impl()` 的无关逻辑

- [ ] **Step 1: 把 turn lifecycle 的控制顺序写进 driver**

在 `chat_turn_driver.rs` 中实现最小顺序：

```rust
// 目标顺序示意
emit RunStarted
build runtime-owned turn context
start streaming
handle llm deltas / tool round placeholder
persist assistant/tool outputs
emit StreamDone / terminal events
```

重点不是一次性把所有细节写完，而是让“什么时候开始、什么时候结束、什么时候持久化”都由 runtime 决定。

- [ ] **Step 2: 从 transport 拆出仍由它主导的时序控制**

在 `chat_runtime_impl.rs` 中识别并迁出以下控制流：

- 何时开始真实聊天主循环
- 何时结束 streaming
- 何时写回 assistant / tool results
- 何时宣布 terminal state

将保留内容缩到 helper 级别，例如：

- build_visible_tool_defs(...)
- build_precompute_sandbox(...)
- build_workspace_context(...)
- host-bound emit / auth / storage wiring helper

- [ ] **Step 3: 把 transport wiring 改为“runtime call helper”，而不是“runtime wrapper transport orchestrator”**

在 `src-tauri/src/transport/tauri_commands/chat.rs` 中，确保 `TauriChatCommandAdapter` 的 wiring 目标变成：

```rust
commands/chat.rs::send_message
  -> TauriChatCommandAdapter::send_message
  -> SessionRuntime::run_chat_request(...)
  -> RuntimeChatTurnDriver
  -> helper(s) from chat_runtime_impl.rs when needed
```

不允许再是：

```text
SessionRuntime preflight -> transport legacy_send_message_impl full orchestration
```

- [ ] **Step 4: 更新 mainline 测试，证明 transport 不再是 full orchestrator**

完善 `chat_runtime_first_mainline_test.rs` 断言：

```rust
#[tokio::test]
async fn runtime_chat_driver_should_persist_messages_and_finish_streaming() {
    // runtime driver 控制 message persisted + stream done
}

#[tokio::test]
async fn transport_helpers_may_assist_but_must_not_own_turn_lifecycle() {
    // transport helper 可被调用，但不拥有时序控制
}
```

- [ ] **Step 5: 跑 R2 targeted tests，确认 ownership 迁移已变绿**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test review_runtime_executor_bypass_test -- --nocapture && \
  cargo test --test send_message_runtime_path_test -- --nocapture && \
  cargo test --test chat_runtime_first_mainline_test -- --nocapture
```

Expected:
- PASS
- 能明确证明 runtime 拥有 turn lifecycle / message persistence / terminal sequencing

- [ ] **Step 6: 提交 R2 ownership 迁移**

Run:
```bash
git add \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs && \
  git commit -m "refactor(chat-runtime): move turn orchestration ownership into runtime"
```

**Exit Criteria:**
- runtime 控制 turn lifecycle / message persistence / terminal state
- transport 不再是 full orchestrator
- R2 targeted tests 全绿

---

### Task 4: 收工具执行主路径、schema surface 与 Workspace-First 注入（R3）

**Goal:** 把真实聊天中的工具执行合同收进 runtime dispatcher，同时保住 authorized workspace 与 request-scoped tools。

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- Create: `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs`

**Do Not Do:**
- 不重写全部 tool registry 设计
- 不新增新的 Workspace-First 功能
- 不改前端事件协议

- [ ] **Step 1: 新建 production-path 测试，锁住 dispatcher 合同**

创建 `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs`，至少写出三类测试：

```rust
#[tokio::test]
async fn runtime_chat_mainline_dispatches_tools_via_runtime_dispatcher() {
    // 不允许 transport 主路径直接 ToolRegistry::execute(...)
}

#[tokio::test]
async fn runtime_chat_mainline_preserves_workspace_first_authorized_directory() {
    // authorized workspace 注入仍然存在
}

#[tokio::test]
async fn runtime_chat_mainline_emits_tool_events_once() {
    // tool executing / completed 不双发
}
```

- [ ] **Step 2: 运行新测试，确认当前代码或半迁移代码下先红灯**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test chat_runtime_dispatcher_production_path_test -- --nocapture
```

Expected:
- FAIL
- 失败原因应指向真实聊天 tool path 还没完全走 runtime dispatcher

- [ ] **Step 3: 在 runtime chat driver 中接入 dispatcher 主路径**

在 `chat_turn_driver.rs` 中，真实 tool round 应调用 runtime path，例如：

```rust
let result = self
    .query_engine
    .run_tool_with_bus(turn, &self.event_bus, tool_name)
    .await?;
```

要求：
- 真实聊天中的工具执行主路径经过 `QueryEngine` / `ToolDispatcher`
- tool events 由 runtime event bus 统一发出
- 不让 transport 直接主导 `ToolRegistry::execute(...)`

- [ ] **Step 4: 收 tool surface 的真相源**

如果当前 `build_visible_tool_defs(...)` 仍在 `chat_runtime_impl.rs`，则本步要么：

1. 将其迁入 `runtime/chat/context_builder.rs`；或
2. 保留 helper，但由 runtime 明确调用并控制暴露时机

关键约束：
- tool schema surface 由 runtime chat orchestration 决定
- transport 不能再以“我手头有哪些 helper / registry”决定主路径暴露面

- [ ] **Step 5: 保住 Workspace-First 注入**

确认 `SessionRuntime::query_engine_for_session(...)` + `QueryEngine::with_authorized_workspace(...)` 在真实聊天主链路上仍成立，目标行为：

```rust
capability.storage.authorized_workspace == Some(AuthorizedWorkspaceRef { ... })
```

同时验证 request-scoped tools 仍能从 runtime dispatcher 路径被访问。

- [ ] **Step 6: 跑 dispatcher 与 workspace-first targeted tests**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test chat_runtime_dispatcher_production_path_test -- --nocapture && \
  cargo test --test tool_runtime_integration_test -- --nocapture && \
  cargo test --test builtin_runtime_registration_test -- --nocapture && \
  cargo test --test workspace_first_agent_golden_path_test -- --nocapture
```

Expected:
- PASS
- 工具执行经 runtime dispatcher
- authorized workspace 保留
- tool events 无 double emit

- [ ] **Step 7: 提交 R3 dispatcher 收口**

Run:
```bash
git add \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs && \
  git commit -m "refactor(chat-runtime): route production tool calls through runtime dispatcher"
```

**Exit Criteria:**
- 真实聊天中的 tool execution 主路径进入 runtime dispatcher
- schema surface 由 runtime 控制
- Workspace-First authorized directory 继续注入
- tool events 不双发

---

### Task 5: 锁兼容事件与最终证据包（R4）

**Goal:** 在 ownership 与 dispatcher 已迁移后，确认 legacy-compatible 前端事件、Workspace-First、Atomic Tool 关键回归都保持成立，并形成最终 closure evidence。

**Files:**
- Modify: `src-tauri/tests/send_message_runtime_path_test.rs`
- Modify: `src-tauri/tests/chat_runtime_first_mainline_test.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`（仅在确有兼容问题时）

**Do Not Do:**
- 不重新打开 ownership 迁移
- 不新增新的 product feature
- 不借机重做前端消费协议

- [ ] **Step 1: 补最终 compatibility 测试断言**

在已有测试中锁住至少以下观测面：

```text
Daily chat, no tool:
RunStarted -> StreamStarted -> StreamDelta... -> MessagePersisted -> StreamDone

Chat with runtime tool:
RunStarted -> StreamStarted -> ToolCallExecuting -> ToolCallCompleted -> MessagePersisted -> StreamDone
```

- [ ] **Step 2: 补 no-double-emit 断言**

增加断言确保 terminal / tool events 不重复：

```rust
#[tokio::test]
async fn runtime_chat_should_not_double_emit_terminal_or_tool_events() {
    // assert counts == 1 for terminal/tool milestones
}
```

- [ ] **Step 3: 仅在必要时修正 `TauriEventAdapter` 映射**

如果测试显示 runtime events 与 legacy-compatible frontend events 的映射缺口真实存在，再修改 `src-tauri/src/transport/tauri_event_adapter.rs`。

禁止无证据改动 adapter。

- [ ] **Step 4: 跑最终 Rust targeted regression 套件**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test review_runtime_executor_bypass_test -- --nocapture && \
  cargo test --test send_message_runtime_path_test -- --nocapture && \
  cargo test --test chat_runtime_first_mainline_test -- --nocapture && \
  cargo test --test chat_runtime_dispatcher_production_path_test -- --nocapture && \
  cargo test --test workspace_first_agent_golden_path_test -- --nocapture && \
  cargo test --test builtin_runtime_registration_test -- --nocapture && \
  cargo test --test tool_runtime_integration_test -- --nocapture
```

Expected:
- PASS
- 所有 closure 关键证据成立

- [ ] **Step 5: 跑前端 Workspace-First 回归**

Run:
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  pnpm exec vitest run src/components/settings/WorkspaceAuthPanel.test.tsx src/components/settings/WorkspaceFirst.integration.test.tsx
```

Expected:
- PASS
- 前端仍能消费兼容协议，Workspace-First 关键路径无回归

- [ ] **Step 6: 提交 R4 compatibility close-out**

Run:
```bash
git add \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/send_message_runtime_path_test.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/chat_runtime_first_mainline_test.rs \
  /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs && \
  git commit -m "test(chat-runtime): close compatibility and regression evidence"
```

**Exit Criteria:**
- Rust targeted regression 套件全绿
- 前端 Workspace-First 回归全绿
- legacy-compatible event protocol 继续成立
- final evidence pack 可证明真实主链路已切换

---

## 5. Final Evidence Pack

关闭本专项前，必须同时拿到以下证据：

1. `review_runtime_executor_bypass_test` 证明 full delegate 模式已消失
2. `send_message_runtime_path_test` 证明 runtime path 真实进入 runtime chat driver
3. `chat_runtime_first_mainline_test` 证明 transport 不再是 full orchestrator
4. `chat_runtime_dispatcher_production_path_test` 证明 tool execution 主路径进入 runtime dispatcher
5. `workspace_first_agent_golden_path_test` 证明 Workspace-First 主路径仍成立
6. `builtin_runtime_registration_test` + `tool_runtime_integration_test` 证明 runtime tool contract 未回退
7. `WorkspaceFirst.integration.test.tsx` 证明前端兼容链路未坏

如果缺少其中任一条，就不能宣告“chat runtime-first closure 完成”。

---

## 6. Self-Review

### 6.1 Spec Coverage

- 运行时主链路 ownership 收口：Task 2 / Task 3
- transport 降级为 bridge/helper：Task 3
- tool execution 走 runtime dispatcher：Task 4
- Workspace-First 保持成立：Task 4 / Task 5
- 兼容事件协议保持可用：Task 1 / Task 5
- 可证明主链路已切换的 tests：Task 1 / Task 4 / Task 5

### 6.2 Placeholder Scan

- 无 `TBD` / `TODO` / “implement later”
- 每个 Task 都包含文件、动作、命令、预期结果、退出条件

### 6.3 Type / Naming Consistency

计划统一使用以下命名：
- `RuntimeChatTurnDriver`
- `run_chat_turn(...)`
- `chat_runtime_first_mainline_test`
- `chat_runtime_dispatcher_production_path_test`

执行时若实际代码已存在更合适命名，可统一替换，但必须在单个提交内保持一致。
