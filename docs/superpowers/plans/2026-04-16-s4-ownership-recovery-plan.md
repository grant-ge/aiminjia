# S4 编排 Ownership 完整回收 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 `agent_loop()`，将全部编排逻辑收归 `RuntimeChatTurnDriver`，executor 退化为 provider streaming adapter，对齐 claude-code-best 架构。

**Architecture:** Driver 拥有 query loop 和所有状态变更，executor 只做 `gateway.stream_message()` + 流解析（只读输入、结构化返回）。所有事件走 RuntimeEventBus，不直接 `app.emit()`。Config（不可变）和 State（可变）严格分离。

**Tech Stack:** Rust, Tauri v2, tokio, async_trait, serde_json

**Spec:** `docs/superpowers/specs/2026-04-16-s4-ownership-recovery-design.md`

---

## 文件结构

### 新建文件

| 文件 | 职责 |
|------|------|
| `src-tauri/src/runtime/chat/turn_config.rs` | TurnConfig, TurnIterationState, LlmStepInput, LlmStepResult, TurnError, build_turn_config() |
| `src-tauri/src/runtime/chat/context_builder.rs` | build_iteration_context() — 每次迭代的动态上下文拼接 |
| `src-tauri/src/runtime/chat/compaction.rs` | filter_daily_messages(), compress_if_needed(), apply_decay() |
| `src-tauri/src/runtime/chat/safeguard.rs` | SafeguardAction, check_iteration() |
| `src-tauri/src/runtime/chat/post_process.rs` | finalize_content() |
| `src-tauri/src/runtime/chat/tool_result_collector.rs` | ToolRoundResults, collect_results(), update_analysis_context() |
| `src-tauri/src/runtime/chat/metrics.rs` | log_context_baseline(), log_iteration_metrics(), log_step_done(), record_step_lifecycle(), record_tool_round() |
| `src-tauri/tests/s4_driver_loop_test.rs` | driver 多轮迭代、cancel、safeguard、事件顺序测试 |

### 修改文件

| 文件 | 改什么 |
|------|--------|
| `src-tauri/src/runtime/chat/mod.rs` | 新增 7 个子模块导出 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 重写 RuntimeTurnExecutor trait + RuntimeChatTurnDriver::run_chat_turn |
| `src-tauri/src/runtime/events.rs` | 新增 StreamError variant |
| `src-tauri/src/transport/tauri_event_adapter.rs` | 新增 StreamError → "streaming:error" 映射 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | 重写 TauriLegacyTurnExecutor impl |
| `src-tauri/src/runtime/query_engine.rs` | 删除 run() 中 stub echo 路径 |

### 删除内容

| 文件 | 删什么 |
|------|--------|
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | agent_loop() (L1649-L3373), finish_agent(), compress_context_if_needed() |

---

## Task 1: 新增核心类型 — turn_config.rs

**Files:**
- Create: `src-tauri/src/runtime/chat/turn_config.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 创建 turn_config.rs 并定义 TurnConfig**

```rust
// src-tauri/src/runtime/chat/turn_config.rs

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::Value as JsonValue;

use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;

/// Turn 级不可变配置。在 run_chat_turn 入口处构建一次，之后只读。
#[derive(Debug, Clone)]
pub struct TurnConfig {
    pub system_prompt: String,
    pub tool_defs: Vec<JsonValue>,
    pub allowed_tools: Option<HashSet<String>>,
    pub max_iterations: usize,
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub is_analysis: bool,
    pub masking_level: String,
    pub workspace_path: PathBuf,
    pub conversation_id: String,
    pub run_id: String,
}

/// Turn 级可变状态。Driver 是唯一修改者。
#[derive(Debug)]
pub struct TurnIterationState {
    pub messages: Vec<JsonValue>,
    pub full_content: String,
    pub generated_file_ids: Vec<String>,
    pub all_file_metas: Vec<JsonValue>,
    pub iteration_count: usize,
    pub stream_cancelled: bool,
    pub step_tokens_in: u64,
    pub step_tokens_out: u64,
    pub force_no_tools: bool,
    pub safeguard_phase1_injected: bool,
}

impl TurnIterationState {
    pub fn new(messages: Vec<JsonValue>) -> Self {
        Self {
            messages,
            full_content: String::new(),
            generated_file_ids: Vec::new(),
            all_file_metas: Vec::new(),
            iteration_count: 0,
            stream_cancelled: false,
            step_tokens_in: 0,
            step_tokens_out: 0,
            force_no_tools: false,
            safeguard_phase1_injected: false,
        }
    }
}

/// Executor 的只读输入。由 driver 从 TurnConfig + TurnIterationState 构建。
#[derive(Debug)]
pub struct LlmStepInput<'a> {
    pub system_prompt: &'a str,
    pub dynamic_context: &'a str,
    pub messages: Vec<JsonValue>,  // decayed 副本，非原始 messages 引用
    pub tool_defs: &'a [JsonValue],
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub masking_level: &'a str,
    pub force_no_tools: bool,
    pub conversation_id: &'a str,
    pub run_id: &'a str,
}

/// Executor 的结构化返回。Executor 只产出数据，不修改外部状态。
#[derive(Debug)]
pub enum LlmStepResult {
    /// LLM 返回了工具调用
    ToolCalls {
        assistant_content: String,
        tool_calls: Vec<RuntimeToolCallRequest>,
        tokens_in: u64,
        tokens_out: u64,
    },
    /// LLM 返回纯文本，无工具调用
    ContentComplete {
        content: String,
        tokens_in: u64,
        tokens_out: u64,
    },
    /// 用户取消
    Cancelled,
}

/// 结构化错误
#[derive(Debug, thiserror::Error)]
pub enum TurnError {
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Max retries exceeded")]
    MaxRetriesExceeded,
    #[error("Persistence error: {0}")]
    PersistenceError(String),
}
```

- [ ] **Step 2: 更新 mod.rs 导出 turn_config**

在 `src-tauri/src/runtime/chat/mod.rs` 顶部添加：

```rust
pub mod turn_config;
```

在 pub use 区域添加：

```rust
pub use turn_config::{LlmStepInput, LlmStepResult, TurnConfig, TurnError, TurnIterationState};
```

- [ ] **Step 3: 写一个基础测试验证类型可用**

```rust
// src-tauri/tests/s4_driver_loop_test.rs

use lotus_app::runtime::chat::turn_config::*;

#[test]
fn turn_iteration_state_initializes_cleanly() {
    let state = TurnIterationState::new(vec![]);
    assert_eq!(state.iteration_count, 0);
    assert!(!state.stream_cancelled);
    assert!(state.full_content.is_empty());
    assert!(!state.force_no_tools);
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test turn_iteration_state_initializes_cleanly -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/chat/turn_config.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T1): add TurnConfig, TurnIterationState, LlmStepInput, LlmStepResult, TurnError types"
```

---

## Task 2: 新增 RuntimeEventKind::StreamError + TauriEventAdapter 映射

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写失败测试 — StreamError 事件能被映射**

```rust
// 追加到 src-tauri/tests/s4_driver_loop_test.rs

use lotus_app::runtime::events::{RuntimeEvent, RuntimeEventKind};
use lotus_app::transport::tauri_event_adapter::map_runtime_event;

#[test]
fn stream_error_maps_to_legacy_event() {
    let event = RuntimeEvent::new(
        "test-session".into(),
        "test-run".into(),
        RuntimeEventKind::StreamError {
            error: "Connection timeout".to_string(),
            raw_error: Some("reqwest::Error".to_string()),
        },
    );
    let legacy = map_runtime_event(&event);
    assert!(legacy.is_some());
    let legacy = legacy.unwrap();
    assert_eq!(legacy.name, "streaming:error");
    assert_eq!(legacy.payload["error"], "Connection timeout");
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test stream_error_maps_to_legacy_event -- --nocapture`
Expected: FAIL — `StreamError` variant 不存在

- [ ] **Step 3: 在 events.rs 新增 StreamError variant**

在 `src-tauri/src/runtime/events.rs` 的 `RuntimeEventKind` enum 中，`StreamDone` 之后添加：

```rust
    StreamError {
        error: String,
        raw_error: Option<String>,
    },
```

- [ ] **Step 4: 在 tauri_event_adapter.rs 新增映射**

在 `map_runtime_event` 函数的 match 分支中，`StreamDone` 分支之后添加：

```rust
            RuntimeEventKind::StreamError { ref error, ref raw_error } => {
                let conv_id = event.session_id.to_string();
                Some(LegacyEvent {
                    name: "streaming:error".to_string(),
                    payload: serde_json::json!({
                        "conversationId": conv_id,
                        "error": error,
                        "rawError": raw_error,
                        "runId": event.run_id.to_string(),
                    }),
                })
            }
```

- [ ] **Step 5: 确保 map_runtime_event 是 pub 的**

检查 `tauri_event_adapter.rs` 中 `map_runtime_event` 是否为 `pub fn`。如果不是，改为 `pub fn`。

- [ ] **Step 6: 运行测试验证通过**

Run: `cd src-tauri && cargo test stream_error_maps_to_legacy_event -- --nocapture`
Expected: PASS

- [ ] **Step 7: 运行全部 review_ 测试确认无回归**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: 全部 PASS

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/runtime/events.rs src-tauri/src/transport/tauri_event_adapter.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T2): add RuntimeEventKind::StreamError + TauriEventAdapter mapping"
```

---

## Task 3: 重新定义 RuntimeTurnExecutor trait

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

这是最关键的接口变更。为了避免编译断裂，采用"先加新 trait → 临时共存 → 后删旧 trait"策略。

- [ ] **Step 1: 在 chat_turn_driver.rs 中定义新 trait（不删旧的）**

在 `src-tauri/src/runtime/chat/chat_turn_driver.rs` 中，在现有 `RuntimeTurnExecutor` trait 之后添加：

```rust
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, TurnConfig, TurnError, TurnIterationState,
};

/// S4 新 trait：executor 只做 provider streaming adapter。
/// Driver 拥有 query loop 和状态变更，executor 不修改外部状态。
#[async_trait]
pub trait RuntimeLlmExecutor: Send + Sync {
    /// 单步 LLM 调用。接收只读输入，返回结构化结果。
    /// 内部调用 gateway.stream_message()，通过 bus emit StreamDelta/StreamError。
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        bus: &RuntimeEventBus,
        cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError>;

    /// Precompute 执行（analysis 模式专用）。默认 no-op。
    async fn run_precompute(
        &self,
        _config: &TurnConfig,
        _state: &mut TurnIterationState,
    ) -> Result<Option<String>, TurnError> {
        Ok(None)
    }

    /// 持久化 assistant message 到存储。纯 I/O，不含事件发射。
    async fn persist_assistant_message(
        &self,
        conversation_id: &str,
        content: &str,
        generated_file_ids: &[String],
        file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError>;

    /// Step 后处理（analysis 模式专用）。默认 no-op。
    async fn finalize_step(
        &self,
        _state: &TurnIterationState,
        _config: &TurnConfig,
    ) -> Result<(), TurnError> {
        Ok(())
    }
}
```

- [ ] **Step 2: 更新 mod.rs 导出新 trait**

在 `src-tauri/src/runtime/chat/mod.rs` 的 `pub use chat_turn_driver` 行中添加 `RuntimeLlmExecutor`：

```rust
pub use chat_turn_driver::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor, RuntimeTurnExecutor};
```

- [ ] **Step 3: 更新 RuntimeChatTurnDriver 持有新 executor**

在 `RuntimeChatTurnDriver` struct 中添加新字段（保留旧的 `legacy_executor`）：

```rust
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    legacy_executor: Option<Arc<dyn RuntimeTurnExecutor>>,
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,  // S4 新增
}
```

添加新构造函数：

```rust
    pub fn with_llm_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        executor: Arc<dyn RuntimeLlmExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: None,
            llm_executor: Some(executor),
        }
    }
```

- [ ] **Step 4: 写 MockLlmExecutor 和基础测试**

```rust
// 追加到 src-tauri/tests/s4_driver_loop_test.rs

use std::sync::Arc;
use lotus_app::runtime::cancellation::CancellationToken;
use lotus_app::runtime::chat::turn_config::*;
use lotus_app::runtime::chat::RuntimeLlmExecutor;
use lotus_app::runtime::event_bus::RuntimeEventBus;
use async_trait::async_trait;

struct MockLlmExecutor {
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
}

impl MockLlmExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self { responses: std::sync::Mutex::new(responses) }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for MockLlmExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[test]
fn mock_executor_implements_trait() {
    let executor = MockLlmExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "hello".to_string(),
            tokens_in: 10,
            tokens_out: 5,
        },
    ]);
    let arc: Arc<dyn RuntimeLlmExecutor> = Arc::new(executor);
    assert!(true); // 编译通过即为成功
}
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cd src-tauri && cargo test mock_executor_implements_trait -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T3): define RuntimeLlmExecutor trait + MockLlmExecutor + with_llm_executor constructor"
```

---

## Task 4: 新增 compaction 模块

**Files:**
- Create: `src-tauri/src/runtime/chat/compaction.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`

- [ ] **Step 1: 创建 compaction.rs，从 agent_loop Block 3+14 提取逻辑**

读取 `chat_runtime_impl.rs` 中 Block 3（L1693-L1718，daily 消息过滤 + compress_context_if_needed）和 Block 14（L2301-L2338，context_decay::apply_decay 调用），将逻辑提取为独立函数。

具体代码需要从 `chat_runtime_impl.rs` 中复制 `compress_context_if_needed` 函数体和 `context_decay::apply_decay` 调用逻辑，封装为：

```rust
// src-tauri/src/runtime/chat/compaction.rs

/// Daily 模式下过滤掉 tool call/result 消息以节省 token
pub fn filter_daily_messages(messages: &mut Vec<serde_json::Value>) {
    // 从 agent_loop Block 3 提取：messages.retain(|m| ...)
}

/// 对长对话做 LLM 摘要压缩（仅 daily 模式）
pub async fn compress_if_needed(
    messages: &mut Vec<serde_json::Value>,
    gateway: &crate::llm::gateway::LlmGateway,
    threshold_chars: usize,
) -> anyhow::Result<()> {
    // 从 compress_context_if_needed() 函数提取
}

/// 非破坏性上下文衰减（减少老旧 tool output）
pub fn apply_decay(
    messages: &[serde_json::Value],
    is_analysis: bool,
) -> Vec<serde_json::Value> {
    // 委托给已有的 context_decay::apply_decay
    crate::transport::tauri_commands::chat::chat_runtime_impl::context_decay::apply_decay(
        messages, is_analysis,
    )
}
```

注意：如果 `context_decay` 模块已在 `chat_runtime_impl.rs` 内部，需要先将其提取为公共模块，或者直接在 compaction.rs 中重新实现衰减逻辑。具体实现时需阅读原始代码决定。

- [ ] **Step 2: 更新 mod.rs**

```rust
pub mod compaction;
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/chat/compaction.rs src-tauri/src/runtime/chat/mod.rs
git commit -m "feat(S4-T4): extract compaction module from agent_loop Blocks 3+14"
```

---

## Task 5: 新增 context_builder 模块

**Files:**
- Create: `src-tauri/src/runtime/chat/context_builder.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`

- [ ] **Step 1: 创建 context_builder.rs，从 agent_loop Block 2,4,5,7,8,13 提取逻辑**

```rust
// src-tauri/src/runtime/chat/context_builder.rs

use crate::runtime::chat::turn_config::TurnConfig;

/// 构建每次迭代的动态上下文字符串（注入到 system prompt 中）
pub fn build_iteration_context(
    core_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    precompute_result: Option<&str>,
    connector_context: &str,
    analysis_ctx_prompt: Option<&str>,
) -> String {
    // 从 agent_loop Block 13 (L2214-L2299) 提取上下文拼接逻辑
    let mut parts = Vec::new();
    if !core_memory.is_empty() {
        parts.push(core_memory.to_string());
    }
    if !workspace_context.is_empty() {
        parts.push(workspace_context.to_string());
    }
    if !file_context.is_empty() {
        parts.push(file_context.to_string());
    }
    if !analysis_notes.is_empty() {
        parts.push(analysis_notes.to_string());
    }
    if let Some(precompute) = precompute_result {
        if !precompute.is_empty() {
            parts.push(precompute.to_string());
        }
    }
    if !connector_context.is_empty() {
        parts.push(connector_context.to_string());
    }
    if let Some(analysis) = analysis_ctx_prompt {
        if !analysis.is_empty() {
            parts.push(analysis.to_string());
        }
    }
    parts.join("\n\n")
}
```

实际实现需要精确复制 Block 13 中的字符串拼接格式和顺序。

- [ ] **Step 2: 更新 mod.rs**

```rust
pub mod context_builder;
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/chat/context_builder.rs src-tauri/src/runtime/chat/mod.rs
git commit -m "feat(S4-T5): extract context_builder module from agent_loop Blocks 2,5,7,13"
```

---

## Task 6: 新增 safeguard 模块

**Files:**
- Create: `src-tauri/src/runtime/chat/safeguard.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
// 追加到 src-tauri/tests/s4_driver_loop_test.rs

use lotus_app::runtime::chat::safeguard::{check_iteration, SafeguardAction};

#[test]
fn safeguard_continues_when_not_near_limit() {
    let mut injected = false;
    let action = check_iteration(0, 10, "some content", false, &mut injected);
    assert!(matches!(action, SafeguardAction::Continue));
}

#[test]
fn safeguard_daily_injects_when_near_limit_no_content() {
    let mut injected = false;
    let action = check_iteration(8, 10, "", false, &mut injected);
    assert!(matches!(action, SafeguardAction::InjectPromptAndContinue(_)));
}

#[test]
fn safeguard_analysis_forces_no_tools_at_phase2() {
    let mut injected = true; // phase 1 已注入
    let action = check_iteration(12, 15, "", true, &mut injected);
    assert!(matches!(action, SafeguardAction::ForceNoToolsAndContinue(_)));
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test safeguard_ -- --nocapture`
Expected: FAIL — 模块不存在

- [ ] **Step 3: 创建 safeguard.rs，从 agent_loop Block 27+28 提取逻辑**

```rust
// src-tauri/src/runtime/chat/safeguard.rs

/// 迭代保护策略的决策结果
#[derive(Debug)]
pub enum SafeguardAction {
    /// 无需干预，正常继续
    Continue,
    /// 注入提示消息后继续（要求 LLM 输出文本）
    InjectPromptAndContinue(String),
    /// 注入提示消息并禁用工具后继续
    ForceNoToolsAndContinue(String),
}

/// 检查当前迭代是否需要触发保护机制
///
/// Daily 模式：接近上限且无内容时注入总结要求
/// Analysis 模式（max_iterations >= 8）：三阶段保护
///   - Phase 1: 注入保存 prompt
///   - Phase 2: 注入纯文本 prompt + force_no_tools
///   - Phase 3: 下轮自动发空 tool_defs（由 force_no_tools 控制）
pub fn check_iteration(
    iteration: usize,
    max_iterations: usize,
    full_content: &str,
    is_analysis: bool,
    safeguard_phase1_injected: &mut bool,
) -> SafeguardAction {
    if is_analysis {
        check_analysis_safeguard(iteration, max_iterations, full_content, safeguard_phase1_injected)
    } else {
        check_daily_safeguard(iteration, max_iterations, full_content)
    }
}

fn check_daily_safeguard(
    iteration: usize,
    max_iterations: usize,
    full_content: &str,
) -> SafeguardAction {
    // Block 27 (L3073-L3086): daily 模式接近上限且 full_content 为空
    if max_iterations > 2
        && iteration >= max_iterations - 2
        && full_content.trim().is_empty()
    {
        SafeguardAction::InjectPromptAndContinue(
            "Please provide a brief summary of what you've done so far and any key findings.".to_string()
        )
    } else {
        SafeguardAction::Continue
    }
}

fn check_analysis_safeguard(
    iteration: usize,
    max_iterations: usize,
    full_content: &str,
    safeguard_phase1_injected: &mut bool,
) -> SafeguardAction {
    // Block 28 (L3088-L3136): analysis 三阶段保护
    if max_iterations < 8 {
        return SafeguardAction::Continue;
    }

    let remaining = max_iterations.saturating_sub(iteration);

    // Phase 1: 还剩 3 轮，注入保存 prompt
    if remaining <= 3 && !*safeguard_phase1_injected && full_content.trim().is_empty() {
        *safeguard_phase1_injected = true;
        SafeguardAction::InjectPromptAndContinue(
            "You are running low on iterations. Please save your analysis notes now using save_analysis_note, then provide a summary.".to_string()
        )
    }
    // Phase 2: 还剩 1 轮，强制纯文本
    else if remaining <= 1 && full_content.trim().is_empty() {
        SafeguardAction::ForceNoToolsAndContinue(
            "This is your final iteration. Please provide your analysis summary in text only, no tool calls.".to_string()
        )
    } else {
        SafeguardAction::Continue
    }
}
```

- [ ] **Step 4: 更新 mod.rs**

```rust
pub mod safeguard;
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cd src-tauri && cargo test safeguard_ -- --nocapture`
Expected: 3 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/safeguard.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T6): extract safeguard module from agent_loop Blocks 27+28"
```

---

## Task 7: 新增 post_process 模块

**Files:**
- Create: `src-tauri/src/runtime/chat/post_process.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
// 追加到 src-tauri/tests/s4_driver_loop_test.rs

use lotus_app::runtime::chat::post_process;

#[test]
fn finalize_adds_max_iter_notice_when_hit_limit() {
    let mut content = "partial result".to_string();
    post_process::finalize_content(
        &mut content,
        10,  // iteration_count
        10,  // max_iterations (== iteration_count, hit limit)
        false, // not cancelled
    );
    assert!(content.contains("reached the maximum"));
}

#[test]
fn finalize_sets_fallback_when_content_empty() {
    let mut content = String::new();
    post_process::finalize_content(&mut content, 1, 10, false);
    assert!(!content.is_empty()); // fallback 被设置
}

#[test]
fn finalize_no_change_for_normal_content() {
    let mut content = "normal response".to_string();
    post_process::finalize_content(&mut content, 3, 10, false);
    assert_eq!(content, "normal response");
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test finalize_ -- --nocapture`
Expected: FAIL

- [ ] **Step 3: 创建 post_process.rs，从 agent_loop Block 32 提取逻辑**

```rust
// src-tauri/src/runtime/chat/post_process.rs

/// 流式结束后的内容后处理
///
/// 1. 达到 max_iterations 时追加通知
/// 2. 空内容时设置 fallback
/// 3. 清理幻觉 XML 标签
pub fn finalize_content(
    full_content: &mut String,
    iteration_count: usize,
    max_iterations: usize,
    stream_cancelled: bool,
) {
    // Block 32 (L3246-L3285)

    // 达到 max_iterations 且未取消
    if iteration_count >= max_iterations && !stream_cancelled {
        full_content.push_str(
            "\n\n---\n*Note: I've reached the maximum number of iterations. \
             Please continue the conversation if you need more analysis.*"
        );
    }

    // 空内容 fallback
    if full_content.trim().is_empty() {
        *full_content = if stream_cancelled {
            "*(Response was cancelled)*".to_string()
        } else {
            "*(No response generated. Please try again.)*".to_string()
        };
    }

    // 清理幻觉 XML（从 strip_hallucinated_xml 提取）
    strip_hallucinated_xml(full_content);
}

fn strip_hallucinated_xml(content: &mut String) {
    // 从 chat_runtime_impl.rs 中 strip_hallucinated_xml 函数提取
    // 移除 LLM 可能产生的虚假 XML 标签如 </response>, </answer> 等
    let patterns = ["</response>", "</answer>", "</result>", "</output>"];
    for pattern in patterns {
        *content = content.replace(pattern, "");
    }
}
```

实际实现需要精确复制原始 `strip_hallucinated_xml` 和 `verify_file_claims` 的逻辑。

- [ ] **Step 4: 更新 mod.rs**

```rust
pub mod post_process;
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cd src-tauri && cargo test finalize_ -- --nocapture`
Expected: 3 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/post_process.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T7): extract post_process module from agent_loop Block 32"
```

---

## Task 8: 新增 tool_result_collector 模块

**Files:**
- Create: `src-tauri/src/runtime/chat/tool_result_collector.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
// 追加到 src-tauri/tests/s4_driver_loop_test.rs

use lotus_app::runtime::chat::tool_result_collector::{collect_results, ToolRoundResults};
use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
use lotus_app::runtime::chat::tool_round_driver::ToolRoundResult;

#[test]
fn collect_results_counts_success_and_error() {
    let results = vec![
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "search".to_string(),
            content: "found it".to_string(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc2".to_string(),
            tool_name: "load".to_string(),
            content: "error loading".to_string(),
            is_error: true,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
    ];
    let collected = collect_results(results, 8000);
    assert_eq!(collected.success_count, 1);
    assert_eq!(collected.error_count, 1);
    assert_eq!(collected.tool_result_messages.len(), 2);
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test collect_results_counts -- --nocapture`
Expected: FAIL

- [ ] **Step 3: 创建 tool_result_collector.rs，从 agent_loop Block 24 提取逻辑**

```rust
// src-tauri/src/runtime/chat/tool_result_collector.rs

use crate::runtime::chat::tool_round_driver::ToolRoundResult;
use crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

/// 工具回合收集结果
#[derive(Debug)]
pub struct ToolRoundResults {
    pub tool_result_messages: Vec<serde_json::Value>,
    pub new_file_metas: Vec<serde_json::Value>,
    pub new_generated_file_ids: Vec<String>,
    pub success_count: usize,
    pub error_count: usize,
}

/// 收集一轮工具执行的结果，构建 tool_result messages
pub fn collect_results(
    round_results: Vec<ToolRoundResult>,
    max_tool_result_chars: usize,
) -> ToolRoundResults {
    let mut tool_result_messages = Vec::new();
    let mut new_file_metas = Vec::new();
    let mut new_generated_file_ids = Vec::new();
    let mut success_count = 0;
    let mut error_count = 0;

    for result in round_results {
        match result {
            ToolRoundResult::Ok(outcome) => {
                let is_err = outcome.is_error();
                if is_err {
                    error_count += 1;
                } else {
                    success_count += 1;
                }

                // 收集 file_meta
                if let Some(meta) = outcome.file_meta() {
                    new_file_metas.push(meta.clone());
                }

                // 截断 + 构建 tool_result message
                let content = outcome.content();
                let truncated = if content.len() > max_tool_result_chars {
                    format!("{}... [truncated]", &content[..max_tool_result_chars])
                } else {
                    content.to_string()
                };

                tool_result_messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": outcome.tool_call_id(),
                    "name": outcome.tool_name(),
                    "content": truncated,
                }));
            }
            ToolRoundResult::Blocked(blocked) => {
                error_count += 1;
                tool_result_messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": blocked.tool_call_id,
                    "name": blocked.tool_name,
                    "content": format!("Tool blocked: {}", blocked.reason),
                }));
            }
        }
    }

    ToolRoundResults {
        tool_result_messages,
        new_file_metas,
        new_generated_file_ids,
        success_count,
        error_count,
    }
}
```

实际实现需要精确对齐原始 Block 24 中的 JSON 解析 file_id、PII masking 等逻辑。

- [ ] **Step 4: 更新 mod.rs**

```rust
pub mod tool_result_collector;
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cd src-tauri && cargo test collect_results_counts -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/tool_result_collector.rs src-tauri/src/runtime/chat/mod.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T8): extract tool_result_collector module from agent_loop Block 24"
```

---

## Task 9: 新增 metrics 模块

**Files:**
- Create: `src-tauri/src/runtime/chat/metrics.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`

- [ ] **Step 1: 创建 metrics.rs，从 agent_loop Block 11,26,30,31 提取日志和 telemetry 函数**

```rust
// src-tauri/src/runtime/chat/metrics.rs

use crate::runtime::chat::turn_config::{TurnConfig, TurnIterationState};
use crate::runtime::chat::tool_result_collector::ToolRoundResults;

pub fn log_context_baseline(state: &TurnIterationState, config: &TurnConfig) {
    let total_chars: usize = state.messages.iter()
        .map(|m| m.to_string().len())
        .sum();
    log::info!(
        "[CTX_METRICS] BASELINE conv={} messages={} total_chars={}",
        config.conversation_id, state.messages.len(), total_chars,
    );
}

pub fn log_iteration_metrics(
    state: &TurnIterationState,
    decayed_messages: &[serde_json::Value],
    iteration: usize,
) {
    log::debug!(
        "[CTX_METRICS] ITER={} original_msgs={} decayed_msgs={}",
        iteration, state.messages.len(), decayed_messages.len(),
    );
}

pub fn log_step_done(state: &TurnIterationState, config: &TurnConfig) {
    let total_chars: usize = state.messages.iter()
        .map(|m| m.to_string().len())
        .sum();
    log::info!(
        "[CTX_METRICS] STEP_DONE conv={} iters={} content_len={} total_chars={}",
        config.conversation_id, state.iteration_count,
        state.full_content.len(), total_chars,
    );
}

pub fn record_step_lifecycle(state: &TurnIterationState, config: &TurnConfig) {
    let status = if state.stream_cancelled {
        "cancelled"
    } else if state.iteration_count >= config.max_iterations {
        "max_iterations"
    } else {
        "completed"
    };
    log::info!(
        "[METRICS:step] conv={} status={} iters={} tokens_in={} tokens_out={}",
        config.conversation_id, status, state.iteration_count,
        state.step_tokens_in, state.step_tokens_out,
    );
}

pub fn record_tool_round(
    tool_calls: &[crate::runtime::chat::tool_round_types::RuntimeToolCallRequest],
    results: &ToolRoundResults,
) {
    log::info!(
        "[METRICS:tool_round] calls={} success={} error={}",
        tool_calls.len(), results.success_count, results.error_count,
    );
}
```

实际实现需要对齐原始 telemetry::record() 调用。如果 telemetry 函数需要额外参数（workspace_path、model 等），在签名中添加。

- [ ] **Step 2: 更新 mod.rs**

```rust
pub mod metrics;
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/chat/metrics.rs src-tauri/src/runtime/chat/mod.rs
git commit -m "feat(S4-T9): extract metrics module from agent_loop Blocks 11,26,30,31"
```

---

## Task 10: TauriLegacyTurnExecutor 实现 RuntimeLlmExecutor

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

这是最核心的实现任务。将 agent_loop 的 Block 15（gateway.stream_message）和 Block 17（流事件消费循环）提取为 `run_llm_step` 方法。

- [ ] **Step 1: 在 TauriLegacyTurnExecutor 上 impl RuntimeLlmExecutor**

在 `src-tauri/src/transport/tauri_commands/chat.rs` 中，在现有 `impl RuntimeTurnExecutor` 之后添加新的 impl block。

`run_llm_step` 的实现需要从 `agent_loop()` 的以下 Block 提取代码：

- Block 15 (L2340-L2430): `gateway.stream_message()` 调用 + 错误处理
- Block 17 (L2442-L2668): `tokio::select!` 流事件消费：
  - cancel 信号 → return `LlmStepResult::Cancelled`
  - chunk timeout → retry 或 return `TurnError::MaxRetriesExceeded`
  - ContentDelta → `bus.emit(RuntimeEventKind::StreamDelta { content })` (替换 `app.emit`)
  - ThinkingDelta → 丢弃
  - ToolCallStart → 收集到 tool_calls
  - Done → 统计 tokens, break
  - Error → `bus.emit(RuntimeEventKind::StreamError { ... })` (替换 `app.emit`)
- Block 16 (L2432-L2440): MaskingContext 合并
- Block 19 (L2691-L2731): 无工具退出 + ghost call recovery
- Block 20 (L2732-L2743): stop_reason 校验

```rust
#[async_trait]
impl RuntimeLlmExecutor for TauriLegacyTurnExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        bus: &RuntimeEventBus,
        cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let s = &self.services;

        // Block 15: 构建请求参数并调用 gateway
        let tool_defs = if input.force_no_tools {
            vec![]
        } else {
            input.tool_defs.to_vec()
        };

        let stream_result = s.gateway.stream_message(
            &input.system_prompt,
            &input.messages,
            &tool_defs,
            input.token_budget,
            // ... 其他参数从 input 中获取
        ).await;

        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                // emit StreamError via bus
                let event = RuntimeEvent::new(
                    input.conversation_id.into(),
                    input.run_id.into(),
                    RuntimeEventKind::StreamError {
                        error: e.to_string(),
                        raw_error: Some(format!("{:?}", e)),
                    },
                );
                let _ = bus.emit(event).await;
                return Err(TurnError::LlmError(e.to_string()));
            }
        };

        // Block 17: 流事件消费循环
        let mut iter_content = String::new();
        let mut tool_calls = Vec::new();
        let mut tokens_in = 0u64;
        let mut tokens_out = 0u64;
        let mut stop_reason = None;
        let chunk_timeout = tokio::time::Duration::from_secs(input.chunk_timeout_secs);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return Ok(LlmStepResult::Cancelled);
                }
                _ = tokio::time::sleep(chunk_timeout) => {
                    // chunk timeout — 简化：直接返回错误
                    // 实际实现需要 retry 逻辑
                    return Err(TurnError::MaxRetriesExceeded);
                }
                event = stream.next() => {
                    match event {
                        Some(StreamEvent::ContentDelta(delta)) => {
                            iter_content.push_str(&delta);
                            // emit StreamDelta via bus (不再 app.emit)
                            let event = RuntimeEvent::stream_delta(
                                input.conversation_id.into(),
                                input.run_id.into(),
                                delta,
                            );
                            let _ = bus.emit(event).await;
                        }
                        Some(StreamEvent::ToolCallStart(tc)) => {
                            tool_calls.push(tc);
                        }
                        Some(StreamEvent::Done(usage)) => {
                            tokens_in = usage.input_tokens;
                            tokens_out = usage.output_tokens;
                            stop_reason = Some(usage.stop_reason);
                            break;
                        }
                        Some(StreamEvent::Error(e)) => {
                            let event = RuntimeEvent::new(
                                input.conversation_id.into(),
                                input.run_id.into(),
                                RuntimeEventKind::StreamError {
                                    error: e.to_string(),
                                    raw_error: Some(format!("{:?}", e)),
                                },
                            );
                            let _ = bus.emit(event).await;
                            return Err(TurnError::LlmError(e.to_string()));
                        }
                        None => break,
                        _ => {} // ThinkingDelta 等，丢弃
                    }
                }
            }
        }

        // Block 19: 判断结果类型
        if tool_calls.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: iter_content,
                tokens_in,
                tokens_out,
            })
        } else {
            // 转换为 RuntimeToolCallRequest
            let requests = tool_calls.into_iter().map(|tc| {
                RuntimeToolCallRequest {
                    tool_call_id: tc.id,
                    tool_name: tc.name,
                    args: tc.arguments,
                    purpose: None,
                }
            }).collect();

            Ok(LlmStepResult::ToolCalls {
                assistant_content: iter_content,
                tool_calls: requests,
                tokens_in,
                tokens_out,
            })
        }
    }

    async fn persist_assistant_message(
        &self,
        conversation_id: &str,
        content: &str,
        generated_file_ids: &[String],
        file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        // 从 finish_agent() 提取 DB 写入逻辑
        // unmask PII → persist 到 DB → 返回 message_id
        // 不含 app.emit("message:updated") — 那个由 driver 通过 bus 发
        todo!("extract from finish_agent Block 33")
    }

    async fn run_precompute(
        &self,
        config: &TurnConfig,
        state: &mut TurnIterationState,
    ) -> Result<Option<String>, TurnError> {
        if config.step_config.is_none() {
            return Ok(None);
        }
        // 从 agent_loop Block 6 (L1795-L2034) 提取 precompute 逻辑
        todo!("extract from agent_loop Block 6")
    }

    async fn finalize_step(
        &self,
        state: &TurnIterationState,
        config: &TurnConfig,
    ) -> Result<(), TurnError> {
        // 从 agent_loop Block 34 (L3300-L3372) 提取 step 后处理
        todo!("extract from agent_loop Block 34")
    }
}
```

**重要**：上面的代码是骨架。实际实现时必须从 `agent_loop()` 的对应 Block 精确复制逻辑，特别是：
- stream event 的完整 match 分支（包括 ThinkingDelta 丢弃、leak detection 等）
- retry 逻辑（`stream_retry_count`、`stream_needs_retry`、`is_retryable_error` 判断）
- ghost call recovery（stop_reason == ToolUse 但 0 个 tool_calls）

- [ ] **Step 2: 运行编译检查**

Run: `cd src-tauri && cargo check`
Expected: 编译通过（todo!() 不影响编译）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "feat(S4-T10): TauriLegacyTurnExecutor implements RuntimeLlmExecutor (skeleton with todos)"
```

---

## Task 11: 填充 run_llm_step 完整实现

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: 从 agent_loop Block 15+17 精确复制流式消费逻辑**

逐行对照 `chat_runtime_impl.rs` L2340-L2668，将所有 `app.emit("streaming:delta", ...)` 替换为 `bus.emit(RuntimeEvent::stream_delta(...))`, 将 `app.emit("streaming:error", ...)` 替换为 `bus.emit(RuntimeEvent::new(..., StreamError {...}))`。

保留：retry 逻辑、leak detection、ghost call recovery。

- [ ] **Step 2: 运行编译检查**

Run: `cd src-tauri && cargo check`
Expected: 编译通过

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(S4-T11): fill run_llm_step complete implementation from agent_loop Blocks 15+17"
```

---

## Task 12: 填充 persist_assistant_message + run_precompute + finalize_step

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

- [ ] **Step 1: 从 finish_agent() 提取 DB 写入逻辑到 persist_assistant_message**

精确复制 `finish_agent()` 函数中的 unmask、leak detection、DB persist 逻辑。**不含** `app.emit("message:updated", ...)`。

- [ ] **Step 2: 从 agent_loop Block 6 提取 precompute 到 run_precompute**

精确复制 L1795-L2034 的 precompute Python 执行逻辑。保留 `app_handle` 依赖（S4 范围外）。

- [ ] **Step 3: 从 agent_loop Block 34 提取 step 后处理到 finalize_step**

精确复制 L3300-L3372 的 auto_capture + advance_step 逻辑。

- [ ] **Step 4: 运行编译检查**

Run: `cd src-tauri && cargo check`
Expected: 编译通过，无 todo!()

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "feat(S4-T12): fill persist_assistant_message, run_precompute, finalize_step"
```

---

## Task 13: 重写 RuntimeChatTurnDriver::run_chat_turn — 核心循环

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

这是 S4 最关键的任务。driver 接管整个 query loop。

- [ ] **Step 1: 写 driver 多轮迭代测试**

```rust
// 追加到 src-tauri/tests/s4_driver_loop_test.rs

use lotus_app::runtime::chat::{RuntimeChatTurnDriver, RuntimeLlmExecutor, ChatTurnRequest};
use lotus_app::runtime::chat::turn_config::*;
use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use lotus_app::runtime::event_bus::RuntimeEventBus;
use lotus_app::runtime::events::RuntimeEventKind;
use lotus_app::runtime::query_engine::QueryEngine;
use lotus_app::runtime::state::TurnState;
use lotus_app::runtime::identity::IdentityMapping;
use lotus_app::runtime::ids::RunId;
use lotus_app::runtime::cancellation::CancellationToken;

#[tokio::test]
async fn driver_loop_iterates_tool_calls_then_content() {
    // MockLlmExecutor: 第一次返回 ToolCalls，第二次返回 ContentComplete
    let executor = Arc::new(MockLlmExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "Let me search...".to_string(),
            tool_calls: vec![RuntimeToolCallRequest {
                tool_call_id: "tc-1".to_string(),
                tool_name: "echo_runtime".to_string(),
                args: serde_json::json!({"input": "hello"}),
                purpose: None,
            }],
            tokens_in: 100,
            tokens_out: 50,
        },
        LlmStepResult::ContentComplete {
            content: "Here are the results.".to_string(),
            tokens_in: 80,
            tokens_out: 30,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mapping = IdentityMapping::from_legacy("test-conv");
    let mut turn = TurnState::new(mapping, RunId::generate(), "test".to_string());
    let request = ChatTurnRequest::new("test-conv".to_string(), "hello".to_string(), vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok());

    // 验证事件中有 StreamDone 和 MessagePersisted
    let events = bus.recorded();
    let has_stream_done = events.iter().any(|e| matches!(e.kind, RuntimeEventKind::StreamDone));
    let has_msg_persisted = events.iter().any(|e| matches!(e.kind, RuntimeEventKind::MessagePersisted { .. }));
    assert!(has_stream_done, "should emit StreamDone");
    assert!(has_msg_persisted, "should emit MessagePersisted");
}

#[tokio::test]
async fn driver_loop_handles_cancel() {
    let executor = Arc::new(MockLlmExecutor::new(vec![
        LlmStepResult::Cancelled,
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mapping = IdentityMapping::from_legacy("test-conv");
    let cancel_token = CancellationToken::new();
    let mut turn = TurnState::new(mapping, RunId::generate(), "test".to_string())
        .with_cancellation(cancel_token);
    let request = ChatTurnRequest::new("test-conv".to_string(), "hello".to_string(), vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok()); // cancel 不是 error，是正常退出
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test driver_loop_ -- --nocapture`
Expected: FAIL — `run_chat_turn` 尚未使用 `llm_executor`

- [ ] **Step 3: 在 RuntimeChatTurnDriver::run_chat_turn 中添加 llm_executor 分支**

在 `chat_turn_driver.rs` 的 `run_chat_turn` 方法中，在现有逻辑之前添加新分支：

```rust
pub async fn run_chat_turn(
    &self,
    turn: &mut TurnState,
    request: &ChatTurnRequest,
) -> Result<()> {
    // S4 新路径：使用 RuntimeLlmExecutor
    if let Some(ref executor) = self.llm_executor {
        return self.run_chat_turn_s4(turn, request, executor.as_ref()).await;
    }

    // S4 之前的旧路径（legacy executor）
    // ... 保留现有代码 ...
}

/// S4 新路径：driver 拥有 query loop
async fn run_chat_turn_s4(
    &self,
    turn: &mut TurnState,
    request: &ChatTurnRequest,
    executor: &dyn RuntimeLlmExecutor,
) -> Result<()> {
    let bus = &self.event_bus;

    // 构建 TurnConfig（简化版，从 request 提取）
    let config = TurnConfig {
        system_prompt: String::new(), // 由上层构建
        tool_defs: vec![],
        allowed_tools: None,
        max_iterations: 30,
        token_budget: 4096,
        chunk_timeout_secs: 90,
        is_analysis: false,
        masking_level: "none".to_string(),
        workspace_path: std::path::PathBuf::new(),
        conversation_id: request.conversation_id.clone(),
        run_id: request.run_id.to_string(),
    };

    // 初始化可变状态
    let mut state = TurnIterationState::new(vec![]);

    // Precompute
    let _precompute_result = executor.run_precompute(&config, &mut state).await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // emit StreamStarted
    bus.emit(RuntimeEvent::new(
        turn.session_id().clone(),
        turn.run_id().clone(),
        RuntimeEventKind::StreamStarted,
    )).await?;

    // 核心迭代循环
    for iteration in 0..config.max_iterations {
        state.iteration_count = iteration;

        // 构建 executor 只读输入
        let step_input = LlmStepInput {
            system_prompt: &config.system_prompt,
            dynamic_context: "",
            messages: state.messages.clone(),
            tool_defs: &config.tool_defs,
            token_budget: config.token_budget,
            chunk_timeout_secs: config.chunk_timeout_secs,
            masking_level: &config.masking_level,
            force_no_tools: state.force_no_tools,
            conversation_id: &config.conversation_id,
            run_id: &config.run_id,
        };

        // LLM 单步调用
        let step_result = executor.run_llm_step(&step_input, bus, &turn.cancellation()).await;

        match step_result {
            Ok(LlmStepResult::ContentComplete { content, tokens_in, tokens_out }) => {
                state.full_content.push_str(&content);
                state.step_tokens_in += tokens_in;
                state.step_tokens_out += tokens_out;
                break;
            }
            Ok(LlmStepResult::Cancelled) => {
                state.stream_cancelled = true;
                break;
            }
            Ok(LlmStepResult::ToolCalls { assistant_content, tool_calls, tokens_in, tokens_out }) => {
                state.full_content.push_str(&assistant_content);
                state.step_tokens_in += tokens_in;
                state.step_tokens_out += tokens_out;

                // 工具执行
                let round_turn = TurnState::new(
                    IdentityMapping::from_legacy(&config.conversation_id),
                    turn.run_id().clone(),
                    String::new(),
                ).with_cancellation(turn.cancellation().child_token());

                let round_driver = ToolRoundDriver::new(self.query_engine.clone());
                let round_results = round_driver.execute_round(&round_turn, bus, tool_calls).await;

                // 结果收集
                let results = crate::runtime::chat::tool_result_collector::collect_results(
                    round_results, 8000,
                );

                // Driver merge
                for msg in results.tool_result_messages {
                    state.messages.push(msg);
                }
                state.all_file_metas.extend(results.new_file_metas);
                state.generated_file_ids.extend(results.new_generated_file_ids);

                // Safeguard
                match crate::runtime::chat::safeguard::check_iteration(
                    iteration, config.max_iterations,
                    &state.full_content, config.is_analysis,
                    &mut state.safeguard_phase1_injected,
                ) {
                    crate::runtime::chat::safeguard::SafeguardAction::Continue => {}
                    crate::runtime::chat::safeguard::SafeguardAction::InjectPromptAndContinue(msg) => {
                        state.messages.push(serde_json::json!({"role": "user", "content": msg}));
                    }
                    crate::runtime::chat::safeguard::SafeguardAction::ForceNoToolsAndContinue(msg) => {
                        state.messages.push(serde_json::json!({"role": "user", "content": msg}));
                        state.force_no_tools = true;
                    }
                }
            }
            Err(TurnError::Cancelled) => {
                state.stream_cancelled = true;
                break;
            }
            Err(e) => return Err(anyhow::anyhow!("{}", e)),
        }

        // 迭代末 cancel 检查
        if turn.cancellation().is_cancelled() {
            state.stream_cancelled = true;
            break;
        }
    }

    // 后处理
    crate::runtime::chat::post_process::finalize_content(
        &mut state.full_content,
        state.iteration_count,
        config.max_iterations,
        state.stream_cancelled,
    );

    // 持久化
    let message_id = executor.persist_assistant_message(
        &config.conversation_id,
        &state.full_content,
        &state.generated_file_ids,
        &state.all_file_metas,
    ).await.map_err(|e| anyhow::anyhow!("{}", e))?;

    // emit 事件
    bus.emit(RuntimeEvent::message_persisted(
        turn.session_id().clone(),
        turn.run_id().clone(),
        &message_id,
        "assistant",
        serde_json::json!(state.full_content),
    )).await?;

    bus.emit(RuntimeEvent::stream_done(
        turn.session_id().clone(),
        turn.run_id().clone(),
    )).await?;

    bus.emit(RuntimeEvent::new(
        turn.session_id().clone(),
        turn.run_id().clone(),
        RuntimeEventKind::AgentIdle {
            agent_id: turn.agent_id().unwrap_or(&"primary".into()).clone(),
            scope: crate::runtime::events::AgentIdleScope::Primary,
        },
    )).await?;

    // Step 后处理
    if config.step_config.is_some() {
        executor.finalize_step(&state, &config).await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    Ok(())
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test driver_loop_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: 运行全部 review_ 测试确认无回归**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: 全部 PASS（旧路径未被修改）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(S4-T13): RuntimeChatTurnDriver::run_chat_turn_s4 — driver owns the query loop"
```

---

## Task 14: 切换生产路径到 S4 新路径

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs` (TauriChatCommandAdapter)

- [ ] **Step 1: 将 TauriChatCommandAdapter 的构造改为使用 with_llm_executor**

在 `TauriChatCommandAdapter` 的构造函数中（`chat.rs` L131-186），将：

```rust
SessionRuntime::with_executor(qe, bus, Arc::new(TauriLegacyTurnExecutor{...}))
```

改为：

```rust
let executor = Arc::new(TauriLegacyTurnExecutor { services: services.clone() });
SessionRuntime::with_llm_executor(qe, bus, executor)
```

注意：需要在 `SessionRuntime` 上也添加对应的 `with_llm_executor` 构造函数。

- [ ] **Step 2: 运行全部测试**

Run: `cd src-tauri && cargo test --tests --no-fail-fast`
Expected: 全部 PASS

- [ ] **Step 3: 手动端到端测试**

Run: `pnpm tauri:dev`

验证：
1. 发送一条 daily 模式消息，验证流式响应正常
2. 触发工具调用（如问"搜索 xxx"），验证工具执行 + 结果返回
3. 点击"停止"按钮，验证 cancel 生效
4. 前端 `streaming:delta`、`streaming:done`、`message:updated` 事件正常触发

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/runtime/session_runtime.rs
git commit -m "feat(S4-T14): switch production path to S4 RuntimeLlmExecutor"
```

---

## Task 15: 删除 legacy 代码

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/runtime/events.rs`

- [ ] **Step 1: 删除 agent_loop() 函数（L1649-L3373）**

在 `chat_runtime_impl.rs` 中删除整个 `agent_loop()` 函数。

- [ ] **Step 2: 删除 finish_agent() 函数**

在 `chat_runtime_impl.rs` 中删除 `finish_agent()` 函数。

- [ ] **Step 3: 删除 compress_context_if_needed() 函数**

如果此函数仅被 `agent_loop` 调用，删除。

- [ ] **Step 4: 删除 legacy_send_message_impl() 中的 tokio::spawn agent_loop 逻辑**

将 `legacy_send_message_impl()` 简化为直接委托给 driver（或删除整个函数，看是否还有其他调用方）。

- [ ] **Step 5: 删除旧 RuntimeTurnExecutor trait 和相关方法**

在 `chat_turn_driver.rs` 中删除：
- `RuntimeTurnExecutor` trait（整个）
- `RuntimeChatTurnDriver` 中的 `legacy_executor` 字段
- `with_legacy_executor()` 构造函数
- `run_chat_turn` 中的旧 legacy executor 分支

将 `RuntimeLlmExecutor` 重命名为 `RuntimeTurnExecutor`（如果你想保持旧名字）。

- [ ] **Step 6: 删除 TauriLegacyTurnExecutor 上的旧 impl RuntimeTurnExecutor**

在 `chat.rs` 中删除旧的 `impl RuntimeTurnExecutor for TauriLegacyTurnExecutor` block（L99-121）。

- [ ] **Step 7: 删除 QueryEngine::run() stub echo 路径**

在 `query_engine.rs` 中，将 `run()` 方法中输出 `"runtime:{user_input}"` 的路径删除或改为 `unreachable!()`。

- [ ] **Step 8: 删除 MessagePersisted synthetic payload**

在 `chat_turn_driver.rs` 中找到 emit synthetic `MessagePersisted`（`"exec-msg-<run_id>"` + `executor_owned`）的代码，删除。

- [ ] **Step 9: 运行全部测试**

Run: `cd src-tauri && cargo test --tests --no-fail-fast`
Expected: 全部 PASS（部分旧测试可能需要更新 import）

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor(S4-T15): delete agent_loop, finish_agent, legacy executor path, QueryEngine stub"
```

---

## Task 16: 架构约束测试 + 最终验证

**Files:**
- Modify: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 添加 PluginContext 不出现在编排层的 grep 测试**

```rust
#[test]
fn review_s4_no_plugin_context_in_driver() {
    let driver_src = std::fs::read_to_string(
        "src/runtime/chat/chat_turn_driver.rs"
    ).expect("read driver source");
    assert!(
        !driver_src.contains("PluginContext"),
        "chat_turn_driver.rs must not reference PluginContext"
    );
}

#[test]
fn review_s4_no_app_emit_in_runtime() {
    let driver_src = std::fs::read_to_string(
        "src/runtime/chat/chat_turn_driver.rs"
    ).expect("read driver source");
    assert!(
        !driver_src.contains("app.emit("),
        "chat_turn_driver.rs must not directly call app.emit()"
    );
}
```

- [ ] **Step 2: 运行全部测试**

Run: `cd src-tauri && cargo test --tests --no-fail-fast`
Expected: 全部 PASS

- [ ] **Step 3: 运行前端测试确认无回归**

Run: `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`
Expected: 全部 PASS

- [ ] **Step 4: 端到端手动验证**

Run: `pnpm tauri:dev`

完整验证：
1. Daily 模式消息 → 流式响应正常
2. 工具调用 → 执行 + 结果返回 → LLM 后续回复
3. 多轮工具调用 → driver loop 正确迭代
4. 点击停止 → cancel 生效，流式停止
5. Analysis 模式（如果可用）→ precompute + 多步分析
6. 长对话 → context 压缩触发

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "test(S4-T16): add architecture constraint tests + final verification"
```

---

## 完成检查清单

| 验收条件 | 验证方式 |
|----------|---------|
| `agent_loop()` 函数不存在 | `grep -r "fn agent_loop" src-tauri/src/` 返回 0 结果 |
| `finish_agent()` 不存在 | `grep -r "fn finish_agent" src-tauri/src/` 返回 0 结果 |
| 编排层无 PluginContext | `grep -r "PluginContext" src-tauri/src/runtime/chat/` 返回 0 结果 |
| 编排层无直接 `app.emit()` | `grep -r "app\.emit(" src-tauri/src/runtime/` 返回 0 结果 |
| `RuntimeTurnExecutor::run_chat_turn` 不存在 | `grep -r "fn run_chat_turn" src-tauri/src/runtime/` 仅在 driver 的 `pub async fn run_chat_turn` 中存在 |
| `QueryEngine::run()` 无 stub echo | `grep -r "runtime:" src-tauri/src/runtime/query_engine.rs` 返回 0 结果 |
| 所有 `review_*` 测试通过 | `cd src-tauri && cargo test review_ --tests --no-fail-fast` |
| 前端事件测试通过 | `pnpm exec vitest run src/lib/tauri.events.test.ts` |
| driver 多轮迭代测试通过 | `cd src-tauri && cargo test driver_loop_ -- --nocapture` |
