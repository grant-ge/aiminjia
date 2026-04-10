# Golden Traces

> 基于 `src-tauri/tests/golden_trace_capture.rs` 和 `src-tauri/src/runtime_audit/trace_capture.rs` 实际代码提取。
> 测试通过 `RecordingRuntimeHost` 捕获所有 emit 到 `TauriEventAdapter` 的事件名序列。

---

## 数据来源说明

### 测试入口
**文件**：`src-tauri/tests/golden_trace_capture.rs`

两个测试函数：
1. `captures_real_legacy_trace_for_basic_chat_flow`（L4–L12）
2. `captures_real_legacy_trace_for_single_tool_flow`（L14–L23）

### 测试基础设施
**文件**：`src-tauri/src/runtime_audit/trace_capture.rs`

- `CapturedEvent { name: String, payload: serde_json::Value }` — 单条事件记录
- `CapturedTrace { events: Vec<CapturedEvent> }` — 一次 turn 的完整事件序列
- `LegacyTraceScenario` enum — 枚举测试场景（`BasicChat`, `SingleTool`）
- `capture_legacy_trace(scenario)` — 委托给 `commands::chat::testsupport` 执行

### 测试执行路径（testsupport，`commands/chat.rs:L96–L128`）

**BasicChat**：
```
RecordingRuntimeHost::new()
→ SessionRuntime::for_test(host.clone())    // 挂载 TauriEventAdapter
→ runtime.run_for_test("conv-trace", "run-test-basic", "hello")
    → SessionRuntime::run_turn(&mut turn)
        → QueryEngine::run(turn, bus)       // query_engine.rs:L31–L51
            → bus.emit(StreamDelta { content: "runtime:hello" })
            → bus.emit(MessagePersisted { message_id: "msg-run-test-basic" })
            → bus.emit(StreamDone)
→ host.trace()  →  CapturedTrace
```

**SingleTool**：
```
RecordingRuntimeHost::new()
→ RuntimeEventBus::new()
→ bus.subscribe(TauriEventAdapter::new(host.clone()))
→ single_legacy_tool_dispatcher("python_exec")
→ QueryEngine::for_test(dispatcher)
→ TurnState::new(IdentityMapping("conv-trace"), RunId("run-test-tool"), "run python_exec")
→ query_engine.run_tool_with_bus(&turn, &bus, "python_exec")  // query_engine.rs:L76–L133
    → dispatcher.dispatch("python_exec", {tool:"python_exec"}, ctx)
        → ToolDispatcher::dispatch → event_sink.emit("tool:executing")
                                    → event_sink.emit("tool:completed")
    → outcome.event_names 中的 "tool:executing" → bus.emit(ToolCallExecuting)
    → outcome.event_names 中的 "tool:completed" → bus.emit(ToolCallCompleted)
    → bus.emit(StreamDone)
→ host.trace()  →  CapturedTrace
```

---

## Golden Trace 序列（断言级别）

### Scenario 1: BasicChat

**场景名**：`LegacyTraceScenario::BasicChat`
**输入**：`conversation_id = "conv-trace"`, `user_input = "hello"`

**断言事件序列**（`golden_trace_capture.rs:L9–L11`）：
```
["streaming:delta", "message:updated", "streaming:done"]
```

**事件详情**（由 `TauriEventAdapter::map_runtime_event` 将 RuntimeEvent 映射）：

| # | Event Name | 来源 RuntimeEventKind | Key Payload Fields |
|---|---|---|---|
| 1 | `streaming:delta` | `StreamDelta { content: "runtime:hello" }` | `conversationId: "conv-trace"`, `delta: "runtime:hello"`, `runId: "run-test-basic"` |
| 2 | `message:updated` | `MessagePersisted { message_id: "msg-run-test-basic" }` | `conversationId: "conv-trace"`, `messageId: "msg-run-test-basic"`, `runId: "run-test-basic"` |
| 3 | `streaming:done` | `StreamDone` | `conversationId: "conv-trace"`, `runId: "run-test-basic"` |

**关键状态变化**：
- `TurnState::output` 从空 → 追加 `"runtime:hello"`（`query_engine.rs:L33`）
- RuntimeEventBus 广播 3 条事件 → TauriEventAdapter 映射 → RecordingRuntimeHost 记录

---

### Scenario 2: SingleTool

**场景名**：`LegacyTraceScenario::SingleTool`
**输入**：`conversation_id = "conv-trace"`, `tool_name = "python_exec"`

**断言事件序列**（`golden_trace_capture.rs:L20–L22`）：
```
["tool:executing", "tool:completed", "streaming:done"]
```

**事件详情**：

| # | Event Name | 来源 RuntimeEventKind | Key Payload Fields |
|---|---|---|---|
| 1 | `tool:executing` | `ToolCallExecuting { tool_call_id: "tool-call-python_exec", tool_name: "python_exec" }` | `conversationId: "conv-trace"`, `toolId: "tool-call-python_exec"`, `toolName: "python_exec"`, `runId: "run-test-tool"` |
| 2 | `tool:completed` | `ToolCallCompleted { tool_call_id: "tool-call-python_exec", tool_name: "python_exec" }` | `conversationId: "conv-trace"`, `toolId: "tool-call-python_exec"`, `toolName: "python_exec"`, `success: true`, `runId: "run-test-tool"` |
| 3 | `streaming:done` | `StreamDone` | `conversationId: "conv-trace"`, `runId: "run-test-tool"` |

**关键状态变化**：
- `ToolDispatcher::dispatch("python_exec")` 执行 → `event_sink.emit("tool:executing")`（`dispatcher.rs:L69`） → `event_sink.emit("tool:completed")`（`dispatcher.rs:L71`）
- `outcome.event_names = ["tool:executing", "tool:completed"]` 被 `run_tool_with_bus` 转为 `RuntimeEventBus` 事件
- `StreamDone` 由 `run_tool_with_bus` 末尾固定 emit（`query_engine.rs:L127`）

---

## 测试局限性说明

1. **仅覆盖新 runtime 路径**：上述 golden trace 测试走的是 `SessionRuntime → QueryEngine → RuntimeEventBus → TauriEventAdapter` 路径，**不**覆盖 legacy `legacy_send_message_impl` / `agent_loop` 路径（那条路径直接调用 `app.emit`，无法被 `RecordingRuntimeHost` 捕获）。

2. **BasicChat content 固定**：`QueryEngine::run` 生成的 delta 内容固定为 `"runtime:{user_input}"`，不经过真实 LLM。

3. **SingleTool 工具实现**：`single_legacy_tool_dispatcher("python_exec")` 使用 `tools/testing.rs` 中的 stub dispatcher，仅记录事件名，不执行真实 Python。

4. **缺少 legacy agent_loop trace**：目前没有覆盖 `streaming:error`、`agent:idle`、`tool:executing`（legacy path）、`message:updated`（完整 payload）的 golden trace。Phase 1 重构应补充这些场景。
