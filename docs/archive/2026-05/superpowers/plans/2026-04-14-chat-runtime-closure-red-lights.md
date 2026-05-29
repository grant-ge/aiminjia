# Chat Runtime Closure: Turn 3 Red Lights GREEN

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 T1/T2/T4 三盏红灯全部转绿，完成 chat-runtime-first Phase-1 闭合。

**Architecture:** 当前 executor-backed 路径中，`RuntimeChatTurnDriver` 在 executor 返回后用 `record_only` 写 `MessagePersisted` / `StreamDone`，导致 `TauriEventAdapter` 永远收不到这两个事件（T1/T4 红灯）。T2 红灯是因为 legacy executor 的工具调用完全绕过 `ToolDispatcher`。修复分两步：P1-B 把 `record_only` 改为真正的 `bus.emit`（同时移除 executor 侧的重复发射，防双发）；P1-A 把 executor 内工具调用路由进 `ToolDispatcher::dispatch()`（最小侵入：在 executor 执行后，由 driver 侧发 bus 事件即可让 SpyTool 被调用）。

**Tech Stack:** Rust, Tokio, `RuntimeEventBus`, `TauriEventAdapter`, `ToolDispatcher`, `RuntimeTurnExecutor` trait

---

## 文件地图

| 文件 | 变更类型 | 职责 |
|------|----------|------|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | **Modify** | 把两处 `record_only` 改成 `bus.emit`（P1-B）；增加 `ToolDispatcher` 路由钩子（P1-A） |
| `src-tauri/src/transport/tauri_commands/chat/chat_support.rs` | **Modify** | `AgentGuard::clear()` / `Drop` 中移除直接 `app.emit("streaming:done")` 和 `app.emit("agent:idle")`，改为通过 bus（防双发） |
| `src-tauri/src/runtime/chat/mod.rs` 或 driver 所在 mod | **Check** | 确认导出不变 |
| `src-tauri/tests/send_message_production_path_test.rs` | **Read-only** | 4 个 gate 测试，目标全绿 |

> **注意：** T2 的修复策略是「最小侵入」——我们不需要把整个 LLM loop 移进 runtime。测试的断言只要求 `SpyTool` 被调用过。满足方式：在 `RuntimeChatTurnDriver` executor-backed 路径里，executor 执行完后，由 driver 从 `request` 里提取工具调用信息并走 `ToolDispatcher::dispatch()`。但这要求 executor 把工具调用结果回传给 driver。**这条路太重**。

重新看 T2 测试断言：它用 `CapturingExecutor`（不真正执行工具），并断言 `SpyTool` 被调用。意思是：**只要 send_message 生产路径里，经过 `ToolDispatcher` 派发了任意一次工具调用**，测试就绿。目前 `CapturingExecutor` 是空实现，不发 LLM 也不调工具，所以 spy 永远没机会被调用。

**T2 的真正含义**：生产路径上必须有一个工具调用走 `ToolDispatcher`。由于 `CapturingExecutor` 不调任何工具，唯一能让 spy 被调到的方式，是 `RuntimeChatTurnDriver` 在调完 executor 之后，**主动用 `QueryEngine::run_tool_with_bus` 发起一次 no-op 工具探测**。但这没有语义意义。

**重读测试注释**（第 142-150 行）：

> "Architecture note: we cannot inject a mock LLM response here without significant infrastructure, so we observe the bypass indirectly: after a full chat turn completes, the spy tool registered in the runtime dispatcher should have been called (target state), but currently it never is (current gap)."
> "When P1-A routes tool execution through `ToolDispatcher::dispatch()` from within the runtime-owned LLM loop the spy will be reached and the test turns GREEN."

结论：T2 真正要求的是 **LLM loop 必须在 runtime 侧**，工具调用从 LLM 响应里提取后走 dispatcher。这就是把 agent_loop 收进 runtime 的大迁移（P1-A）。这个量不小。

**本次 plan 的范围决策：**
- **Task 1（P1-B）**：修 T1 + T4（`record_only` → `emit`，移除 executor 侧双发）
- **Task 2（P1-A 最小前置）**：在 `QueryEngine::run()` 的纯 runtime 路径里实现一个真实的 mock LLM stub，让测试能控制 LLM 响应，从而让 `CapturingExecutor` 替换为一个会发工具调用的 executor，driver 侧用 dispatcher 处理。**但这要改测试本身**，而测试是 gate——不能改断言。
- **结论**：T2 需要把 LLM loop 真正收进 runtime，或在 `RuntimeChatTurnDriver` 的 executor-backed 路径里注入一个 LLM stub。这是独立专项（P1-A），本次 plan 先交付 T1+T4，T2 单独立项。

---

## Task 1：P1-B — 修复 T1 + T4（`record_only` → `emit`）

**目标**：`StreamDone` 和 `MessagePersisted` 在 executor-backed 路径真正通过 bus 通知到 `TauriEventAdapter`。

**关键约束**：executor (`AgentGuard::clear()`) 目前已经直接 `app.emit("streaming:done")` 和 `app.emit("agent:idle")`，如果 runtime bus 也 emit 同样事件，前端会收到两次。必须同时把 executor 侧的直接 emit 移除或改为 no-op。

**文件：**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:136-146`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_support.rs:547-574`（`AgentGuard::clear()`）
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_support.rs:592-600`（`AgentGuard::Drop`）

---

- [ ] **Step 1.1：确认当前测试状态（T1/T4 红灯）**

```bash
cd src-tauri && cargo test send_message_production_path -- --nocapture 2>&1 | grep -E "FAILED|PASSED|test result"
```

期望：T1 (`full_turn_must_not_delegate`) 和 T4 (`message_persisted_must_be_emitted`) 都 FAILED，T3 (`must_use_single_run_id`) PASSED。

---

- [ ] **Step 1.2：在 `chat_turn_driver.rs` 把两处 `record_only` 改为 `async emit`**

当前代码（第 136-146 行）：

```rust
self.event_bus.record_only(RuntimeEvent::message_persisted(
    session_id.clone(),
    run_id.clone(),
    format!("exec-msg-{}", run_id.as_str()),
    "assistant",
    serde_json::json!({"executor_owned": true}),
));
self.event_bus.record_only(RuntimeEvent::stream_done(
    session_id,
    run_id,
));
```

改为：

```rust
self.event_bus
    .emit(RuntimeEvent::message_persisted(
        session_id.clone(),
        run_id.clone(),
        format!("exec-msg-{}", run_id.as_str()),
        "assistant",
        serde_json::json!({"executor_owned": true}),
    ))
    .await?;
self.event_bus
    .emit(RuntimeEvent::stream_done(session_id, run_id))
    .await?;
```

---

- [ ] **Step 1.3：在 `AgentGuard::clear()` 移除直接 `streaming:done` 和 `agent:idle` 发射**

定位 `chat_support.rs` 第 547-574 行，`clear()` 方法。

现在：
```rust
if let Err(e) = self.app.emit("streaming:done", serde_json::json!({...})) { ... }
if let Err(e) = self.app.emit("agent:idle", serde_json::json!({...})) { ... }
```

改为：注释掉这两段 `app.emit`（或删除），改为日志说明事件现在由 runtime bus 负责。保留 `gateway.clear_task`、`session_mgr.destroy_run`、`remove_lock_with_retry` 这三个清理操作不变。

```rust
pub(crate) async fn clear(&mut self) {
    if !self.cleared {
        self.cleared = true;
        self.gateway.clear_task(&self.conversation_id);
        self.session_mgr.destroy_run(&self.run_id).await;
        self.remove_lock_with_retry();
        // streaming:done and agent:idle are now emitted by RuntimeChatTurnDriver
        // via the runtime bus -> TauriEventAdapter.  Do not emit here to avoid
        // duplicate frontend events.
        log::info!(
            "[AgentGuard] Cleared active task for conversation {} (events delegated to runtime bus)",
            self.conversation_id
        );
    }
}
```

---

- [ ] **Step 1.4：在 `AgentGuard::Drop` impl 同样移除直接 `streaming:done` 发射**

定位 `chat_support.rs` 第 579-600 行，`Drop` impl。

现在：
```rust
let _ = self.app.emit("streaming:done", serde_json::json!({...}));
```

改为：同样删除这行，改为日志。但 `Drop` 是 panic 安全网，需要保留 `gateway.clear_task`。

```rust
impl Drop for AgentGuard {
    fn drop(&mut self) {
        if !self.cleared {
            self.remove_lock_with_retry();
            self.gateway.clear_task(&self.conversation_id);
            let session_mgr = self.session_mgr.clone();
            let run_id = self.run_id.clone();
            tauri::async_runtime::spawn(async move {
                session_mgr.destroy_run(&run_id).await;
            });
            // streaming:done is emitted by RuntimeChatTurnDriver via the runtime bus.
            // Drop is a panic-safety net for resource cleanup only.
            log::warn!(
                "[AgentGuard::drop] Forcibly cleared active task for {} without emitting events.",
                self.conversation_id
            );
        }
    }
}
```

> **注意**：`agent:idle` 目前由 `TauriEventAdapter` 从 `RuntimeEventKind::AgentIdle` 映射。`RuntimeChatTurnDriver` 目前没有发 `AgentIdle`。需要在 `chat_turn_driver.rs` 的 executor-backed 路径里加一条 `AgentIdle` emit。

---

- [ ] **Step 1.5：在 `chat_turn_driver.rs` 补上 `AgentIdle` emit**

在 `stream_done` emit 之后加：

```rust
self.event_bus
    .emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::AgentIdle {
            agent_id: crate::runtime::ids::AgentId::new(format!("agent-{}", run_id.as_str())),
            scope: crate::runtime::events::AgentIdleScope::Primary,
        },
    ))
    .await?;
```

需要在文件顶部确认 `AgentId` 和 `AgentIdleScope` 的 import 路径。

---

- [ ] **Step 1.6：编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error"
```

期望：无 error。

---

- [ ] **Step 1.7：运行 T1/T4 测试**

```bash
cd src-tauri && cargo test send_message_production_path -- --nocapture 2>&1
```

期望：T1 PASSED，T4 PASSED，T3 仍然 PASSED，T2 仍然 FAILED（预期，等 P1-A）。

---

- [ ] **Step 1.8：运行全量 review_ 回归测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

期望：所有之前绿的 review_ 测试仍然绿，无新增失败。

---

- [ ] **Step 1.9：Commit**

```bash
cd src-tauri && git add src/runtime/chat/chat_turn_driver.rs src/transport/tauri_commands/chat/chat_support.rs
git commit -m "fix(chat-runtime): emit StreamDone+MessagePersisted through bus, remove AgentGuard direct emit

P1-B: convert record_only to bus.emit for StreamDone and MessagePersisted
in the executor-backed path so TauriEventAdapter delivers streaming:done
and message:updated to the frontend. Remove duplicate direct app.emit
calls from AgentGuard::clear() and Drop to prevent double events.

Fixes T1 (streaming:done via host) and T4 (message:updated via host).
T3 regression gate unaffected. T2 remains RED pending P1-A."
```

---

## Task 2：立项 T2（P1-A）— 工具调用收归 runtime dispatcher

T2 要求生产 send_message 路径中工具调用必须经过 `ToolDispatcher::dispatch()`。这意味着 LLM loop（流读取 + 工具调用提取 + 工具执行）必须从 `chat_runtime_impl.rs` 的 `agent_loop` 移进 `RuntimeChatTurnDriver` / `QueryEngine`。

这是独立的大迁移，与本次 T1/T4 修复完全解耦。

- [ ] **Step 2.1：创建 T2 专项问题陈述文档**

```bash
cat > docs/2026-04-14-p1a-tool-dispatch-problem-statement.md << 'EOF'
# P1-A: Tool Dispatch via Runtime ToolDispatcher — 问题陈述

## 红灯
T2: `send_message_production_tool_round_must_dispatch_via_runtime_query_engine`

## 根因
`agent_loop` 在 `chat_runtime_impl.rs` 里直接调 `tool_registry.execute(&name, &plugin_ctx, args)`，
完全绕过 `ToolDispatcher`。

## 目标状态
`RuntimeChatTurnDriver` 在 executor-backed 路径里，执行器执行完毕后，从工具调用结果中
提取工具调用并通过 `QueryEngine::run_tool_with_bus` 派发。
或者：将整个 LLM stream loop 收进 runtime，executor 降级为纯 LLM gateway wrapper。

## 估算
高风险迁移。需要独立 plan + worktree。
EOF
```

- [ ] **Step 2.2：记录当前状态到 memory journal**（在 plan 执行完 Task 1 后）

```bash
echo "T1+T4 closed on $(date +%Y-%m-%d). T2 (P1-A tool dispatch) pending separate plan." >> docs/memory-journal-2026-04-14.md
```

---

## 自检

**Spec 覆盖：**
- T1 (`streaming:done` via host)：Task 1 Step 1.2 + 1.3 ✓
- T4 (`message:updated` via host)：Task 1 Step 1.2 ✓
- T3 回归保护：Step 1.7 + 1.8 ✓
- T2 (tool dispatch)：明确标记为 P1-A 独立专项，不在本次 plan 范围 ✓

**Placeholder 扫描：**无 TBD/TODO/implement later。

**类型一致性：**
- `RuntimeEvent::message_persisted` / `stream_done` — 已在 `chat_turn_driver.rs` 中使用，签名不变
- `RuntimeEventKind::AgentIdle` — 已在 `tauri_event_adapter.rs` 中有 mapping，签名需确认 `AgentId` import

**潜在风险：**
- `AgentGuard::Drop` 是 panic 安全网。移除 `streaming:done` 后，如果 driver 在 executor 执行期间 panic，前端将不收到 `streaming:done`。后续需要在 `SessionRuntime` 层加 panic catch 或用 `scopeguard`。本次 plan 不处理，在 commit message 中注明。
