# P1-A 聊天工具回合 Runtime Dispatcher 收口计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 lotus-app 真实 `send_message` 主链路中的 **tool round** 从 `chat_runtime_impl.rs` 直接 `tool_registry.execute(...) + app.emit(...)` 的 legacy 路径，收口为 **runtime-owned tool dispatch**：由 runtime 统一执行 allowed-tools 判断、dispatcher 调度、capability 注入、tool lifecycle event 发射，并保持 Workspace-First 与现有前端兼容事件协议不回归。

**Architecture:** 保留当前 Phase-1 已完成的 terminal-event 修复成果（`MessagePersisted` / `StreamDone` 已经通过 runtime bus 到达 host），本专项不再争论 terminal state，而是单独解决 **T2**：真实聊天工具调用仍绕过 runtime。实现上允许外层 LLM stream/provider loop 暂时继续留在 `chat_runtime_impl.rs`，但**一旦模型产出 tool calls，执行 ownership 必须立即切到 runtime/tool-round driver + QueryEngine + ToolDispatcher**。transport 只负责 host glue、helper provider 和 legacy 兼容函数。

**Tech Stack:** Rust / Tokio / async_trait / anyhow / serde_json / cargo test / Tauri v2 兼容事件适配

---

## 0. 当前基线与问题定义

### 0.1 已确认的当前状态

- `T1`：`streaming:done via host` 已从红灯变绿
- `T3`：`RunId` 一致性已修复并保持绿灯
- `T4`：`message:updated via host` 已从红灯变绿
- **`T2`：tool dispatch via runtime 仍是红灯**，这是当前唯一剩余的主 blocker

### 0.2 当前真实断层

当前生产聊天主链路里，tool round 仍然是：

`LLM tool calls -> chat_runtime_impl.rs -> allowed_tools filter -> app.emit("tool:executing") -> tool_registry.execute(...) -> app.emit("tool:completed")`

对应代码事实：

- `chat_runtime_impl.rs` 直接发 `tool:executing`：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2747`
- `chat_runtime_impl.rs` 直接 `tool_registry.execute(...)`：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2828`、`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2877`
- `QueryEngine::run_tool_with_bus(...)` 当前只覆盖局部 runtime path，不是生产聊天 tool round 的真实 owner：`src-tauri/src/runtime/query_engine.rs:126`

### 0.3 这次专项要回答的唯一核心问题

**真实 `send_message` 的 tool round，是否已经切到 runtime dispatcher 主路径？**

如果答案仍然是否，则 chat-runtime-first 收口仍不能关闭。

---

## 1. Scope / Non-Goals / Truth Source

### 1.1 In Scope

- 真实聊天中的工具执行 ownership 迁移到 runtime
- `allowed_tools` / step tool filter 从 legacy loop 收口到 runtime tool-round driver
- 真实生产链路用 `ToolRegistry::to_runtime_dispatcher(...)` 构造 dispatcher
- 真实工具调用使用 runtime event bus 发射 `ToolCallExecuting` / `ToolCallCompleted`
- Workspace-First 已授权目录能力在真实 tool round 中继续可达
- 针对真实 adapter / production path 的 gating TDD

### 1.2 Out of Scope

- 不在本专项重写整个 LLM provider / stream loop
- 不在本专项重做前端 UI
- 不顺手迁移整个 `legacy_send_message_impl()` 到 runtime
- 不扩大 `CapabilityContext` 成第二个 `PluginContext`
- 不新开 Skill / Workflow 专项

### 1.3 本专项完成后的真相源

- **tool round orchestration**：`src-tauri/src/runtime/chat/*`
- **工具执行合同**：`src-tauri/src/runtime/query_engine.rs` + `src-tauri/src/runtime/tools/*`
- **tool lifecycle events**：`src-tauri/src/runtime/event_bus.rs`
- **前端兼容映射**：`src-tauri/src/transport/tauri_event_adapter.rs`
- **transport helper glue**：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

### 1.4 判定未完成的红线

以下任一情况仍算未完成：

- `chat_runtime_impl.rs` 仍直接 `tool_registry.execute(...)`
- `chat_runtime_impl.rs` 仍直接 `app.emit("tool:executing")` / `app.emit("tool:completed")`
- runtime 仍拿不到真实 tool args / tool_call_id，只能跑假调用
- 真实 adapter path 没有测试证明“tool round 已进 runtime”

---

## 2. 设计原则

### 2.1 增量边界

本专项是 **P1-A：tool round ownership migration**，不是“一次性把整段聊天 loop 全搬进 runtime”。

允许的增量边界：

- 外层 LLM stream loop 可以暂时还在 `chat_runtime_impl.rs`
- 但 **tool calls 一旦产生**，执行 ownership 必须切到 runtime
- `chat_runtime_impl.rs` 只能做：
  - tool call 输入整理
  - host/helper provider
  - runtime driver 需要的上下文装配

### 2.2 单一执行入口

同一个真实 tool call 的以下事项必须由同一条 runtime 链路负责：

- allowed-tools 判断
- dispatcher 构造与 dispatch
- capability 注入
- `ToolCallExecuting`
- `ToolCallCompleted`
- 输出归一化为 LLM 可消费的 tool result

### 2.3 不能接受的伪迁移

- 只是在 `chat_runtime_impl.rs` 外面包一层 helper，但里面仍直接 `tool_registry.execute(...)`
- 只把 `app.emit("tool:executing")` 改名或挪位置，不改变 ownership
- 只补局部 unit test，不补真实 `send_message` adapter path 测试
- 只让 runtime 记录事件，不让 subscriber / host 真收到工具事件

---

## 3. File Map

### 3.1 Runtime Core

- Modify: `src-tauri/src/runtime/query_engine.rs`
  - 当前问题：`run_tool_with_bus(...)` 只有 `tool_name`，没有真实 `tool_call_id` / `args` / 结果对象
  - 目标职责：提供真实生产 tool round 可调用的 runtime dispatch API

- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - 当前问题：executor-backed 路径不拥有 tool round
  - 目标职责：成为 runtime tool-round 的调用入口，哪怕外层 LLM loop 还在 legacy 层

- Create: `src-tauri/src/runtime/chat/tool_round_driver.rs`
  - 目标职责：接收 tool calls、allowed-tools、dispatcher、event bus，执行真实 tool round

- Optional Create/Modify: `src-tauri/src/runtime/chat/tool_round_types.rs`
  - 目标职责：定义 `RuntimeToolCallRequest` / `RuntimeToolCallOutcome` / `BlockedToolOutcome`

### 3.2 Transport / Host Bridge

- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
  - 当前问题：仍直接发 tool 事件并直接调用 `tool_registry.execute(...)`
  - 目标职责：降级为 tool-round helper provider，不再拥有真实 dispatch

- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
  - 目标职责：如有必要，补 wiring / facade 注入 / helper provider 接口下传

### 3.3 Registry / Tool Contract

- Modify: `src-tauri/src/plugin/registry.rs`
  - 当前问题：生产 `execute()` 与 `to_runtime_dispatcher()` 之间仍有过渡双路径
  - 目标职责：让真实聊天 tool round 通过 dispatcher path，`execute()` 不再是 send_message 主路径依赖

### 3.4 Tests

- Create: `src-tauri/tests/review_chat_tool_dispatch_runtime_test.rs`
- Modify: `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs`
- Re-run: `src-tauri/tests/workspace_first_agent_golden_path_test.rs`
- Re-run: `src-tauri/tests/tool_runtime_integration_test.rs`
- Re-run: `src-tauri/tests/builtin_runtime_registration_test.rs`

---

## 4. Acceptance Criteria

### AC-1：真实聊天 tool round 不再直接调用 `tool_registry.execute(...)`

对应 Task 2 / Task 3 / Task 4

### AC-2：真实生产工具调用进入 runtime dispatcher

对应 Task 2 / Task 4

### AC-3：`tool:executing` / `tool:completed` 由 runtime bus 成为真相源

对应 Task 2 / Task 4

### AC-4：Workspace-First 在真实 tool round 中继续成立

对应 Task 3 / Task 5

### AC-5：analysis step 的 `allowed_tools` 仍然有效

对应 Task 3 / Task 5

### AC-6：必须有直接驱动真实 adapter path 的测试证明 owner 已迁移

对应 Task 1 / Task 4 / Task 5

---

## 5. 必补 TDD（先红后绿）

以下测试不是“实现后再补”，而是本专项的 gating：

1. `send_message_production_tool_round_should_dispatch_via_runtime_query_engine`
   - 真实 adapter path 下，spy runtime tool / spy dispatcher 必须被调用
   - 当前应为红灯

2. `send_message_production_tool_events_should_be_emitted_via_runtime_bus`
   - 工具事件必须通过 runtime bus -> adapter -> host 到达，而不是 legacy `app.emit`
   - 当前应为红灯

3. `send_message_production_tool_round_should_not_call_legacy_tool_registry_execute_directly`
   - 需要直接锁死“真实 path 不再走 `tool_registry.execute(...)`”
   - 可以通过 spy registry / injected seam 验证

4. `workspace_first_tool_round_still_works_after_runtime_dispatch_migration`
   - 授权目录下的 `list_directory` / `read_workspace_file` 仍在真实 tool round 可达

5. `analysis_allowed_tools_still_gate_runtime_tool_round`
   - 分析步骤的 `allowed_tools` 仍然在迁移后生效

6. `tool_round_runtime_dispatch_preserves_tool_call_id_and_args`
   - runtime 路径必须拿到真实 `tool_call_id` 与原始 args，不能退化成只传 tool name

---

## 6. Task Plan

### Task 1：先把 T2 红灯锁死成真实 production-path 测试

**Goal:** 不再只测试局部 runtime helper，而是直接锁死真实 `send_message` adapter path 的 tool round 断层。

**Files:**
- Create: `src-tauri/tests/review_chat_tool_dispatch_runtime_test.rs`
- Modify: `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs`

**Do Not Do:**
- 不改生产代码
- 不先放宽断言
- 不只测裸 `QueryEngine`

- [ ] **Step 1: 新建 production-path 红灯测试，直驱真实 adapter path**

至少新增这些测试：

```rust
#[tokio::test]
async fn send_message_production_tool_round_should_dispatch_via_runtime_query_engine() {}

#[tokio::test]
async fn send_message_production_tool_events_should_be_emitted_via_runtime_bus() {}

#[tokio::test]
async fn send_message_production_tool_round_should_not_call_legacy_tool_registry_execute_directly() {}
```

- [ ] **Step 2: 跑 targeted tests，确认当前红灯**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test -- --nocapture
```

Expected:

- FAIL
- 失败原因直接指向真实 tool round 仍未进入 runtime dispatcher

- [ ] **Step 3: 提交红灯基线**

---

### Task 2：扩展 QueryEngine，让 runtime 能执行“真实 tool call”而不是假调用

**Goal:** 把 `QueryEngine::run_tool_with_bus(...)` 从“只传 tool name 的 demo path”升级为“真实生产 tool call dispatch API”。

**Files:**
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Optional Create: `src-tauri/src/runtime/chat/tool_round_types.rs`

**Do Not Do:**
- 不把 `PluginContext` 塞进 `CapabilityContext`
- 不直接在 `QueryEngine` 里 new 一堆 transport 依赖

- [ ] **Step 1: 定义真实 tool call 输入结构**

至少包含：

- `tool_call_id`
- `tool_name`
- `args`
- 可选：`purpose` / `origin`（如果现有 UI/审计需要）

- [ ] **Step 2: 把 QueryEngine API 扩成真实 dispatch 入口**

目标形态类似：

```rust
pub async fn run_tool_call_with_bus(
    &self,
    turn: &TurnState,
    bus: &RuntimeEventBus,
    call: RuntimeToolCallRequest,
) -> Result<RuntimeToolCallOutcome>
```

要求：

- dispatcher 使用真实 `args`
- `ToolCallExecuting` / `ToolCallCompleted` 带真实 `tool_call_id`
- outcome 返回标准化内容，供 LLM loop 写回消息

- [ ] **Step 3: 保持 capability 注入不退化**

必须继续支持：

- `workspace_path`
- `authorized_workspace`
- `browser_available`

- [ ] **Step 4: 跑局部测试转绿**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test chat_runtime_dispatcher_production_path_test -- --nocapture
```

---

### Task 3：创建 runtime-owned tool-round driver，迁移 allowed-tools 与 dispatch ownership

**Goal:** 把真实 tool round 的判断与调度 ownership 从 `chat_runtime_impl.rs` 收回 runtime。

**Files:**
- Create: `src-tauri/src/runtime/chat/tool_round_driver.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

**Do Not Do:**
- 不顺手改整个 LLM loop
- 不让 `chat_runtime_impl.rs` 继续保留真正的 dispatch 决策权

- [ ] **Step 1: 抽出 runtime tool-round driver**

它至少要负责：

- `allowed_tools` 检查
- blocked tool 的标准化结果
- 单工具 / 并行工具 dispatch
- 统一调用 `QueryEngine::run_tool_call_with_bus(...)`

- [ ] **Step 2: legacy 文件只保留 helper / context build**

从 `chat_runtime_impl.rs` 移出的 ownership 包括：

- 直接 `app.emit("tool:executing")`
- 直接 `app.emit("tool:completed")`
- 直接 `tool_registry.execute(...)`

可以暂留的 helper：

- prompt / context builder
- tool result 压缩 / 文案 helper
- 与 Tauri host 强绑定的 glue

- [ ] **Step 3: analysis step 的 `allowed_tools` 必须原样保留**

迁移后仍要保持：

- 非允许工具被阻止
- blocked browser tool 仍返回正确引导文案
- 当前 step 的 tool contract 不回归

- [ ] **Step 4: 跑 review red-light 测试转绿**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test -- --nocapture
```

---

### Task 4：把真实 send_message wiring 接到 runtime dispatcher

**Goal:** 让真实 adapter path 使用 `ToolRegistry::to_runtime_dispatcher(...)`，而不是 `tool_registry.execute(...)`。

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- Modify: `src-tauri/src/plugin/registry.rs`

**Do Not Do:**
- 不把 `execute()` 整个删掉；它仍可作为其它 legacy path 过渡接口
- 不为了图省事，在生产 path 继续绕回 `execute()`

- [ ] **Step 1: 在真实 tool round 中构造 request-scoped dispatcher**

用真实 `PluginContext` 调：

```rust
tool_registry.to_runtime_dispatcher(plugin_ctx).await
```

然后把 dispatcher 交给 runtime tool-round driver / QueryEngine。

- [ ] **Step 2: 真正切断 send_message 对 `tool_registry.execute()` 的依赖**

要求真实聊天主路径里，不再出现：

```rust
tool_registry.execute(...)
```

作为生产 tool round 执行入口。

- [ ] **Step 3: 让 tool lifecycle 事件只从 runtime bus 走**

要求真实路径中：

- `tool:executing`
- `tool:completed`

都通过：

`runtime bus -> TauriEventAdapter -> host`

而不是 `chat_runtime_impl.rs` 直接 `app.emit(...)`。

---

### Task 5：回归验证与专项验收

**Goal:** 证明迁移后的 runtime tool round 既真接线，又不打坏 Workspace-First 和现有集成行为。

**Files:**
- Modify: `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs`
- Re-run: `src-tauri/tests/workspace_first_agent_golden_path_test.rs`
- Re-run: `src-tauri/tests/tool_runtime_integration_test.rs`
- Re-run: `src-tauri/tests/builtin_runtime_registration_test.rs`

**Do Not Do:**
- 不只跑新增测试
- 不省略 workspace-first 回归

- [ ] **Step 1: T2 必须转绿**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test -- --nocapture
```

Expected:

- PASS

- [ ] **Step 2: runtime dispatcher 生产路径回归**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test chat_runtime_dispatcher_production_path_test -- --nocapture
```

- [ ] **Step 3: Workspace-First 回归**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test workspace_first_agent_golden_path_test -- --nocapture
```

- [ ] **Step 4: tool/runtime 注册链路回归**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
cargo test --manifest-path src-tauri/Cargo.toml --test tool_runtime_integration_test -- --nocapture && \
cargo test --manifest-path src-tauri/Cargo.toml --test builtin_runtime_registration_test -- --nocapture
```

- [ ] **Step 5: review_ 测试全量回归**

Run:

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast
```

---

## 7. 关闭条件

只有同时满足以下条件，本专项才算完成：

1. 真实 `send_message` tool round 不再直接 `tool_registry.execute(...)`
2. 真实 `send_message` tool round 不再直接 `app.emit("tool:executing/completed")`
3. 真实生产路径的工具执行进入 runtime dispatcher
4. `allowed_tools` 仍然有效
5. Workspace-First golden path 不回归
6. `T2` 对应的 gating test 由红转绿
7. 至少一条测试直接驱动真实 adapter path，证明不是局部假绿

---

## 8. 本专项完成后的状态说明

即使 P1-A 完成，也只代表：

- terminal events ownership 已闭合
- tool round ownership 已闭合

**不自动等于** 整个 chat-runtime-first 最终专项完全关闭。

后续若还要把：

- 外层 LLM stream loop
- provider glue
- 完整 `legacy_send_message_impl()`

继续迁回 runtime，需要再开下一阶段计划，而不是在本计划里顺手扩大范围。
