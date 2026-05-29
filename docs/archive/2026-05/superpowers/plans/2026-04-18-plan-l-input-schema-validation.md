# Input Schema Validation 计划（Plan-L）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 dispatcher 执行工具前统一校验 LLM 传来的参数，校验失败时返回结构化错误让 LLM 自修正，消除各工具手动 parse 的碎片化防御代码。

**Architecture:** `RuntimeTool` trait 新增 `validate_input()` 默认方法；`ToolDispatcher::dispatch()` 在权限通过后、`execute()` 前插入校验门；校验失败返回新 `ToolError::InputValidationError` 变体，dispatcher 将其编码为可重试的 tool result 内容，而非 transport 层错误。

**Tech Stack:** Rust, tokio, async_trait

**Worktree branch:** pzc

---

## 背景：claude-code-best 对标

`claude-code-best/src/services/tools/toolExecution.ts` L628-693 的流程：
1. `tool.inputSchema.safeParse(input)` — zod schema 校验，失败时返回 `tool_result` 类型的错误内容（`is_error: true`），LLM 收到后可自修正。
2. `tool.validateInput?(parsedInput.data, toolUseContext)` — 工具级语义校验，返回 `{ result: false, message }` 时同样返回可重试错误。

lotus-app 中 JSON Schema 已存储在 `TOOL_CATALOG`（`src-tauri/src/runtime/tools/catalog.rs`）但仅发送给 LLM，执行时从不验证。`BashTool::execute()` 内手动 `ok_or_else(|| ToolError::ExecutionFailed("Missing required: command"))` 是典型的碎片化防御。

---

## Task L1 — 新增 `ToolError::InputValidationError` 变体

**Files:**
- Modify: `src-tauri/src/runtime/tools/executor.rs`
- Test: `src-tauri/tests/plan_l_input_schema_validation_test.rs`（新建）

### TDD 步骤

- [ ] 1. 写失败测试

新建 `src-tauri/tests/plan_l_input_schema_validation_test.rs`：

```rust
//! Plan-L Task 1: InputValidationError variant exists and formats correctly.

#[test]
fn l1_input_validation_error_formats_tool_name_and_message() {
    use lotus_app::runtime::tools::executor::ToolError;

    let err = ToolError::InputValidationError {
        tool_name: "bash".to_string(),
        message: "Missing required field: command".to_string(),
    };

    let display = err.to_string();
    assert!(
        display.contains("bash"),
        "error display should contain tool name, got: {display}"
    );
    assert!(
        display.contains("Missing required field: command"),
        "error display should contain the message, got: {display}"
    );
}

#[test]
fn l1_input_validation_error_is_retriable_distinguishable_from_execution_failed() {
    use lotus_app::runtime::tools::executor::ToolError;

    let validation_err = ToolError::InputValidationError {
        tool_name: "write_file".to_string(),
        message: "field path is required".to_string(),
    };
    let exec_err = ToolError::ExecutionFailed("disk full".to_string());

    // Pattern matching must be possible — this is a compile-time test.
    let is_validation = matches!(validation_err, ToolError::InputValidationError { .. });
    let is_exec = matches!(exec_err, ToolError::ExecutionFailed(_));

    assert!(is_validation);
    assert!(is_exec);
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l1_ -- --nocapture 2>&1 | head -40
```

期望：`error[E0599]: no variant named 'InputValidationError'`

- [ ] 3. 最小实现

在 `src-tauri/src/runtime/tools/executor.rs` 的 `ToolError` enum 增加新变体：

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("permission ask required: {0}")]
    AskRequired(crate::runtime::tools::permission::PermissionDecision),
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
    // NEW: LLM-correctable validation error
    #[error("input validation error for tool '{tool_name}': {message}")]
    InputValidationError { tool_name: String, message: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l1_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/tools/executor.rs \
        src-tauri/tests/plan_l_input_schema_validation_test.rs && \
git commit -m "feat(tool-executor): add InputValidationError variant for LLM-correctable schema failures - L1"
```

---

## Task L2 — `RuntimeTool` trait 新增 `validate_input()` 默认方法

**Files:**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`（RuntimeTool trait 在此文件）
- Test: `src-tauri/tests/plan_l_input_schema_validation_test.rs`（追加）

### TDD 步骤

- [ ] 1. 写失败测试（追加到同一测试文件）

```rust
mod l2_validate_input_trait {
    use std::sync::Arc;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use lotus_app::runtime::tools::{RuntimeTool, ToolDispatcher};
    use lotus_app::runtime::tools::definition::ToolDefinition;
    use lotus_app::runtime::tools::executor::{ToolError, ToolResult};
    use lotus_app::runtime::tools::context::ToolExecutionContext;

    /// A tool that rejects any input missing the "command" field.
    struct StrictTool;

    #[async_trait]
    impl RuntimeTool for StrictTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("strict_tool", "test tool with validation")
        }

        fn validate_input(&self, input: &Value) -> Option<ToolError> {
            if input.get("command").is_none() {
                return Some(ToolError::InputValidationError {
                    tool_name: "strict_tool".to_string(),
                    message: "Missing required field: command".to_string(),
                });
            }
            None
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("strict_tool", "ok", None))
        }
    }

    /// A tool that has no validate_input override (default None).
    struct PermissiveTool;

    #[async_trait]
    impl RuntimeTool for PermissiveTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("permissive_tool", "test tool without validation")
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("permissive_tool", "ok", None))
        }
    }

    #[test]
    fn l2_default_validate_input_returns_none() {
        let tool = PermissiveTool;
        let input = json!({"anything": "goes"});
        assert!(
            tool.validate_input(&input).is_none(),
            "default validate_input should return None"
        );
    }

    #[test]
    fn l2_override_validate_input_returns_error_when_missing_field() {
        let tool = StrictTool;
        let bad_input = json!({"not_command": "ls"});
        let result = tool.validate_input(&bad_input);
        assert!(
            result.is_some(),
            "validate_input should return Some(ToolError) when command is missing"
        );
        assert!(
            matches!(result.unwrap(), ToolError::InputValidationError { .. }),
            "returned error should be InputValidationError variant"
        );
    }

    #[test]
    fn l2_override_validate_input_returns_none_when_valid() {
        let tool = StrictTool;
        let good_input = json!({"command": "ls -la"});
        let result = tool.validate_input(&good_input);
        assert!(result.is_none(), "validate_input should return None for valid input");
    }
}
```

- [ ] 2. 確認失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l2_ -- --nocapture 2>&1 | head -40
```

期望：编译错误 `no method named 'validate_input' found for trait 'RuntimeTool'`

- [ ] 3. 最小实现

在 `src-tauri/src/runtime/tools/dispatcher.rs` 的 `RuntimeTool` trait 中增加默认方法：

```rust
#[async_trait]
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }
    fn is_read_only(&self, _input: &Value) -> bool {
        self.definition().default_read_only
    }
    fn is_destructive(&self, _input: &Value) -> bool {
        self.definition().default_destructive
    }

    /// Validate tool input before execution.
    ///
    /// Called by `ToolDispatcher::dispatch()` after permission check, before
    /// `execute()`.  Return `Some(ToolError::InputValidationError { .. })` to
    /// short-circuit execution and let the LLM self-correct.  Default: `None`.
    fn validate_input(&self, _input: &Value) -> Option<ToolError> {
        None
    }

    async fn check_permissions(
        &self,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<crate::runtime::tools::permission::PermissionDecision> {
        None
    }
    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError>;
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l2_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/tools/dispatcher.rs \
        src-tauri/tests/plan_l_input_schema_validation_test.rs && \
git commit -m "feat(runtime-tool): add validate_input() default method to RuntimeTool trait - L2"
```

---

## Task L3 — `ToolDispatcher::dispatch()` 在执行前调用 `validate_input()`

**Files:**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`（`dispatch` 方法）
- Test: `src-tauri/tests/plan_l_input_schema_validation_test.rs`（追加）

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod l3_dispatcher_validation_gate {
    use std::sync::Arc;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use lotus_app::runtime::tools::{RuntimeTool, ToolDispatcher, ToolDispatchOutcome};
    use lotus_app::runtime::tools::definition::ToolDefinition;
    use lotus_app::runtime::tools::executor::{ToolError, ToolResult};
    use lotus_app::runtime::tools::context::ToolExecutionContext;

    struct ValidatingTool {
        executed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl RuntimeTool for ValidatingTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("validating_tool", "requires 'path' field")
        }

        fn validate_input(&self, input: &Value) -> Option<ToolError> {
            if input.get("path").is_none() {
                return Some(ToolError::InputValidationError {
                    tool_name: "validating_tool".to_string(),
                    message: "Missing required field: path".to_string(),
                });
            }
            None
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            self.executed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(ToolResult::new("validating_tool", "executed", None))
        }
    }

    #[tokio::test]
    async fn l3_dispatcher_returns_validation_error_before_execute() {
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tool = ValidatingTool { executed: executed.clone() };
        let dispatcher = ToolDispatcher::allow_all();
        dispatcher.register(Arc::new(tool));

        let ctx = ToolExecutionContext::for_test("sess-l3", "run-l3", "tc-l3");
        let bad_input = json!({"not_path": "/foo"});

        let result = dispatcher
            .dispatch("validating_tool", bad_input, ctx)
            .await;

        assert!(
            result.is_err(),
            "dispatch should return Err on validation failure"
        );
        assert!(
            matches!(result.unwrap_err(), ToolError::InputValidationError { .. }),
            "error should be InputValidationError"
        );
        assert!(
            !executed.load(std::sync::atomic::Ordering::SeqCst),
            "execute() must NOT be called when validation fails"
        );
    }

    #[tokio::test]
    async fn l3_dispatcher_executes_when_validation_passes() {
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tool = ValidatingTool { executed: executed.clone() };
        let dispatcher = ToolDispatcher::allow_all();
        dispatcher.register(Arc::new(tool));

        let ctx = ToolExecutionContext::for_test("sess-l3b", "run-l3b", "tc-l3b");
        let good_input = json!({"path": "/workspace/file.txt"});

        let result = dispatcher
            .dispatch("validating_tool", good_input, ctx)
            .await;

        assert!(result.is_ok(), "dispatch should succeed with valid input");
        assert!(
            executed.load(std::sync::atomic::Ordering::SeqCst),
            "execute() must be called for valid input"
        );
    }
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l3_ -- --nocapture 2>&1 | head -60
```

期望：`l3_dispatcher_returns_validation_error_before_execute` 失败（dispatch 返回成功因为 validation gate 还未插入），且 `executed` 为 true。

- [ ] 3. 最小实现

在 `ToolDispatcher::dispatch()` 中，在权限检查通过之后、`ctx.event_sink.emit("tool:executing")` 之前插入：

```rust
// After permission check (match permission_decision block), before execute:

// Schema validation gate: let the LLM self-correct before wasting a tool call.
if let Some(validation_err) = tool.validate_input(&input) {
    return Err(validation_err);
}

ctx.event_sink.emit("tool:executing");
let result = tool.execute(input, ctx.clone()).await;
```

完整的修改后 `dispatch` 方法关键段（示意，精确位置在 permission match 块结束后）：

```rust
match permission_decision {
    PermissionDecision::Allow { .. } => {}
    PermissionDecision::Deny { message, .. } => {
        return Err(ToolError::PermissionDenied(message));
    }
    decision @ PermissionDecision::Ask { .. } => {
        return Ok(ToolDispatchOutcome::AskRequired(decision));
    }
}

// NEW: validate input before execution
if let Some(validation_err) = tool.validate_input(&input) {
    return Err(validation_err);
}

ctx.event_sink.emit("tool:executing");
let result = tool.execute(input, ctx.clone()).await;
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l3_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/tools/dispatcher.rs \
        src-tauri/tests/plan_l_input_schema_validation_test.rs && \
git commit -m "feat(dispatcher): insert validate_input gate before execute() in dispatch() - L3"
```

---

## Task L4 — QueryEngine 把 `InputValidationError` 编码为 LLM 可重试的 tool result

**Files:**
- Modify: `src-tauri/src/runtime/query_engine.rs`（`run_tool_call_with_bus_internal` 的 Err 分支）
- Test: `src-tauri/tests/plan_l_input_schema_validation_test.rs`（追加）

### 背景

当前 `run_tool_call_with_bus_internal` 的 `Err(err)` 分支把所有错误编码为 `RuntimeToolCallOutcome::Completed { is_error: true, content: err.to_string() }`。这对 `ExecutionFailed` 是合理的，对 `InputValidationError` 同样适用——都应回传给 LLM 作为可重试 tool result。

需要确保错误消息前缀清晰，让 LLM 知道是参数问题：`InputValidationError: <message>`。

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod l4_query_engine_validation_error_encoding {
    use std::sync::Arc;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use lotus_app::runtime::tools::{RuntimeTool, ToolDispatcher};
    use lotus_app::runtime::tools::definition::ToolDefinition;
    use lotus_app::runtime::tools::executor::{ToolError, ToolResult};
    use lotus_app::runtime::tools::context::ToolExecutionContext;
    use lotus_app::runtime::query_engine::QueryEngine;
    use lotus_app::runtime::event_bus::RuntimeEventBus;
    use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use lotus_app::runtime::state::TurnState;
    use lotus_app::runtime::identity::IdentityMapping;
    use lotus_app::runtime::ids::RunId;

    struct AlwaysFailsValidation;

    #[async_trait]
    impl RuntimeTool for AlwaysFailsValidation {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("always_invalid", "always fails validation")
        }

        fn validate_input(&self, _input: &Value) -> Option<ToolError> {
            Some(ToolError::InputValidationError {
                tool_name: "always_invalid".to_string(),
                message: "required field missing".to_string(),
            })
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            unreachable!("execute must not be called when validation fails")
        }
    }

    #[tokio::test]
    async fn l4_validation_error_encoded_as_retriable_tool_result() {
        let dispatcher = Arc::new(ToolDispatcher::allow_all());
        dispatcher.register(Arc::new(AlwaysFailsValidation));

        let engine = QueryEngine::for_test(dispatcher);
        let bus = RuntimeEventBus::new();
        let mapping = IdentityMapping::from_legacy_conversation_id("sess-l4");
        let mut turn = TurnState::new(mapping, RunId::new("run-l4"), "test".to_string());

        let call = RuntimeToolCallRequest {
            tool_call_id: "tc-l4".to_string(),
            tool_name: "always_invalid".to_string(),
            args: json!({}),
        };

        let outcome = engine
            .run_tool_call_with_bus(&turn, &bus, call)
            .await
            .expect("run_tool_call_with_bus should not Err on validation failure");

        use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
        match outcome {
            RuntimeToolCallOutcome::Completed {
                is_error,
                content,
                ..
            } => {
                assert!(is_error, "validation error should be encoded as is_error=true");
                assert!(
                    content.contains("InputValidationError"),
                    "content should contain 'InputValidationError', got: {content}"
                );
                assert!(
                    content.contains("required field missing"),
                    "content should contain original message, got: {content}"
                );
            }
            other => panic!("expected Completed outcome, got {:?}", other),
        }
    }
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l4_ -- --nocapture 2>&1 | head -60
```

期望：测试编译通过，但 `content.contains("InputValidationError")` 断言失败（当前 err.to_string() 不含该前缀）。

- [ ] 3. 最小实现

在 `query_engine.rs` 的 `run_tool_call_with_bus_internal` Err 分支中，为 `InputValidationError` 生成更清晰的内容字符串：

```rust
Err(err) => {
    // Emit ToolCallCompleted with is_error=true regardless of error type.
    bus.emit(RuntimeEvent::new(
        turn.session_id().clone(),
        turn.run_id().clone(),
        RuntimeEventKind::ToolCallCompleted {
            tool_call_id: crate::runtime::ids::ToolCallId::new(
                call.tool_call_id.clone(),
            ),
            tool_name: call.tool_name.clone(),
            is_error: true,
        },
    ))
    .await?;

    // Format error content so the LLM can self-correct on validation errors.
    let content = match &err {
        crate::runtime::tools::executor::ToolError::InputValidationError {
            tool_name,
            message,
        } => format!("InputValidationError for tool '{tool_name}': {message}"),
        other => other.to_string(),
    };

    Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: call.tool_call_id,
        tool_name: call.tool_name,
        content,
        is_error: true,
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 8_000,
    })
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l4_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/query_engine.rs \
        src-tauri/tests/plan_l_input_schema_validation_test.rs && \
git commit -m "feat(query-engine): encode InputValidationError as LLM-retriable tool result content - L4"
```

---

## Task L5 — 关键工具实现 `validate_input()`

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/bash.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/grep.rs`（GrepContentTool）
- Test: `src-tauri/tests/plan_l_input_schema_validation_test.rs`（追加）

> **范围说明**：L5 只实现 `BashTool` 和 `GrepContentTool` 的 `validate_input()`。`file.rs` 中的 `LoadFileRuntimeTool` 超出当前计划范围，不在本 task 实现，可在后续专项中补充。

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod l5_builtin_tool_validation {
    use serde_json::json;
    use lotus_app::runtime::tools::RuntimeTool;
    use lotus_app::runtime::tools::builtin::bash::BashTool;
    use lotus_app::runtime::tools::builtin::grep::GrepContentTool;
    use lotus_app::runtime::tools::executor::ToolError;

    // ── BashTool ─────────────────────────────────────────────────────────────

    #[test]
    fn l5_bash_validates_missing_command_field() {
        let tool = BashTool;
        let bad = json!({"timeout_secs": 30});
        let result = tool.validate_input(&bad);
        assert!(
            result.is_some(),
            "BashTool should reject input missing 'command'"
        );
        assert!(matches!(result.unwrap(), ToolError::InputValidationError { .. }));
    }

    #[test]
    fn l5_bash_validates_command_must_be_string() {
        let tool = BashTool;
        let bad = json!({"command": 42});
        let result = tool.validate_input(&bad);
        assert!(
            result.is_some(),
            "BashTool should reject non-string 'command'"
        );
    }

    #[test]
    fn l5_bash_accepts_valid_input() {
        let tool = BashTool;
        let good = json!({"command": "ls -la"});
        assert!(
            tool.validate_input(&good).is_none(),
            "BashTool should accept valid input"
        );
    }

    #[test]
    fn l5_bash_accepts_valid_input_with_timeout() {
        let tool = BashTool;
        let good = json!({"command": "sleep 1", "timeout_secs": 5});
        assert!(
            tool.validate_input(&good).is_none(),
            "BashTool should accept valid input with optional timeout_secs"
        );
    }

    // ── GrepContentTool ──────────────────────────────────────────────────────

    #[test]
    fn l5_grep_validates_missing_pattern_field() {
        let tool = GrepContentTool;
        let bad = json!({"path": "/workspace"});
        let result = tool.validate_input(&bad);
        assert!(
            result.is_some(),
            "GrepContentTool should reject input missing 'pattern'"
        );
    }

    #[test]
    fn l5_grep_accepts_valid_input() {
        let tool = GrepContentTool;
        let good = json!({"pattern": "fn main", "path": "/workspace"});
        assert!(
            tool.validate_input(&good).is_none(),
            "GrepContentTool should accept valid input"
        );
    }
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l5_ -- --nocapture 2>&1 | head -60
```

期望：`l5_bash_validates_missing_command_field` 失败（当前 `validate_input` 默认返回 None）。

- [ ] 3. 最小实现

**bash.rs** — 在 `impl RuntimeTool for BashTool` 中增加：

```rust
fn validate_input(&self, input: &Value) -> Option<ToolError> {
    match input.get("command") {
        None => Some(ToolError::InputValidationError {
            tool_name: "bash".to_string(),
            message: "Missing required field: command (string)".to_string(),
        }),
        Some(v) if !v.is_string() => Some(ToolError::InputValidationError {
            tool_name: "bash".to_string(),
            message: format!(
                "Field 'command' must be a string, got: {}",
                v.to_string().chars().take(40).collect::<String>()
            ),
        }),
        _ => None,
    }
}
```

**grep.rs** — 在 `impl RuntimeTool for GrepContentTool` 中增加：

```rust
fn validate_input(&self, input: &Value) -> Option<ToolError> {
    match input.get("pattern") {
        None => Some(ToolError::InputValidationError {
            tool_name: "grep_content".to_string(),
            message: "Missing required field: pattern (string regex)".to_string(),
        }),
        Some(v) if !v.is_string() => Some(ToolError::InputValidationError {
            tool_name: "grep_content".to_string(),
            message: "Field 'pattern' must be a string".to_string(),
        }),
        _ => None,
    }
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test l5_ -- --nocapture
```

- [ ] 5. 全量回归检查

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test bash_tool_test -- --nocapture 2>&1 | tail -20
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test grep_content_tool_test -- --nocapture 2>&1 | tail -20
```

- [ ] 6. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/tools/builtin/bash.rs \
        src-tauri/src/runtime/tools/builtin/grep.rs \
        src-tauri/tests/plan_l_input_schema_validation_test.rs && \
git commit -m "feat(builtin-tools): implement validate_input for bash and grep_content - L5"
```

---

## 验收标准（全量）

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_l_input_schema_validation_test -- --nocapture
```

所有 `l1_` `l2_` `l3_` `l4_` `l5_` 前缀测试全部 PASS，无 WARN 未使用变量。

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast
```

架构约束回归测试全部 PASS。

---

## 实现注意事项

1. **不修改 execute() 内部的手动 parse 防御**：这是 L5 之后的增量工作，目前保留以确保 execute() 的 early-return 路径不被 validation gate 引入回归。
2. **validate_input 是同步方法**：不需要 async，避免引入不必要的 trait object 复杂度。
3. **错误格式对齐 claude-code-best**：内容前缀 `InputValidationError for tool '...': ...` 让 LLM 明确知道是参数问题，与 `toolExecution.ts` L683 的 `<tool_use_error>InputValidationError: ...` 语义一致。
4. **不破坏 MCP 工具**：`McpRuntimeTool` 没有 schema 知识，`validate_input` 默认 None 对其透明，无需修改。

<!-- reviewed: 2026-04-18, fixes applied -->
