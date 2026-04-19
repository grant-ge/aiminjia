# 小修复包（Plan-AB）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — each task must have a failing test before implementation. REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — run `cargo test` and confirm output before marking any task done.

**Goal:** 接通 core_memory 注入、修复 SSE error 静默丢弃、sub_agent 从混合桥接执行收敛到 RuntimeTool + ToolDispatcher batch 主路径
**Tech Stack:** Rust, tokio
**Worktree branch:** pzc

---

## 背景与现状

| # | 问题 | 受影响文件 | 现象 |
|---|------|-----------|------|
| AB1 | `build_iteration_context()` 第一个参数 `core_memory` 在 S4 路径始终传空字符串 | `chat_turn_driver.rs:703-710` | 跨对话记忆永远不注入 LLM |
| AB2 | `process_sse_data()` 的 `"error"` event type 走 `_ => None` 静默丢弃 | `providers/claude.rs:492-495` | API 返回流式错误时调用方收不到通知 |
| AB3 | `sub_agent.rs:292` 用 `for tc in &tool_calls` 串行执行，且直接走 `ToolRegistry::execute()` 混合桥接路径，与主路径 `ToolDispatcher::dispatch_batch()` 不一致 | `sub_agent.rs:292` | 浏览器 sub-agent 工具调用无法复用 runtime batch 调度/并发语义，架构不对齐 |

---

## 测试文件

所有新测试写入 `src-tauri/tests/plan_ab_small_fixes_test.rs`，通过
```
cd src-tauri && cargo test --test plan_ab_small_fixes_test -- --nocapture
```
执行。

---

## Task AB1 — core_memory 接通 S4 路径

### 目标

在 `run_chat_turn_s4` 中，于每次 iteration 构建 `dynamic_context` 时，将 `core_memory` 作为第一个参数传给 `build_iteration_context()`。

### 现状定位

**文件：** `src-tauri/src/runtime/chat/chat_turn_driver.rs`

**当前问题代码（第 703-710 行）：**
```rust
let precompute_ctx = precompute_result.as_deref().unwrap_or_default();
let dynamic_context = if env_info.is_empty() {
    precompute_ctx.to_string()
} else if precompute_ctx.is_empty() {
    env_info.clone()
} else {
    format!("{}\n\n{}", env_info, precompute_ctx)
};
```
此处完全没有调用 `build_iteration_context()`，而是用一段手写拼接代替了它，导致：
- `core_memory` 参数永远为空
- `file_context`、`analysis_notes`、`connector_context`、`analysis_ctx_prompt` 同样没有接入（本次只修 `core_memory`）

**`build_iteration_context` 签名（`context_builder.rs:9`）：**
```rust
pub fn build_iteration_context(
    core_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    precompute_result: Option<&str>,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
) -> String
```

**`load_core_memory` 路径（`storage/file_store/mod.rs:551`）：**
```rust
pub fn load_core_memory(&self) -> String { ... }
```
该方法在 `AppStorage`（FileStore）上，返回纯字符串，读文件，同步调用。

### 方案

1. `RuntimeLlmExecutor` trait 新增默认方法 `load_core_memory()`，返回 `Result<String, TurnError>`，默认实现返回空字符串。
2. 生产实现 `TauriLegacyTurnExecutor` override 该方法，从 `self.storage.load_core_memory()` 读取。
3. 在 `run_chat_turn_s4` 开头（与 `get_env_info` 并列，在 iteration 循环外）调用 `executor.load_core_memory(conversation_id).await`，获得 `core_memory_str`。
4. 在 iteration 循环内，将手写拼接替换为 `build_iteration_context()` 调用：
   ```rust
   let dynamic_context = build_iteration_context(
       &core_memory_str,
       &env_info,        // workspace_context（现阶段复用 env_info）
       "",               // file_context（TODO: S4-T5）
       "",               // analysis_notes（TODO: S4-T5）
       precompute_result.as_deref(),
       None,             // connector_context（TODO: S4-T5）
       None,             // analysis_ctx_prompt（TODO: S4-T5）
   );
   ```

> **为什么在循环外加载**：core_memory 是跨对话的全局 KV，turn 期间不变，避免每 iteration 重复 IO。

### TDD 步骤

**Step 1 — 写失败测试**

在 `plan_ab_small_fixes_test.rs` 中：
- 构造 `MockTurnExecutor`，其 `load_core_memory()` 返回 `"test_core_memory_content"`
- 驱动一次 `run_chat_turn_s4`（用现有 mock 框架）
- 断言 `LlmStepInput::dynamic_context` 包含 `"test_core_memory_content"` 和 `"[核心记忆]"` 标签

此时测试应编译通过但失败（因 trait 方法和调用点均不存在）。

**Step 2 — 实现**

1. `chat_turn_driver.rs`：在 `RuntimeLlmExecutor` trait 中添加：
   ```rust
   async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
       Ok(String::new())
   }
   ```
2. 生产实现（`TauriLegacyTurnExecutor`）：
   ```rust
   async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
       Ok(self.storage.load_core_memory())
   }
   ```
3. `run_chat_turn_s4`：在 `get_env_info` 调用之后、iteration 循环之前添加：
   ```rust
   let core_memory_str = executor
       .load_core_memory(request.conversation_id.as_str())
       .await
       .unwrap_or_else(|e| {
           log::warn!("[run_chat_turn_s4] load_core_memory failed: {}", e);
           String::new()
       });
   ```
4. 替换 iteration 内部的手写拼接为 `build_iteration_context()` 调用（见上方方案）。
   需要在文件顶部 `use` 引入 `build_iteration_context`（已在同 crate 内）。

**Step 3 — 验证**
```bash
cd src-tauri && cargo test --test plan_ab_small_fixes_test ab1 -- --nocapture
```
测试绿。

### Commit 信息

```
feat(chat-turn): inject core_memory into S4 dynamic context via build_iteration_context - AB1
```

---

## Task AB2 — SSE error event 不再静默丢弃

### 目标

`process_sse_data()` 遇到 `"error"` event type 时，返回 `Some(vec![StreamEvent::Error { ... }])`，让调用方感知 API 错误。

### 现状定位

**文件：** `src-tauri/src/llm/providers/claude.rs`

**当前 match 末尾（第 491-496 行）：**
```rust
// ping, message_stop, etc.
_ => {
    debug!("Ignored SSE event type: {}", event_type);
    None
}
```

**`StreamEvent::Error` 变体（`streaming.rs:51`）：**
```rust
Error { error: String },
```

**Anthropic SSE error event 格式：**
```json
{ "type": "error", "error": { "type": "overloaded_error", "message": "..." } }
```

### 方案

在 `_ =>` 分支之前插入新分支：

```rust
"error" => {
    let message = parsed["error"]["message"]
        .as_str()
        .unwrap_or("unknown SSE error")
        .to_string();
    let error_type = parsed["error"]["type"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    warn!("SSE error event from provider: type={} message={}", error_type, message);
    Some(vec![StreamEvent::Error {
        error: format!("{}: {}", error_type, message),
    }])
}
```

不返回 `Err`：`process_sse_data` 的签名是 `Option<Vec<StreamEvent>>`，保持函数纯（无 I/O），将错误表示委托给 `StreamEvent::Error`。

### TDD 步骤

**Step 1 — 写失败测试**

在 `plan_ab_small_fixes_test.rs` 中：
- 调用 `process_sse_data`（需要通过模块可见性访问；可在 `claude.rs` 的 `#[cfg(test)]` 块内公开一个 `pub(crate)` 包装，或直接在同模块测试中测）

由于 `process_sse_data` 是私有 `fn`，推荐在 `claude.rs` 内部 `#[cfg(test)] mod tests` 补充测试用例（现有已有测试）：

```rust
#[test]
fn test_process_sse_data_error_event_emits_stream_error() {
    let data = r#"{"type":"error","error":{"type":"overloaded_error","message":"API overloaded"}}"#;
    let mut state = SseState::new();
    let result = process_sse_data(data, &mut state);
    assert!(result.is_some());
    let events = result.unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Error { error } => {
            assert!(error.contains("overloaded_error"));
            assert!(error.contains("API overloaded"));
        }
        other => panic!("Expected Error event, got {:?}", other),
    }
}
```

此时测试失败（返回 `None`）。

**Step 2 — 实现**

在 `process_sse_data()` 的 `match event_type` 中，`_ =>` 前插入 `"error" =>` 分支（见上方方案）。

**Step 3 — 验证**
```bash
cd src-tauri && cargo test process_sse_data -- --nocapture
```
所有 `process_sse_data` 测试绿，包括新增 error 用例。

### Commit 信息

```
fix(claude-provider): surface SSE error events as StreamEvent::Error instead of silent discard - AB2
```

---

## Task AB3 — sub_agent 迁移到 RuntimeTool + ToolDispatcher 路径

### 目标

将 `sub_agent.rs` 中通过 `ToolRegistry::execute()` 串行执行工具的循环，迁移为先基于 request-scoped `PluginContext` 构造 runtime dispatcher，再通过 `ToolDispatcher::dispatch_batch()` 执行，与主路径 `tool_round_driver.rs` 的批量调度语义一致。这是真实架构迁移，而非权宜之计。

### 现状分析

**文件：** `src-tauri/src/llm/sub_agent.rs`

**当前串行循环（第 292-463 行）：**
```rust
for tc in &tool_calls {
    if !config.allowed_tools.contains(&tc.name) { ... continue; }
    // emit tool:executing（via tauri app.emit）
    let result = tool_registry.execute(&tc.name, &sub_plugin_ctx, tc.arguments.clone(), sub_cancel).await;
    // match result: Ok / AskRequired / Err
    // push to messages / terminal_tool_results
}
```

- `tool_registry` 类型是 `&ToolRegistry`（`plugin::registry::ToolRegistry`），当前承担 schema 查询 + dispatcher 构造桥接
- event emit 走 `tauri::Emitter`（直接持有 `app_handle`）
- cancel token 来自 `config.cancel_token`
- `AskRequired` 会 `break 'agent_loop`（这个语义必须保留）

**主路径对比 — `ToolRoundDriver::execute_round`（`runtime/chat/tool_round_driver.rs`）：**
- 通过 `QueryEngine::run_tool_call_with_bus()` → `ToolDispatcher::dispatch()` 执行
- 多个 permitted 调用走 `futures::future::join_all` 并发
- event sink 在 `ToolExecutionContext` 内（`EventCollectingSink`），不直接持有 `app_handle`

**`ToolDispatcher::dispatch_batch` 签名（`dispatcher.rs:201`）：**
```rust
pub async fn dispatch_batch(
    &self,
    calls: Vec<(String, Value, ToolExecutionContext)>,
) -> Vec<Result<ToolDispatchOutcome, ToolError>>
```

**`ToolExecutionContext::new` 签名（`runtime/tools/context.rs:59`）：**
```rust
pub fn new(
    session_id: SessionId,
    run_id: RunId,
    agent_id: Option<AgentId>,
    tool_call_id: impl Into<String>,
    cancellation: CancellationToken,
) -> Self
```

**`PluginContext` 已有的字段（`plugin/context.rs`）：**
- `session_id: SessionId`
- `run_id: Option<RunId>`
- `agent_id: Option<AgentId>`

**`ToolDispatchOutcome` 变体（`dispatcher.rs:59`）：**
```rust
pub enum ToolDispatchOutcome {
    Completed { result: ToolResult, max_result_size_chars: usize, .. },
    AskRequired(PermissionDecision),
}
```

### 依赖注入方式

`sub_agent.rs` 不额外扩展 `PluginContext` 字段，也不把 `ToolDispatcher` 再往调用链上透传；改为在子 agent 内部使用：

```rust
let dispatcher = tool_registry.to_runtime_dispatcher(sub_plugin_ctx.clone()).await;
```

这样：
- `tool_registry` 仍负责 `get_all_schemas()`
- `sub_plugin_ctx` 继续提供 request-scoped 依赖（`connector_engine`、`conversation_id`、`run_id`、`agent_id`、`read_file_state`）
- 真实执行统一走 runtime dispatcher batch 主路径

### 修改文件

- **Modify:** `src-tauri/src/llm/sub_agent.rs`
- **Modify:** `src-tauri/src/llm/sub_agent.rs`
- **Modify:** `src-tauri/tests/plan_ab_small_fixes_test.rs`

### TDD 步骤

**Step 1 — 写失败测试**

在 `src-tauri/tests/plan_ab_small_fixes_test.rs` 中，构造 `ToolDispatcher` + 两个注册 `RuntimeTool`，每个 execute 耗时 50ms（`tokio::time::sleep`），然后模拟 `run_sub_agent` 内部的 `dispatch_batch` 调用路径：

```rust
#[tokio::test]
async fn ab3_dispatch_batch_runs_tools_concurrently() {
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::time::Duration;
    use lotus_app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher,
        ToolExecutionContext, ToolResult, ToolError,
    };
    use async_trait::async_trait;
    use serde_json::Value;

    struct SlowTool { name: String }

    #[async_trait]
    impl RuntimeTool for SlowTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(&self.name, "slow tool")
        }
        fn is_concurrency_safe(&self, _: &Value) -> bool { true }
        async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(ToolResult::new(self.name.clone(), "done", None))
        }
    }

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(SlowTool { name: "tool_a".into() }));
    dispatcher.register(Arc::new(SlowTool { name: "tool_b".into() }));

    let calls = vec![
        ("tool_a".to_string(), serde_json::json!({}), ToolExecutionContext::for_test("sess", "run", "tc-a")),
        ("tool_b".to_string(), serde_json::json!({}), ToolExecutionContext::for_test("sess", "run", "tc-b")),
    ];

    let start = Instant::now();
    let results = dispatcher.dispatch_batch(calls).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()), "all tools should succeed");
    // Parallel: both tools should finish well under 100ms (serial would be ~100ms)
    assert!(
        elapsed < Duration::from_millis(90),
        "dispatch_batch should run concurrently; took {:?}",
        elapsed
    );
}
```

此时测试失败（`sub_agent.rs` 尚未调用 `dispatch_batch()`，且仍直接使用 `ToolRegistry::execute()`）。

**Step 2 — 实现**

**2a. 修改 `sub_agent.rs`：**

1. 在 `sub_agent.rs` 中基于 `sub_plugin_ctx` 构造 request-scoped dispatcher：

```rust
let dispatcher = tool_registry.to_runtime_dispatcher(sub_plugin_ctx.clone()).await;
```

2. 保留 `use tauri::Emitter;` 和现有 `tool:executing` / `tool:completed` 直接 emit，避免这一步误伤 frontend watchdog 语义。

3. 将工具执行循环（第 292-463 行）替换为：

```rust
// --- Step A: 前置过滤（保持原有 allowed_tools 拦截逻辑）---
let mut denied_tool_calls = Vec::new();
let mut permitted_tool_calls = Vec::new();
for tc in &tool_calls {
    if !config.allowed_tools.contains(&tc.name) {
        denied_tool_calls.push(tc);
    } else {
        permitted_tool_calls.push(tc);
    }
}

// 立即处理被拦截的工具（无需执行）
for tc in &denied_tool_calls {
    let err_msg = format!("Tool '{}' not available in this sub-agent", tc.name);
    terminal_tool_results.push(SubAgentTerminalToolResult {
        tool_call_id: tc.id.clone(),
        tool_name: tc.name.clone(),
        success: false,
        summary: err_msg.clone(),
        generated_files: Vec::new(),
    });
    messages.push(ChatMessage::tool_result(&tc.id, &tc.name, err_msg));
}

// --- Step B: 为每个 permitted 工具构建 ToolExecutionContext ---
let batch_calls: Vec<(String, serde_json::Value, crate::runtime::tools::ToolExecutionContext)> =
    permitted_tool_calls
        .iter()
        .map(|tc| {
            let sub_cancel = match config.cancel_token.as_ref() {
                Some(parent) => parent.child_token(),
                None => crate::runtime::cancellation::CancellationToken::new(),
            };
            let ctx = crate::runtime::tools::ToolExecutionContext::new(
                sub_plugin_ctx.session_id.clone(),
                child_run_id.clone(),
                child_agent_id.clone(),
                tc.id.clone(),
                sub_cancel,
            );
            (tc.name.clone(), tc.arguments.clone(), ctx)
        })
        .collect();

// --- Step C: 并发 dispatch ---
let dispatch_results = dispatcher.dispatch_batch(batch_calls).await;

// --- Step D: 收集结果（顺序与 permitted_tool_calls 一致）---
for (tc, dispatch_result) in permitted_tool_calls.iter().zip(dispatch_results) {
    info!("[SubAgent] Tool '{}' (id={}) dispatched", tc.name, tc.id);

    // Emit heartbeat to keep frontend watchdog alive
    if let Some(ref app) = config.app_handle {
        let _ = app.emit(
            "tool:executing",
            serde_json::json!({
                "conversationId": config.conversation_id,
                "toolName": tc.name,
                "toolId": tc.id,
                "purpose": format!("[Browser Agent] {}", tc.name),
            }),
        );
    }

    match dispatch_result {
        Ok(crate::runtime::tools::ToolDispatchOutcome::Completed { result, .. }) => {
            let tool_summary = if result.content.len() > 240 {
                format!("{}...", safe_truncate(&result.content, 240))
            } else {
                result.content.clone()
            };
            terminal_tool_results.push(SubAgentTerminalToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                success: !result.file_meta.as_ref().map(|_| false).unwrap_or(false),
                summary: tool_summary,
                generated_files: result.file_meta
                    .as_ref()
                    .and_then(|fm| fm.file_path.as_ref())
                    .map(|p| vec![p.clone()])
                    .unwrap_or_default(),
            });
            if let Some(ref app) = config.app_handle {
                let summary = if result.content.len() > 100 {
                    format!("{}...", safe_truncate(&result.content, 100))
                } else {
                    result.content.clone()
                };
                let _ = app.emit(
                    "tool:completed",
                    serde_json::json!({
                        "conversationId": config.conversation_id,
                        "toolId": tc.id,
                        "success": true,
                        "summary": summary,
                    }),
                );
            }
            let content = if result.content.len() > 8000 {
                format!("{}...(truncated)", safe_truncate(&result.content, 8000))
            } else {
                result.content
            };
            messages.push(ChatMessage::tool_result(&tc.id, &tc.name, content));
        }
        Ok(crate::runtime::tools::ToolDispatchOutcome::AskRequired(decision)) => {
            let bubbled = annotate_subagent_ask_decision(&tc.name, &tc.id, decision);
            terminal_tool_results.push(SubAgentTerminalToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                success: false,
                summary: "Permission Ask required".to_string(),
                generated_files: Vec::new(),
            });
            messages.push(ChatMessage::tool_result(
                &tc.id,
                &tc.name,
                "Permission Ask required".to_string(),
            ));
            warn!(
                "[SubAgent] Tool '{}' returned AskRequired; bubbling to parent",
                tc.name
            );
            pending_ask = Some(bubbled);
            break 'agent_loop;
        }
        Err(e) => {
            let err_msg = format!("Tool error: {}", e);
            terminal_tool_results.push(SubAgentTerminalToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                success: false,
                summary: err_msg.clone(),
                generated_files: Vec::new(),
            });
            warn!("[SubAgent] Tool '{}' failed: {}", tc.name, err_msg);
            if let Some(ref app) = config.app_handle {
                let _ = app.emit(
                    "tool:completed",
                    serde_json::json!({
                        "conversationId": config.conversation_id,
                        "toolId": tc.id,
                        "success": false,
                        "summary": err_msg.clone(),
                    }),
                );
            }
            messages.push(ChatMessage::tool_result(&tc.id, &tc.name, err_msg));
        }
    }
}
```

> **AskRequired 语义说明**：原循环在遇到第一个 `AskRequired` 时立即 `break 'agent_loop`。迁移后，`dispatch_batch` 对 permitted_tool_calls 并发执行，收集结果后**顺序遍历**时遇到第一个 `AskRequired` 同样执行 `break 'agent_loop`，语义一致。并发执行期间其余工具已经开始，但 AskRequired 在结果收集阶段（顺序遍历）才处理，这与主路径 `ToolRoundDriver` 的行为对齐（主路径也不提前中断并发批次）。

> **file 路径采集**：原实现从 `tool_output.content` 逐行解析文件路径（`File: /path` 等格式）并推入 `files` Vec。迁移后 `ToolResult` 同样有 `content` 字段，`file_meta` 携带结构化路径信息。Step D 结果收集处需保留原有的文本行扫描逻辑（从 `result.content` 解析），以免文件路径丢失。实现时直接将原来 `Ok(tool_output) =>` 分支中的文件解析代码搬入 `Completed { result, .. } =>` 分支。

**2b. 修改 `internal_system.rs`（调用方）：**

`run_sub_agent` 调用处（第 383 行）需要传入 `Arc<ToolDispatcher>`。`browse_data` 工具的执行上下文是 `PluginContext`（`ctx`），目前该结构不携带 `ToolDispatcher`。

有两种方式传入，选择**最小侵入方式**：

在 `PluginContext` 中新增可选字段：
```rust
pub tool_dispatcher: Option<Arc<crate::runtime::tools::ToolDispatcher>>,
```
并在宿主层（`TauriLegacyTurnExecutor` 构建 `PluginContext` 时）注入。

调用点改为：
```rust
let dispatcher = ctx
    .tool_dispatcher
    .clone()
    .ok_or_else(|| anyhow!("ToolDispatcher not available in sub-agent context"))?;

crate::llm::sub_agent::run_sub_agent(
    gateway,
    tool_registry,
    dispatcher,
    ctx,
    config,
    app_settings,
)
.await
```

若注入宿主层难度较高（需要跨多层传递），可暂时在调用点构造一个 `ToolDispatcher::allow_all()`（仅用于过渡，含 `#[allow(deprecated)]` 注释），并在 Task 中标注 `// TODO(AB3-followup): inject real dispatcher from session runtime`。

**Step 3 — 验证**
```bash
cd src-tauri && cargo test --test plan_ab_small_fixes_test ab3 -- --nocapture
```
并发耗时测试绿（两个 50ms 工具并发完成，耗时 < 90ms）。

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
```
架构约束回归无新增失败。

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error"
```
确认编译通过。

### Commit 信息

```
refactor(sub-agent): migrate tool execution from PluginRegistry to RuntimeTool + ToolDispatcher - AB3
```

---

## 执行顺序

任务间无依赖，可独立执行。推荐顺序：AB2（最小改动、最高确定性）→ AB1 → AB3（AB3 涉及跨模块依赖注入，工作量最大）。

## 验证清单

- [ ] `cargo test --test plan_ab_small_fixes_test -- --nocapture` 全绿
- [ ] `cargo test process_sse_data -- --nocapture` 全绿（含新增 error 用例）
- [ ] `cargo test review_ --tests --no-fail-fast` 无回归
- [ ] `cargo test` 全量通过
