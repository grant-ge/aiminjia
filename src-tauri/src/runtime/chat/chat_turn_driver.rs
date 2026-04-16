use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::post_process;
use crate::runtime::chat::safeguard::{self, SafeguardAction};
use crate::runtime::chat::tool_result_collector;
use crate::runtime::chat::tool_round_driver::ToolRoundDriver;
use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
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
        }
    }
}

/// Trait implemented by legacy turn executors.
///
/// Retained as a runtime-controlled helper during the chat-runtime-first migration.
/// The runtime (via `RuntimeChatTurnDriver`) remains the single turn entry; the
/// executor is invoked by the driver — not by `SessionRuntime` directly — to
/// preserve current LLM / transport behaviour while the migration is in progress.
/// Will be removed once Tasks 3/4 complete the migration.
///
/// ## Tool call routing
///
/// `run_chat_turn` preserves backward compatibility (returns `Result<(), String>`).
/// Executors that produce LLM-driven tool calls for the runtime to dispatch can
/// override either `run_chat_turn_with_calls` for one-shot compatibility or the
/// newer `run_llm_step` / `feed_tool_results` pair for iterative tool rounds.
/// Default implementations preserve existing behaviour, so legacy executors compile
/// without change.
#[async_trait]
pub trait RuntimeTurnExecutor: Send + Sync {
    /// Execute the chat turn.  Backward-compatible signature; override this
    /// **or** `run_chat_turn_with_calls` / `run_llm_step` (not all of them).
    async fn run_chat_turn(
        &self,
        request: ChatTurnRequest,
    ) -> std::result::Result<(), String>;

    /// Execute the chat turn and return any tool calls the LLM issued so the
    /// driver can dispatch them via `ToolRoundDriver`.
    ///
    /// Default: calls `run_chat_turn` and returns an empty Vec.  Executors that
    /// need to surface tool calls for runtime dispatch override this method.
    async fn run_chat_turn_with_calls(
        &self,
        request: ChatTurnRequest,
    ) -> std::result::Result<Vec<RuntimeToolCallRequest>, String> {
        self.run_chat_turn(request).await?;
        Ok(vec![])
    }

    /// Execute a single LLM step and return tool calls.
    ///
    /// Default: calls `run_chat_turn` (full turn) and returns an empty Vec.
    async fn run_llm_step(
        &self,
        request: ChatTurnRequest,
        _previous_tool_results: &[crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome],
    ) -> std::result::Result<Vec<RuntimeToolCallRequest>, String> {
        self.run_chat_turn(request).await?;
        Ok(vec![])
    }

    /// Receive tool execution results.
    ///
    /// Default: no-op.
    async fn feed_tool_results(
        &self,
        _outcomes: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome>,
    ) -> std::result::Result<(), String> {
        Ok(())
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

    /// Step 后处理（analysis 模式专用）。默认 no-op。
    async fn finalize_step(
        &self,
        _state: &TurnIterationState,
        _config: &TurnConfig,
    ) -> Result<(), TurnError> {
        Ok(())
    }
}

/// Runtime-owned chat turn driver.
///
/// Single entry point for chat turn orchestration.  There are two execution modes:
///
/// **No-executor mode** (pure runtime path, used in tests and future production):
///   `run_chat_turn` emits `StreamStarted`, then calls `QueryEngine::run()` which
///   emits `StreamDelta → MessagePersisted → StreamDone` on the bus.  The
///   `TauriEventAdapter` translates these to the expected frontend legacy events.
///
/// **Executor-backed mode** (current production, until Tasks 3/4 complete):
///   `run_chat_turn` emits `StreamStarted` on the bus, invokes the legacy executor
///   as a runtime-controlled helper (it performs the real LLM / tool loop), then
///   emits `MessagePersisted`, `StreamDone`, and `AgentIdle` through the bus so
///   `TauriEventAdapter` can deliver the corresponding legacy frontend events
///   (`message:updated`, `streaming:done`, `agent:idle`).
///
/// This design satisfies three constraints simultaneously:
///   1. `SessionRuntime` never full-delegates; the driver is the only turn owner.
///   2. `MessagePersisted` and `StreamDone` appear in `recorded_events()`.
///   3. `TauriEventAdapter` is notified so frontend receives `streaming:done`,
///      `message:updated`, and `agent:idle`.  `AgentGuard::clear()` and `Drop` must
///      NOT re-emit these events to avoid duplicates.
#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    /// Legacy executor helper, present on production paths during migration.
    legacy_executor: Option<Arc<dyn RuntimeTurnExecutor>>,
    /// S4 新增：新 executor，只做 provider streaming adapter。
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
}

impl RuntimeChatTurnDriver {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: None,
            llm_executor: None,
        }
    }

    pub fn with_legacy_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        legacy_executor: Arc<dyn RuntimeTurnExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: Some(legacy_executor),
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
            legacy_executor: None,
            llm_executor: Some(executor),
        }
    }

    pub async fn run_chat_turn(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
    ) -> Result<()> {
        // S4 new path takes priority.
        if let Some(ref executor) = self.llm_executor {
            return self
                .run_chat_turn_s4(turn, request, executor.as_ref())
                .await;
        }

        // Emit StreamStarted on the bus in all cases.  TauriEventAdapter does not
        // map StreamStarted to a legacy event, so this is always safe.
        self.event_bus
            .emit(RuntimeEvent::new(
                turn.session_id().clone(),
                turn.run_id().clone(),
                RuntimeEventKind::StreamStarted,
            ))
            .await?;

        if let Some(executor) = &self.legacy_executor {
            // Executor-backed mode: iterative tool-round loop.
            // `run_llm_step` returns any tool calls the LLM issued; these are
            // dispatched through `ToolRoundDriver` so the runtime owns execution.
            // Results are fed back via `feed_tool_results` for the next step.
            // When `run_llm_step` returns an empty Vec the loop ends (legacy
            // executors use the default impl that always returns vec![], so
            // behaviour is unchanged for existing executors).
            let round_driver = ToolRoundDriver::new(self.query_engine.clone());
            let mut previous_outcomes: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> = vec![];

            loop {
                let tool_calls = executor
                    .run_llm_step(request.clone(), &previous_outcomes)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;

                if tool_calls.is_empty() {
                    break;
                }

                let round_results = round_driver
                    .execute_round(turn, &self.event_bus, tool_calls)
                    .await;

                previous_outcomes = round_results
                    .iter()
                    .filter_map(|r| match r {
                        crate::runtime::ToolRoundResult::Ok(outcome) => Some(outcome.clone()),
                        crate::runtime::ToolRoundResult::Blocked(blocked) => {
                            // FIXME(S6): blocked tools could also be surfaced structurally.
                            // For now, synthesize a Completed(is_error=true) outcome so
                            // the executor receives feedback via feed_tool_results.
                            Some(crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome::Completed {
                                tool_call_id: blocked.tool_call_id.clone(),
                                tool_name: blocked.tool_name.clone(),
                                content: blocked.reason.clone(),
                                is_error: true,
                                file_meta: None,
                                is_degraded: false,
                                degradation_notice: None,
                            })
                        }
                    })
                    .collect();

                executor
                    .feed_tool_results(previous_outcomes.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
            }

            // Emit runtime events through the bus so TauriEventAdapter can
            // deliver the corresponding legacy frontend events (message:updated,
            // streaming:done, agent:idle).
            let session_id = turn.session_id().clone();
            let run_id = turn.run_id().clone();
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
        } else {
            // Pure runtime mode: QueryEngine drives the full turn and emits
            // StreamDelta → MessagePersisted → StreamDone through the bus.
            // TauriEventAdapter translates these to the expected frontend events.
            self.query_engine.run(turn, &self.event_bus).await?;
        }

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
        let config = TurnConfig {
            system_prompt: String::new(),
            tool_defs: vec![],
            allowed_tools: None,
            max_iterations: 30,
            token_budget: 4096,
            chunk_timeout_secs: 90,
            is_analysis: false,
            masking_level: "strict".to_string(),
            workspace_path: std::path::PathBuf::new(),
            conversation_id: request.conversation_id.clone(),
            run_id: request.run_id.as_str().to_string(),
        };

        // ── Step 2: Initialize iteration state ───────────────────────────────
        let mut state = TurnIterationState::new(vec![]);

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

        'turn: for iteration in 0..config.max_iterations {
            // Build a dynamic context string for this iteration.
            // Currently minimal; T14 will wire the full context_builder call.
            let dynamic_context = precompute_result.as_deref().unwrap_or_default().to_string();

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
                        state.messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": assistant_content,
                        }));
                    }
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
        self.event_bus
            .emit(RuntimeEvent::message_persisted(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!(state.full_content),
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
