use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;

use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::post_process;
use crate::runtime::chat::safeguard::{self, SafeguardAction};
use crate::runtime::chat::tool_result_collector;
use crate::runtime::chat::tool_round_driver::ToolRoundDriver;
use crate::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnConfig, TurnError, TurnIterationState};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{AgentIdleScope, RuntimeEvent, RuntimeEventKind};
use crate::runtime::ids::{AgentId, RunId};
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;

/// The chat turn request type.  Defined here to avoid circular imports between
/// `session_runtime` and `chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: String,
    pub content: String,
    pub file_ids: Vec<String>,
    /// The run_id assigned by `SessionRuntime` for this turn.
    /// Callers should use `ChatTurnRequest::new` for ad-hoc creation (generates a
    /// fresh id) or `SessionRuntime::run_chat_request` which overwrites the id
    /// with the single authoritative id generated for this turn.
    pub run_id: RunId,
    /// true = analysis 步骤模式；false = daily 日常模式（默认）
    pub is_analysis: bool,
}

impl ChatTurnRequest {
    pub fn new(
        conversation_id: impl Into<String>,
        content: impl Into<String>,
        file_ids: Vec<String>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            content: content.into(),
            file_ids,
            run_id: RunId::new(uuid::Uuid::new_v4().to_string()),
            is_analysis: false,
        }
    }

    /// Convenience constructor for analysis mode turns.
    pub fn new_analysis(
        conversation_id: impl Into<String>,
        content: impl Into<String>,
        file_ids: Vec<String>,
    ) -> Self {
        let mut r = Self::new(conversation_id, content, file_ids);
        r.is_analysis = true;
        r
    }
}

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

    /// 持久化 user message 到存储。纯 I/O，不含事件发射。
    /// 默认 no-op（返回空 message_id）；生产 executor 必须 override。
    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _file_ids: &[String],
    ) -> Result<String, TurnError> {
        Ok(String::new())
    }

    /// Step 后处理（analysis 模式专用）。默认 no-op。
    async fn finalize_step(
        &self,
        _state: &TurnIterationState,
        _config: &TurnConfig,
    ) -> Result<(), TurnError> {
        Ok(())
    }

    /// 构建 Turn 级的 system prompt。
    /// 由 executor 从 DB / settings / persona / product_name 合成。
    /// 默认 no-op（返回空字符串），生产 executor 必须 override。
    async fn build_system_prompt(
        &self,
        _conversation_id: &str,
        _is_analysis: bool,
    ) -> Result<String, TurnError> {
        Ok(String::new())
    }

    /// 构建发给 LLM 的当前用户消息内容。
    ///
    /// 允许生产 executor 将上传附件提示、workspace 提示等附加到用户消息，
    /// 但持久化到 DB 的原始 user message 仍由 `persist_user_message` 负责。
    async fn build_user_message_content(
        &self,
        _conversation_id: &str,
        content: &str,
        _file_ids: &[String],
    ) -> Result<String, TurnError> {
        Ok(content.to_string())
    }

    /// 返回本次 Turn 使用的 tool definitions（JSON schema）。
    ///
    /// - `is_analysis=false`（daily）：从 registry 按 DAILY_ALLOWED_TOOLS 白名单过滤
    /// - `is_analysis=true`（analysis）：全量工具
    ///
    /// 默认实现返回空 vec（向后兼容旧 mock executor）。
    /// 生产 executor（TauriLegacyTurnExecutor）必须 override。
    async fn get_tool_defs(
        &self,
        _is_analysis: bool,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }

    /// 加载 conversation 的历史对话消息（格式：[{role, content}, ...]）。
    ///
    /// 返回的消息将被插入到 messages 中（在 system-reminder 之后、当前 user message 之前）。
    /// 默认实现返回空 vec（无历史）。生产 executor 必须 override。
    async fn load_history(
        &self,
        _conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }

    /// 返回会话级环境信息字符串（工作目录、git 状态、平台）。
    ///
    /// 返回值将被注入到每次 iteration 的 dynamic_context 中。
    /// 默认实现返回空字符串（向后兼容旧 mock executor）。
    /// 生产 executor 必须 override。
    async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
        let _ = conversation_id;
        Ok(String::new())
    }
}

/// Runtime-owned chat turn driver.
///
/// Single entry point for chat turn orchestration.  There are two execution modes:
///
/// **No-executor mode** (pure runtime path, used in tests):
///   `run_chat_turn` calls `QueryEngine::run()` which emits
///   `StreamDelta → MessagePersisted → StreamDone` on the bus.  The
///   `TauriEventAdapter` translates these to the expected frontend legacy events.
///
/// **S4 executor mode** (production):
///   `run_chat_turn_s4` is the driver-owned loop.  The `RuntimeLlmExecutor` is a
///   pure provider streaming adapter; the driver owns the query/tool loop and
///   emits all lifecycle events through the bus.
#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    /// S4 executor：只做 provider streaming adapter。
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
}

const SYNTHETIC_CANCELLED_TOOL_RESULT: &str =
    "Tool execution was interrupted by user cancellation.";

/// Inject synthetic tool results for assistant tool calls that have no matching
/// tool response yet. Returns the number of injected messages.
pub fn inject_synthetic_tool_results_for_missing_calls(
    messages: &mut Vec<serde_json::Value>,
) -> usize {
    let mut pending_ids: Vec<(String, Option<String>)> = Vec::new();
    let mut seen_assistant_ids: HashSet<String> = HashSet::new();
    let mut existing_tool_result_ids: HashSet<String> = HashSet::new();

    for message in messages.iter() {
        let role = message.get("role").and_then(|v| v.as_str()).unwrap_or_default();
        if role == "assistant" {
            if let Some(tool_calls) = message.get("toolCalls").and_then(|v| v.as_array()) {
                for tc in tool_calls {
                    let Some(id) = tc.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if seen_assistant_ids.insert(id.to_string()) {
                        let name = tc
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        pending_ids.push((id.to_string(), name));
                    }
                }
            }
        } else if role == "tool" {
            let tool_call_id = message
                .get("toolCallId")
                .or_else(|| message.get("tool_call_id"))
                .and_then(|v| v.as_str());
            if let Some(id) = tool_call_id {
                existing_tool_result_ids.insert(id.to_string());
            }
        }
    }

    let mut injected = 0usize;
    for (id, name) in pending_ids {
        if existing_tool_result_ids.contains(&id) {
            continue;
        }
        messages.push(serde_json::json!({
            "role": "tool",
            "toolCallId": id,
            "name": name.unwrap_or_else(|| "unknown_tool".to_string()),
            "content": SYNTHETIC_CANCELLED_TOOL_RESULT,
        }));
        injected += 1;
    }

    injected
}

impl RuntimeChatTurnDriver {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            event_bus,
            llm_executor: None,
        }
    }

    pub fn with_llm_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        executor: Arc<dyn RuntimeLlmExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            llm_executor: Some(executor),
        }
    }

    pub async fn run_chat_turn(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
    ) -> Result<()> {
        // S4 path: when an llm_executor is present, use the S4 driver loop.
        if let Some(ref executor) = self.llm_executor {
            return self
                .run_chat_turn_s4(turn, request, executor.as_ref())
                .await;
        }

        // Pure runtime mode: QueryEngine drives the full turn and emits
        // StreamDelta → MessagePersisted → StreamDone through the bus.
        // TauriEventAdapter translates these to the expected frontend events.
        self.query_engine.run(turn, &self.event_bus).await?;

        Ok(())
    }

    /// S4 core loop: driver owns the query loop when an `RuntimeLlmExecutor` is present.
    ///
    /// ## Flow
    /// 1. Build `TurnConfig` with defaults (executor reads settings/tool_registry internally).
    /// 2. Initialize `TurnIterationState`.
    /// 3. `executor.run_precompute` (analysis mode; no-op in daily mode).
    /// 4. Emit `StreamStarted`.
    /// 5. Iteration loop (up to `config.max_iterations`):
    ///    a. Build `LlmStepInput` (read-only view).
    ///    b. Call `executor.run_llm_step` → `LlmStepResult`.
    ///    c. On `ContentComplete` → merge content, break.
    ///    d. On `Cancelled` → mark cancelled, break.
    ///    e. On `ToolCalls` → `ToolRoundDriver::execute_round`, collect results, merge,
    ///       then run safeguard checks.
    ///    f. Check cancellation at end of each iteration.
    /// 6. `post_process::finalize_content`.
    /// 7. `executor.persist_assistant_message`.
    /// 8. Emit `MessagePersisted`, `StreamDone`, `AgentIdle`.
    async fn run_chat_turn_s4(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
        executor: &dyn RuntimeLlmExecutor,
    ) -> Result<()> {
        // ── Step 1: Build TurnConfig ──────────────────────────────────────────
        // Executor reads settings and tool_registry from its own services; the
        // config fields that matter for the driver loop are max_iterations and
        // the IDs. Remaining fields are filled with sentinel defaults — T14
        // (production path switch) will enrich them.

        // Build the system prompt via the executor (reads DB/persona/auth).
        let system_prompt = executor
            .build_system_prompt(&request.conversation_id, request.is_analysis)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Build the tool definitions via the executor (daily: whitelist, analysis: all).
        let tool_defs = executor
            .get_tool_defs(request.is_analysis)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let config = TurnConfig {
            system_prompt,
            tool_defs,
            allowed_tools: None,
            max_iterations: 30,
            token_budget: 4096,
            chunk_timeout_secs: 90,
            is_analysis: request.is_analysis,
            masking_level: "strict".to_string(),
            workspace_path: std::path::PathBuf::new(),
            conversation_id: request.conversation_id.clone(),
            run_id: request.run_id.as_str().to_string(),
        };

        // ── Step 2: Initialize iteration state ───────────────────────────────
        // messages 顺序：[system-reminder, ...history, current-user-content]
        let now = chrono::Local::now();
        let today = now.format("%Y年%m月%d日");
        let today_iso = now.format("%Y-%m-%d");
        let system_reminder_message = serde_json::json!({
            "role": "user",
            "content": format!(
                "<system-reminder>\n今天是 {}（{}）。\n</system-reminder>",
                today, today_iso
            ),
        });

        let llm_user_content = executor
            .build_user_message_content(
                &request.conversation_id,
                &request.content,
                &request.file_ids,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // 加载历史对话；失败时直接返回错误，避免静默丢失上下文。
        let history = executor
            .load_history(&request.conversation_id)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let user_message = serde_json::json!({
            "role": "user",
            "content": llm_user_content,
        });

        let mut initial_messages = Vec::with_capacity(1 + history.len() + 1);
        initial_messages.push(system_reminder_message);
        initial_messages.extend(history);
        initial_messages.push(user_message);

        let mut state = TurnIterationState::new(initial_messages);

        // ── Step 2b: Persist user message (mirrors legacy_send_message_impl) ──
        // Legacy path wrote the user message to DB before spawning agent_loop.
        // The driver must do the same so the frontend message list is durable.
        let _user_msg_id = executor
            .persist_user_message(
                &request.conversation_id,
                &request.content,
                &request.file_ids,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // ── Step 3: Precompute (analysis mode; no-op in daily mode) ──────────
        let precompute_result = executor.run_precompute(&config, &mut state).await?;

        // ── Step 4: Emit StreamStarted ────────────────────────────────────────
        let session_id = turn.session_id().clone();
        let run_id = turn.run_id().clone();
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::StreamStarted,
            ))
            .await?;

        // Build the cancel token for this turn.
        let cancel = turn.cancellation();

        // ── Step 5: Iteration loop ────────────────────────────────────────────
        let round_driver = ToolRoundDriver::new(self.query_engine.clone());

        // 获取会话级环境信息（整个 turn 内稳定）
        let env_info = executor
            .get_env_info(&request.conversation_id)
            .await
            .unwrap_or_else(|e| {
                log::warn!("[run_chat_turn_s4] get_env_info failed: {}", e);
                String::new()
            });

        'turn: for iteration in 0..config.max_iterations {
            let precompute_ctx = precompute_result.as_deref().unwrap_or_default();
            let dynamic_context = if env_info.is_empty() {
                precompute_ctx.to_string()
            } else if precompute_ctx.is_empty() {
                env_info.clone()
            } else {
                format!("{}\n\n{}", env_info, precompute_ctx)
            };

            // Build the read-only executor input.
            let input = LlmStepInput {
                system_prompt: &config.system_prompt,
                dynamic_context: &dynamic_context,
                // Pass a clone of the current messages slice so executor cannot
                // mutate driver state.
                messages: state.messages.clone(),
                tool_defs: &config.tool_defs,
                token_budget: config.token_budget,
                chunk_timeout_secs: config.chunk_timeout_secs,
                masking_level: &config.masking_level,
                force_no_tools: state.force_no_tools,
                conversation_id: &config.conversation_id,
                run_id: &config.run_id,
            };

            // ── Step 5b: single LLM step ─────────────────────────────────────
            let step_result = executor
                .run_llm_step(&input, &self.event_bus, &cancel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            match step_result {
                // ── 5c: pure content response — done ─────────────────────────
                LlmStepResult::ContentComplete {
                    content,
                    tokens_in,
                    tokens_out,
                } => {
                    state.full_content.push_str(&content);
                    state.step_tokens_in += tokens_in;
                    state.step_tokens_out += tokens_out;
                    state.iteration_count = iteration + 1;
                    break 'turn;
                }

                // ── 5d: user / token cancellation ────────────────────────────
                LlmStepResult::Cancelled => {
                    inject_synthetic_tool_results_for_missing_calls(&mut state.messages);
                    state.stream_cancelled = true;
                    break 'turn;
                }

                // ── 5e: tool calls ────────────────────────────────────────────
                LlmStepResult::ToolCalls {
                    assistant_content,
                    tool_calls,
                    tokens_in,
                    tokens_out,
                } => {
                    // Merge assistant message into state so the next iteration
                    // sees the full conversation history.
                    if !assistant_content.is_empty() {
                        state.full_content.push_str(&assistant_content);
                    }
                    let normalized_tool_calls: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|call| {
                            serde_json::json!({
                                "id": call.tool_call_id,
                                "name": call.tool_name,
                                "arguments": call.args,
                            })
                        })
                        .collect();
                    state.messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": assistant_content,
                        "toolCalls": normalized_tool_calls,
                    }));
                    state.step_tokens_in += tokens_in;
                    state.step_tokens_out += tokens_out;
                    state.iteration_count = iteration + 1;

                    // Execute the tool round.
                    let round_results = round_driver
                        .execute_round(turn, &self.event_bus, tool_calls)
                        .await;

                    // Collect and merge results into state.
                    let results =
                        tool_result_collector::collect_results(round_results, 8000);
                    for msg in results.tool_result_messages {
                        state.messages.push(msg);
                    }
                    state.all_file_metas.extend(results.new_file_metas);
                    state
                        .generated_file_ids
                        .extend(results.new_generated_file_ids);

                    // Safeguard check.
                    let has_saved_note = state.messages.iter().any(|m| {
                        m.get("role").and_then(|v| v.as_str()) == Some("tool")
                            && m.get("name").and_then(|v| v.as_str())
                                == Some("save_analysis_note")
                    });
                    match safeguard::check_iteration(
                        iteration,
                        config.max_iterations,
                        &state.full_content,
                        config.is_analysis,
                        has_saved_note,
                        &mut state.safeguard_phase1_injected,
                        state.force_no_tools,
                    ) {
                        SafeguardAction::Continue => {}
                        SafeguardAction::InjectPromptAndContinue(msg) => {
                            state.messages.push(serde_json::json!({
                                "role": "user",
                                "content": msg,
                            }));
                        }
                        SafeguardAction::ForceNoToolsAndContinue(msg) => {
                            state.messages.push(serde_json::json!({
                                "role": "user",
                                "content": msg,
                            }));
                            state.force_no_tools = true;
                        }
                    }
                }
            }

            // ── 5f: per-iteration cancel check ───────────────────────────────
            if cancel.is_cancelled() {
                inject_synthetic_tool_results_for_missing_calls(&mut state.messages);
                state.stream_cancelled = true;
                break 'turn;
            }
        }

        // ── Step 6: Post-process content ──────────────────────────────────────
        post_process::finalize_content(
            &mut state.full_content,
            state.iteration_count,
            config.max_iterations,
            state.stream_cancelled,
        );

        // ── Step 7: Persist assistant message ─────────────────────────────────
        let message_id = executor
            .persist_assistant_message(
                &config.conversation_id,
                &state.full_content,
                &state.generated_file_ids,
                &state.all_file_metas,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // ── Step 8: Emit terminal events ──────────────────────────────────────
        // content must be a MessageContent object (matching the frontend Message type),
        // not a raw string.  The legacy finish_agent path always emitted {"text": "..."},
        // so we must do the same here or the frontend will discard the message.
        self.event_bus
            .emit(RuntimeEvent::message_persisted(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": state.full_content }),
            ))
            .await?;
        self.event_bus
            .emit(RuntimeEvent::stream_done(session_id.clone(), run_id.clone()))
            .await?;
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id,
                run_id.clone(),
                RuntimeEventKind::AgentIdle {
                    agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                    scope: AgentIdleScope::Primary,
                },
            ))
            .await?;

        Ok(())
    }
}
