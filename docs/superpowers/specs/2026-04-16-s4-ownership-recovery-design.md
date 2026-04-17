# S4：编排 Ownership 完整回收 — 设计文档（对齐 claude-code-best）

## 目标

删除 `agent_loop()`，将全部编排逻辑收归 `RuntimeChatTurnDriver`，executor 退化为 provider streaming adapter。清除所有 legacy 残留代码和直接 `app.emit()` 调用。

## 对齐 claude-code-best 的核心原则

| 原则 | claude-code-best | S4 目标 |
|------|-----------------|---------|
| **Canonical path 唯一** | `query()` AsyncGenerator 是整轮编排唯一 owner | `RuntimeChatTurnDriver::run_chat_turn` 是唯一 owner |
| **Config / State 分离** | `QueryConfig`(immutable) + `State`(mutable per-iteration) | `TurnConfig`(immutable) + `TurnIterationState`(mutable) |
| **Executor 只做 provider adapter** | `deps.callModel()` 只负责发 API 请求和解析流 | `RuntimeTurnExecutor::run_llm_step` 只负责 `gateway.stream_message` + 解析流 |
| **Driver 持有状态，executor 不改状态** | query loop 决定如何合并结果到 State | driver 决定如何合并 `LlmStepResult` 到 `TurnIterationState` |
| **事件只走总线** | 无直接 emit | 所有事件走 `RuntimeEventBus → TauriEventAdapter` |
| **Turn finalize 归 driver** | query loop 退出后处理 stop hooks、return Terminal | driver 退出循环后执行 post_process、persist、emit 事件 |

## 现状

```
SessionRuntime::run_chat_request
  → RuntimeChatTurnDriver::run_chat_turn (executor branch)
    → TauriLegacyTurnExecutor::run_chat_turn    ← 唯一 override
      → legacy_send_message_impl
        → agent_loop() ← 1700 行，持有全部编排逻辑
          → gateway.stream_message()             ← LLM streaming
          → app.emit("streaming:delta")          ← 绕过 RuntimeEventBus
          → 构建 PluginContext                   ← transport 对象下穿到编排层
          → ToolRoundDriver::execute_round       ← 唯一走 runtime 的环节
          → finish_agent()                       ← app.emit("message:updated")

driver loop 调用 executor.run_llm_step() → 默认空实现返回 vec![]
→ loop 第一次就退出 → driver 的迭代循环是无效代码
```

## 目标架构

```
SessionRuntime::run_chat_request
  → RuntimeChatTurnDriver::run_chat_turn           ← 唯一 canonical path
      |
      |— build TurnConfig (immutable)               ← context_builder
      |— init TurnIterationState (mutable)
      |— precompute (executor 提供)
      |
      |— emit StreamStarted via bus
      |
      |— loop {                                     ← driver 拥有迭代
      |     compaction::apply_decay(...)
      |     step_input = build_step_input(config, state)
      |
      |     step_result = executor.run_llm_step(step_input, bus)
      |       ← executor 只做: gateway.stream → 解析流 → 返回 LlmStepResult
      |       ← streaming delta/error 通过 bus emit，不直接 app.emit
      |
      |     match step_result {
      |       ContentComplete => driver.merge_content(state, result) → break
      |       ToolCalls(calls) => {
      |         driver 构建 ToolRoundDriver
      |         outcomes = ToolRoundDriver::execute_round(...)      ← 已有
      |         results = tool_result_collector::collect(outcomes)
      |         driver.merge_tool_results(state, results)           ← driver 合并
      |         safeguard::check_iteration(...)
      |       }
      |       Cancelled => break
      |     }
      |   }
      |
      |— post_process::finalize_content(state)       ← driver 拥有
      |— driver 持久化 assistant message 到 DB       ← driver 拥有
      |— emit MessagePersisted / StreamDone / AgentIdle via bus
```

## 范围

### 包含

- `agent_loop()` 全部 34 个 Block 的迁移和删除
- `legacy_send_message_impl()` 中 `tokio::spawn agent_loop` 逻辑删除
- `RuntimeTurnExecutor` trait 重新定义（只保留 `run_llm_step`）
- `agent_loop()` 内 20+ 处 `app.emit()` 替换为 RuntimeEventBus
- PluginContext 退出编排热路径（仅允许存在于 LegacyToolAdapter 内部兼容层）
- `finish_agent()` 删除，持久化逻辑收归 driver
- `QueryEngine::run()` stub echo 路径删除
- `RuntimeTurnExecutor` 旧方法（`run_chat_turn`、`run_chat_turn_with_calls`）删除
- `MessagePersisted` synthetic payload 删除

### 不包含

- PluginContext 从 LegacyToolAdapter 内部移除（后续工具迁移专项）
- 工具从 ToolPlugin 迁移到 RuntimeTool（后续专项）
- Python sandbox 移除（产品决策）
- Permission suspend-resume 原语（S6）

---

## 新增类型

### `TurnConfig`（不可变，Turn 级配置）

```rust
// runtime/chat/turn_config.rs

pub struct TurnConfig {
    pub system_prompt: String,
    pub tool_defs: Vec<ToolDefinition>,
    pub allowed_tools: Option<HashSet<String>>,
    pub max_iterations: usize,
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub is_analysis: bool,
    pub masking_level: MaskingLevel,
    // analysis 模式专用
    pub step_config: Option<StepConfig>,
    pub precompute_context: Option<PrecomputeContext>,
}
```

构建时机：`run_chat_turn` 入口处，构建一次后不再修改。

来源 Block：1（AgentContext 解构）、4（system prompt/tool defs/budget）、8（allowed_tools）、9（max_iterations 调整）。

### `TurnIterationState`（可变，迭代运行时状态）

```rust
// runtime/chat/turn_config.rs（同文件）

pub struct TurnIterationState {
    pub messages: Vec<ChatMessage>,
    pub full_content: String,
    pub combined_mask_ctx: MaskingContext,
    pub generated_file_ids: Vec<String>,
    pub all_file_metas: Vec<FileMeta>,
    pub iteration_count: usize,
    pub stream_cancelled: bool,
    pub step_tokens_in: u64,
    pub step_tokens_out: u64,
    pub force_no_tools: bool,
    pub safeguard_phase1_injected: bool,
}
```

Owner：`RuntimeChatTurnDriver`（driver 是唯一修改者）。

来源 Block：10（状态初始化）。

### `LlmStepInput`（executor 的只读输入）

```rust
// runtime/chat/turn_config.rs（同文件）

pub struct LlmStepInput<'a> {
    pub system_prompt: &'a str,
    pub messages: &'a [ChatMessage],   // 只读引用，executor 不能改
    pub tool_defs: &'a [ToolDefinition],
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub masking_level: MaskingLevel,
    pub force_no_tools: bool,
}
```

由 driver 从 `TurnConfig + TurnIterationState` 构建，传给 executor。executor 拿只读引用。

### `LlmStepResult`（executor 的结构化返回）

```rust
// runtime/chat/turn_config.rs（同文件）

pub enum LlmStepResult {
    /// LLM 返回了工具调用
    ToolCalls {
        assistant_content: String,
        tool_calls: Vec<RuntimeToolCallRequest>,
        mask_ctx: MaskingContext,
        tokens_in: u64,
        tokens_out: u64,
    },
    /// LLM 返回纯文本，无工具调用
    ContentComplete {
        content: String,
        mask_ctx: MaskingContext,
        tokens_in: u64,
        tokens_out: u64,
    },
    /// 用户取消
    Cancelled,
}
```

executor 只产出数据，**不修改任何外部状态**。driver 拿到 result 后自行 merge 到 `TurnIterationState`。

### `TurnError`

```rust
// runtime/chat/turn_config.rs（同文件）

pub enum TurnError {
    LlmError(String),
    Cancelled,
    MaxRetriesExceeded,
    PersistenceError(String),
}
```

---

## RuntimeTurnExecutor trait 重新定义

### 删除

```rust
// 以下方法全部删除
async fn run_chat_turn(...)
async fn run_chat_turn_with_calls(...)
async fn feed_tool_results(...)   // driver 直接 merge，不需要 executor 参与
async fn finish_turn(...)         // driver 拥有 finalize
```

### 最终定义

```rust
// runtime/chat/chat_turn_driver.rs

pub trait RuntimeTurnExecutor: Send + Sync {
    /// 单步 LLM 调用。
    /// 接收只读输入，返回结构化结果。
    /// 内部调用 gateway.stream_message()，通过 bus emit StreamDelta/StreamError。
    /// 不修改任何外部状态。
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        bus: &RuntimeEventBus,
        cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError>;

    /// Precompute 执行（analysis 模式专用，可选）。
    /// 默认实现为 no-op。
    async fn run_precompute(
        &self,
        _config: &TurnConfig,
        _state: &mut TurnIterationState,
    ) -> Result<Option<String>, TurnError> {
        Ok(None)
    }

    /// 持久化 assistant message 到存储。
    /// driver 在 finalize 阶段调用。
    async fn persist_assistant_message(
        &self,
        content: &str,
        mask_ctx: &MaskingContext,
        generated_file_ids: &[String],
        file_metas: &[FileMeta],
    ) -> Result<String, TurnError>;  // 返回 message_id
}
```

**关键约束**：
- `run_llm_step` 接收 `&LlmStepInput`（只读）+ `&RuntimeEventBus` + `&CancellationToken`，返回 `LlmStepResult`
- executor 不拿 `&mut TurnIterationState`，不能改状态
- `persist_assistant_message` 是纯 I/O，不含事件发射（事件由 driver emit）
- `run_precompute` 是唯一允许 `&mut TurnIterationState` 的方法（因为 precompute 可能修改 messages），但有默认 no-op

### TauriLegacyTurnExecutor 实现

```rust
impl RuntimeTurnExecutor for TauriLegacyTurnExecutor {
    async fn run_llm_step(&self, input, bus, cancel) -> Result<LlmStepResult, TurnError> {
        // 1. gateway.stream_message(input.system_prompt, input.messages, ...)
        // 2. tokio::select! 流事件消费：
        //    - cancel 信号 → return Cancelled
        //    - chunk timeout → retry 或 return Err
        //    - ContentDelta → 通过 bus.emit(StreamDelta) 发送（不再 app.emit）
        //    - ToolCallStart → 收集
        //    - Done → 统计 tokens, break
        //    - Error → bus.emit(StreamError), retry 或 return Err
        // 3. 返回 LlmStepResult::ToolCalls 或 ContentComplete 或 Cancelled
        //
        // 来源：Block 15 (gateway call) + Block 17 (stream loop)
    }

    async fn run_precompute(&self, config, state) -> Result<Option<String>, TurnError> {
        // Block 6: precompute Python 执行
        // 仅 analysis 模式，config.step_config.is_some() 时执行
        // 保留 app_handle 依赖（S4 范围外不清理）
    }

    async fn persist_assistant_message(&self, content, mask_ctx, file_ids, metas) -> Result<String, TurnError> {
        // Block 33 的 DB 写入部分（不含 app.emit）
        // unmask PII → leak detection → persist 到 DB → 返回 message_id
    }
}
```

---

## RuntimeChatTurnDriver::run_chat_turn 重写

```rust
pub async fn run_chat_turn(
    &self,
    turn: &mut TurnState,
    request: &ChatTurnRequest,
) -> Result<(), anyhow::Error> {
    let executor = self.legacy_executor.as_ref()
        .ok_or_else(|| anyhow::anyhow!("executor required for S4 path"))?;
    let bus = &self.event_bus;

    // ======== 构建不可变配置 ========
    let config = turn_config::build_turn_config(
        &request.settings,
        &request.step_config,
        &request.tool_registry,
        &request.authorized_workspace,
        &request.persona,
        // ...
    );

    // ======== 初始化可变状态 ========
    let mut state = TurnIterationState::new(
        request.messages.clone(),
        // ...
    );

    // ======== Precompute（analysis 模式）========
    let precompute_result = executor.run_precompute(&config, &mut state).await?;

    // ======== 迭代前准备 ========
    bus.emit(RuntimeEvent::new(turn, RuntimeEventKind::StreamStarted));
    metrics::log_context_baseline(&state, &config);

    // ======== 核心迭代循环（driver 拥有）========
    for iteration in 0..config.max_iterations {
        state.iteration_count = iteration;

        // 上下文准备
        let dynamic_ctx = context_builder::build_iteration_context(
            &config, precompute_result.as_deref(), /* ... */
        );
        let decayed_messages = compaction::apply_decay(&state.messages, config.is_analysis);
        metrics::log_iteration_metrics(&state, &decayed_messages, iteration);

        // 构建 executor 的只读输入
        let step_input = LlmStepInput {
            system_prompt: &config.system_prompt,
            messages: &decayed_messages,
            tool_defs: &config.tool_defs,
            token_budget: config.token_budget,
            chunk_timeout_secs: config.chunk_timeout_secs,
            masking_level: config.masking_level,
            force_no_tools: state.force_no_tools,
        };

        // LLM 单步调用（executor 只负责 streaming，不改状态）
        let step_result = executor.run_llm_step(
            &step_input,
            bus,
            turn.cancellation(),
        ).await;

        // Driver 处理结果
        match step_result {
            Ok(LlmStepResult::ContentComplete { content, mask_ctx, tokens_in, tokens_out }) => {
                // Driver merge
                state.full_content.push_str(&content);
                state.combined_mask_ctx.merge(mask_ctx);
                state.step_tokens_in += tokens_in;
                state.step_tokens_out += tokens_out;
                break;
            }

            Ok(LlmStepResult::Cancelled) => {
                state.stream_cancelled = true;
                break;
            }

            Ok(LlmStepResult::ToolCalls { assistant_content, tool_calls, mask_ctx, tokens_in, tokens_out }) => {
                // Driver merge assistant content
                state.full_content.push_str(&assistant_content);
                state.combined_mask_ctx.merge(mask_ctx);
                state.step_tokens_in += tokens_in;
                state.step_tokens_out += tokens_out;

                // 追加 assistant + tool_calls 到 messages
                state.messages.push(ChatMessage::assistant_with_tool_calls(
                    &assistant_content, &tool_calls,
                ));

                // 工具执行（已有 runtime 路径）
                let round_turn = TurnState::new(/* ... */)
                    .with_cancellation(turn.cancellation().child_token());
                let round_driver = ToolRoundDriver::new(&self.query_engine, config.allowed_tools.as_ref());
                let round_results = round_driver.execute_round(&round_turn, bus, &tool_calls).await;

                // 结果收集（driver 拥有 merge）
                let results = tool_result_collector::collect_results(
                    round_results,
                    &state.combined_mask_ctx,
                    MAX_TOOL_RESULT_CHARS,
                );
                metrics::record_tool_round(&tool_calls, &results);

                // Driver merge tool results 到 state
                for msg in results.tool_result_messages {
                    state.messages.push(msg);
                }
                state.all_file_metas.extend(results.new_file_metas);
                state.generated_file_ids.extend(results.new_generated_file_ids);

                // 更新 AnalysisContext（如果是 analysis 模式）
                if let Some(ref mut analysis_ctx) = state.analysis_ctx {
                    tool_result_collector::update_analysis_context(analysis_ctx, &round_results);
                }

                // 迭代保护（driver 拥有策略）
                match safeguard::check_iteration(
                    iteration, config.max_iterations,
                    &state.full_content, config.is_analysis,
                    &mut state.safeguard_phase1_injected,
                ) {
                    SafeguardAction::Continue => {}
                    SafeguardAction::InjectPromptAndContinue(msg) => {
                        state.messages.push(ChatMessage::user(&msg));
                    }
                    SafeguardAction::ForceNoToolsAndContinue(msg) => {
                        state.messages.push(ChatMessage::user(&msg));
                        state.force_no_tools = true;
                    }
                }
            }

            Err(TurnError::Cancelled) => {
                state.stream_cancelled = true;
                break;
            }
            Err(e) => return Err(e.into()),
        }

        // 迭代末 cancel 检查
        if turn.cancellation().is_cancelled() {
            state.stream_cancelled = true;
            break;
        }
    }

    // ======== 后处理（driver 拥有）========
    post_process::finalize_content(
        &mut state.full_content,
        state.iteration_count,
        config.max_iterations,
        state.stream_cancelled,
        &state.all_file_metas,
        &config.workspace_path,
    );
    metrics::log_step_done(&state, &config);
    metrics::record_step_lifecycle(&state, &config);

    // ======== 持久化（executor 做 I/O，driver 发事件）========
    let message_id = executor.persist_assistant_message(
        &state.full_content,
        &state.combined_mask_ctx,
        &state.generated_file_ids,
        &state.all_file_metas,
    ).await?;

    bus.emit(RuntimeEvent::message_persisted(turn, &message_id, "assistant", &state.full_content));
    bus.emit(RuntimeEvent::stream_done(turn));
    bus.emit(RuntimeEvent::new(turn, RuntimeEventKind::AgentIdle {
        agent_id: turn.agent_id().to_string(),
        scope: AgentIdleScope::Primary,
    }));

    // Step 后处理（analysis 模式）
    if config.step_config.is_some() {
        executor.finalize_step(&state, &config).await?;
    }

    Ok(())
}
```

---

## 新增 runtime 模块

### `runtime::chat::turn_config.rs`

Turn 级不可变配置 + 可变状态 + executor 输入/输出类型。

- `TurnConfig` — 不可变配置
- `TurnIterationState` — 可变状态（driver 唯一 owner）
- `LlmStepInput` — executor 只读输入
- `LlmStepResult` — executor 结构化返回
- `TurnError` — 结构化错误
- `build_turn_config(...)` — 构建函数

来源 Block：1, 4, 8, 9, 10。

### `runtime::chat::context_builder.rs`

每次迭代的动态上下文构建。

```rust
pub fn build_iteration_context(
    config: &TurnConfig,
    precompute_result: Option<&str>,
    analysis_ctx: Option<&AnalysisContext>,
    core_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    connector_context: &str,
) -> String
```

来源 Block：2, 5, 7, 13。

### `runtime::chat::compaction.rs`

消息压缩和上下文衰减。

```rust
pub fn filter_daily_messages(messages: &mut Vec<ChatMessage>);
pub async fn compress_if_needed(
    messages: &mut Vec<ChatMessage>,
    gateway: &dyn LlmProvider,
    settings: &AppSettings,
);
pub fn apply_decay(messages: &[ChatMessage], is_analysis: bool) -> Vec<ChatMessage>;
```

来源 Block：3, 14。

### `runtime::chat::safeguard.rs`

迭代保护策略。

```rust
pub enum SafeguardAction {
    Continue,
    InjectPromptAndContinue(String),
    ForceNoToolsAndContinue(String),
}

pub fn check_iteration(
    iteration: usize,
    max_iterations: usize,
    full_content: &str,
    is_analysis: bool,
    safeguard_phase1_injected: &mut bool,
) -> SafeguardAction
```

来源 Block：27, 28。

### `runtime::chat::post_process.rs`

流式结束后的内容后处理。

```rust
pub fn finalize_content(
    full_content: &mut String,
    iteration_count: usize,
    max_iterations: usize,
    stream_cancelled: bool,
    all_file_metas: &[FileMeta],
    workspace_path: &Path,
)
```

来源 Block：32。

### `runtime::chat::tool_result_collector.rs`

工具执行结果收集和消息构建。

```rust
pub struct ToolRoundResults {
    pub tool_result_messages: Vec<ChatMessage>,
    pub new_file_metas: Vec<FileMeta>,
    pub new_generated_file_ids: Vec<String>,
    pub success_count: usize,
    pub error_count: usize,
}

pub fn collect_results(
    round_results: Vec<Result<RuntimeToolCallOutcome, BlockedToolOutcome>>,
    mask_ctx: &MaskingContext,
    max_chars: usize,
) -> ToolRoundResults

pub fn update_analysis_context(
    analysis_ctx: &mut AnalysisContext,
    results: &[RuntimeToolCallOutcome],
)
```

来源 Block：24, 25。

### `runtime::chat::metrics.rs`

可观测性指标。

```rust
pub fn log_context_baseline(state: &TurnIterationState, config: &TurnConfig);
pub fn log_iteration_metrics(state: &TurnIterationState, decayed: &[ChatMessage], iteration: usize);
pub fn log_step_done(state: &TurnIterationState, config: &TurnConfig);
pub fn record_step_lifecycle(state: &TurnIterationState, config: &TurnConfig);
pub fn record_tool_round(tool_calls: &[RuntimeToolCallRequest], results: &ToolRoundResults);
```

来源 Block：11, 26, 30, 31。

---

## 事件迁移

### 替换表

| 当前 `app.emit()` | 替换为 |
|---|---|
| `app.emit("streaming:delta", ...)` (Block 17, 5 处) | `bus.emit(StreamDelta { content })` |
| `app.emit("streaming:error", ...)` (Block 15/17, 3 处) | `bus.emit(StreamError { error, raw_error })` — **新增 variant** |
| `finish_agent → app.emit("message:updated", ...)` (Block 33) | `bus.emit(MessagePersisted { message_id, role, content })` — **不再 synthetic** |

### 新增 RuntimeEventKind variant

```rust
StreamError {
    error: String,
    raw_error: Option<String>,
}
```

### TauriEventAdapter 新增映射

| RuntimeEventKind | Tauri Legacy Event |
|---|---|
| `StreamError` | `"streaming:error"` |

---

## 完整删除清单

| 文件 | 删除内容 | 原因 |
|---|---|---|
| `chat_runtime_impl.rs` | `agent_loop()` 函数（L1649-L3373，全部 34 个 Block） | 迁移到 runtime 模块 + driver + executor |
| `chat_runtime_impl.rs` | `finish_agent()` 函数 | 持久化移到 `executor.persist_assistant_message`，事件移到 driver |
| `chat_runtime_impl.rs` | `compress_context_if_needed()` | 迁移到 `compaction::compress_if_needed` |
| `chat_runtime_impl.rs` | `legacy_send_message_impl()` 中 `tokio::spawn agent_loop` 的逻辑 | executor 不再 spawn 独立 task |
| `chat_turn_driver.rs` | `RuntimeTurnExecutor::run_chat_turn` | 用 `run_llm_step` 替代 |
| `chat_turn_driver.rs` | `RuntimeTurnExecutor::run_chat_turn_with_calls` | 用 `run_llm_step` 替代 |
| `chat_turn_driver.rs` | `RuntimeTurnExecutor::feed_tool_results` | driver 直接 merge |
| `chat_turn_driver.rs` | `RuntimeTurnExecutor::finish_turn` | driver 拥有 finalize |
| `query_engine.rs` | `QueryEngine::run()` 中 stub echo 路径（`"runtime:{user_input}"` 输出） | 不再需要 |
| `runtime/events.rs` | `MessagePersisted` synthetic payload（`"exec-msg-<run_id>"` + `executor_owned`） | 改为真实 payload |
| `chat.rs` | `TauriLegacyTurnExecutor::run_chat_turn` override | 用 `run_llm_step` 替代 |

---

## PluginContext 在 S4 中的处理

**原则**：PluginContext 退出编排热路径，但保留在 LegacyToolAdapter 内部兼容层。

**具体**：
- `agent_loop()` Block 21 中构建 PluginContext 的代码删除
- PluginContext 的构建下沉到 `ToolDispatcher` 内部——`to_runtime_dispatcher(plugin_ctx)` 已经是这样做的，只是之前 plugin_ctx 在 agent_loop 里构建然后传进去
- S4 后，PluginContext 仅在 `LegacyToolAdapter::execute()` 内部可见，编排层（driver + executor）完全不触碰
- `app.try_state::<ConnectorEngine>()` 等全局状态获取下沉到 ToolDispatcher 的构造期（由 executor 在 run_llm_step 之外提供）

---

## 测试策略

### 必须通过的现有测试
- 所有 `review_*` 约束测试
- `tool_runtime_integration_test.rs`
- 前端事件集成测试（`tauri.events.test.ts`、`useStreaming.integration.test.tsx`、`chatStore.test.ts`）

### 新增测试

1. **Driver 多轮迭代**：MockLlmExecutor 第一次返回 `ToolCalls`，第二次返回 `ContentComplete`，验证 driver loop 正确迭代 2 次
2. **Cancel 传播**：在 `run_llm_step` 执行期间触发 cancel，验证 loop break + `state.stream_cancelled`
3. **Safeguard 触发**：设 max_iterations=3，验证 safeguard 在接近上限时注入 prompt
4. **事件顺序**：验证 StreamStarted → StreamDelta* → ToolCallExecuting → ToolCallCompleted → StreamDone → MessagePersisted → AgentIdle
5. **Ghost call recovery**：模拟 stop_reason=ToolUse 但 0 个 tool call，验证重试
6. **Config/State 隔离**：验证 executor 不能修改 TurnIterationState（编译期保证：`run_llm_step` 只接收 `&LlmStepInput`）
7. **PluginContext 不出现在编排层**：grep 验证 `chat_turn_driver.rs` 和 `run_chat_turn` 中无 PluginContext 引用

---

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 核心循环重写引入回归 | TDD：先写全部新增测试（test 1-7），再改代码 |
| precompute (Block 6) 依赖 app_handle | 保留在 `executor.run_precompute()` 中，不迁入 runtime |
| 工具执行仍需 PluginContext | 下沉到 ToolDispatcher 内部，编排层不触碰 |
| PhaseTracker 持有 AppHandle | 如果只用于 telemetry → 替换为日志；如果 emit → 改走 bus |
| `legacy_send_message_impl` 被其他路径调用 | 先 grep 确认调用方，逐一迁移或删除 |
| streaming 顺序变化导致前端闪烁 | 保持 delta 发送频率和 payload 格式不变 |
