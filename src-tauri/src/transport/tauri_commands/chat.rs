// Legacy Tauri command layer: constructs PluginContext to bootstrap tool execution.
// Suppress the deprecation lint here; this is the entry-point bridge between
// Tauri commands and the legacy PluginContext-based tool chain.
// Migrate to CapabilityContext when the command layer is refactored.
#![allow(deprecated)]
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tauri::{AppHandle, Emitter, Manager};

use crate::auth::AuthManager;
use crate::llm::analysis_context::AnalysisContext;
use crate::llm::content_filter::strip_hallucinated_xml;
use crate::llm::context_decay;
use crate::llm::gateway::LlmGateway;
use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::orchestrator::{self, StepConfig, StepStatus};
use crate::llm::prompt_guard;
use crate::llm::prompts;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent};
use crate::llm::taor::PhaseTracker;
use crate::models::settings::AppSettings;
use crate::plugin::skill_trait::{SkillState, StepAction, ToolFilter};
use crate::plugin::tool_trait::FileMeta;
use crate::plugin::{PluginContext, SkillRegistry, ToolRegistry};
use crate::runtime::conversation_service;
use crate::runtime::ids::{RunId, SessionId};
use crate::runtime::store::conversation_store::ConversationStore;
use crate::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, SessionRuntime,
};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::{
    LlmStepInput, LlmStepResult, RuntimeLlmExecutor, TurnConfig, TurnError, TurnIterationState,
};
use crate::transport::tauri_event_adapter::TauriEventAdapter;
use crate::transport::tauri_runtime_host::TauriRuntimeHost;
use crate::storage::crypto::SecureStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

mod chat_runtime_impl;
mod chat_support;

pub(crate) use chat_runtime_impl::{build_visible_tool_defs, legacy_send_message_impl};

/// Maximum agent loop iterations for daily consultation mode.
/// 30 iterations allows multi-step browser workflows: navigate → login check →
/// filter → paginate → read data across multiple pages.
const MAX_TOOL_ITERATIONS: usize = 30;

/// Maximum wall-clock time for the entire agent loop (15 minutes for multi-step analysis).
const AGENT_TIMEOUT_SECS: u64 = 900;

/// Maximum time to wait for a single chunk from the LLM stream (90 seconds).
/// If no data arrives within this window, the stream is considered stalled.
const CHUNK_TIMEOUT_SECS: u64 = 90;

/// Maximum number of stream-level retries within the agent loop.
/// When a stream error or gateway error is retryable (5xx, timeout, connection),
/// the current iteration is retried instead of aborting the entire agent loop.
const MAX_STREAM_RETRIES: u32 = 2;

/// Delay before retrying a failed stream (seconds).
const STREAM_RETRY_DELAY_SECS: u64 = 2;

/// Character threshold for triggering context compression in daily mode.
/// When total message content exceeds this, older messages are summarized.
/// ~24K chars ≈ 8K tokens (Chinese averages ~3 chars/token).
const COMPRESS_THRESHOLD_CHARS: usize = 24_000;

/// Number of recent messages to preserve (not compress) during compression.
/// These are kept verbatim so the LLM has full context for the current exchange.
const COMPRESS_KEEP_RECENT: usize = 10;

pub(crate) use chat_support::{
    auto_capture_step_context, build_config_from_skill, build_knowledge_preamble,
    clear_analysis_notes, compress_tool_result, is_daily_question, truncate_at_char_boundary,
};

#[derive(Clone)]
struct TauriChatServices {
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    crypto: Option<Arc<SecureStorage>>,
    tool_registry: Arc<ToolRegistry>,
    skill_registry: Arc<SkillRegistry>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    auth_manager: Arc<AuthManager>,
    app: tauri::AppHandle,
}

struct TauriLegacyTurnExecutor {
    services: TauriChatServices,
}

#[async_trait]
impl RuntimeTurnExecutor for TauriLegacyTurnExecutor {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> Result<(), String> {
        legacy_send_message_impl(
            self.services.db.clone(),
            self.services.gateway.clone(),
            self.services.file_mgr.clone(),
            self.services.crypto.clone(),
            self.services.tool_registry.clone(),
            self.services.skill_registry.clone(),
            self.services.session_mgr.clone(),
            self.services.auth_manager.clone(),
            self.services.app.clone(),
            request.conversation_id,
            request.content,
            request.file_ids,
            request.run_id,
            None,
        )
        .await
    }
}

#[async_trait]
impl RuntimeLlmExecutor for TauriLegacyTurnExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        bus: &RuntimeEventBus,
        cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent, ToolDefinition};
        use crate::llm::masking::MaskingLevel;
        use crate::models::settings::AppSettings;
        use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
        use crate::runtime::ids::{RunId, SessionId};
        use futures::StreamExt;

        let session_id = SessionId::from(input.conversation_id);
        let run_id = RunId::from(input.run_id);

        // --- Load settings from DB ---
        let settings: AppSettings = {
            let settings_map = self.services.db.get_all_settings().unwrap_or_default();
            if settings_map.is_empty() {
                AppSettings::default()
            } else {
                AppSettings::from_string_map(&settings_map)
            }
        };

        // --- Convert JsonValue messages to ChatMessage ---
        let chat_messages: Vec<ChatMessage> = input
            .messages
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();

        // --- Resolve masking level (always Strict; field kept for forward compat) ---
        let masking_level = match input.masking_level.to_lowercase().as_str() {
            "relaxed" => MaskingLevel::Relaxed,
            "standard" => MaskingLevel::Standard,
            _ => MaskingLevel::Strict,
        };

        // --- Build effective tool defs (empty when force_no_tools) ---
        let effective_tools: Option<Vec<ToolDefinition>> = if input.force_no_tools {
            log::info!(
                "[run_llm_step] force_no_tools=true — sending empty tool_defs (conv={})",
                input.conversation_id
            );
            Some(vec![])
        } else if input.tool_defs.is_empty() {
            None // Let gateway use its default registry
        } else {
            let defs: Vec<ToolDefinition> = input
                .tool_defs
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();
            Some(defs)
        };

        let dynamic_ctx_opt: Option<&str> = if input.dynamic_context.is_empty() {
            None
        } else {
            Some(input.dynamic_context)
        };

        // --- Retry loop: up to MAX_STREAM_RETRIES for gateway / stream errors ---
        let mut stream_retry_count: u32 = 0;
        loop {
            log::info!(
                "[run_llm_step] Calling gateway.stream_message() messages={} tools={} \
                 force_no_tools={} conv={} run={}",
                chat_messages.len(),
                effective_tools.as_ref().map_or(0, |t| t.len()),
                input.force_no_tools,
                input.conversation_id,
                input.run_id,
            );

            // --- Block 15: call gateway.stream_message ---
            let stream_result = self
                .services
                .gateway
                .stream_message(
                    &settings,
                    chat_messages.clone(),
                    masking_level.clone(),
                    Some(input.system_prompt),
                    dynamic_ctx_opt,
                    effective_tools.clone(),
                    input.token_budget as u32,
                    Some(input.conversation_id),
                )
                .await;

            let (_task_id, mut stream, _mask_ctx, mut cancel_rx) = match stream_result {
                Ok(r) => {
                    log::info!("[run_llm_step] gateway.stream_message() OK task_id={}", r.0);
                    r
                }
                Err(e) => {
                    let err_str = e.to_string();
                    log::error!("[run_llm_step] gateway.stream_message() FAILED: {}", err_str);

                    // Retry transient errors
                    if stream_retry_count < MAX_STREAM_RETRIES
                        && is_retryable_stream_error_str(&err_str)
                    {
                        stream_retry_count += 1;
                        log::warn!(
                            "[run_llm_step] Gateway error retryable (attempt {}/{}) conv={}",
                            stream_retry_count, MAX_STREAM_RETRIES, input.conversation_id
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(STREAM_RETRY_DELAY_SECS))
                            .await;
                        continue;
                    }

                    let user_error = classify_llm_error_str(&err_str);
                    let _ = bus
                        .emit(RuntimeEvent::new(
                            session_id.clone(),
                            run_id.clone(),
                            RuntimeEventKind::StreamError {
                                error: user_error.clone(),
                                raw_error: Some(truncate_str(&err_str, 200)),
                            },
                        ))
                        .await;
                    return Err(TurnError::LlmError(user_error));
                }
            };

            // --- Block 16/17: stream event loop ---
            let mut iter_content = String::new();
            let mut tool_calls = Vec::new();
            let mut stop_reason = StopReason::EndTurn;
            let mut tokens_in: u64 = 0;
            let mut tokens_out: u64 = 0;
            let mut stream_needs_retry = false;

            loop {
                // Check the runtime CancellationToken before each iteration
                if cancel.is_cancelled() {
                    log::info!(
                        "[run_llm_step] Cancel signal detected conv={}",
                        input.conversation_id
                    );
                    return Ok(LlmStepResult::Cancelled);
                }

                let chunk_timeout = tokio::time::sleep(std::time::Duration::from_secs(
                    input.chunk_timeout_secs,
                ));
                tokio::select! {
                    // Legacy cancel_rx from gateway run-registry
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() || cancel.is_cancelled() {
                            log::info!(
                                "[run_llm_step] cancel_rx fired conv={}",
                                input.conversation_id
                            );
                            return Ok(LlmStepResult::Cancelled);
                        }
                    }
                    // Chunk timeout — treat as stalled stream
                    _ = chunk_timeout => {
                        log::error!(
                            "[run_llm_step] Chunk timeout ({}s) conv={}",
                            input.chunk_timeout_secs, input.conversation_id
                        );
                        if stream_retry_count < MAX_STREAM_RETRIES {
                            stream_retry_count += 1;
                            log::warn!(
                                "[run_llm_step] Chunk timeout retryable (attempt {}/{}) conv={}",
                                stream_retry_count, MAX_STREAM_RETRIES, input.conversation_id
                            );
                            iter_content.clear();
                            tool_calls.clear();
                            stream_needs_retry = true;
                            break;
                        }
                        // All retries exhausted
                        let error_msg = format!(
                            "响应超时（{}秒无数据）。请检查网络连接后重试。",
                            input.chunk_timeout_secs
                        );
                        let _ = bus
                            .emit(RuntimeEvent::new(
                                session_id.clone(),
                                run_id.clone(),
                                RuntimeEventKind::StreamError {
                                    error: error_msg.clone(),
                                    raw_error: Some("chunk_timeout".to_string()),
                                },
                            ))
                            .await;
                        return Err(TurnError::MaxRetriesExceeded);
                    }
                    // Normal stream event
                    event = stream.next() => {
                        match event {
                            Some(StreamEvent::ContentDelta { delta }) => {
                                // Strip DeepSeek-style thinking markers before forwarding
                                let clean = strip_thinking_tag(&delta);
                                if !clean.is_empty() {
                                    iter_content.push_str(&clean);
                                    // TODO(leak-detect): skipped — check_for_leak requires
                                    // app_handle context not available in executor
                                    let _ = bus
                                        .emit(RuntimeEvent::stream_delta(
                                            session_id.clone(),
                                            run_id.clone(),
                                            clean,
                                        ))
                                        .await;
                                }
                            }
                            Some(StreamEvent::ThinkingDelta { .. }) => {
                                // ThinkingDelta: internal model reasoning — intentionally dropped.
                                // Not shown to users; bypasses prompt_guard.
                            }
                            Some(StreamEvent::ToolCallStart { tool_call }) => {
                                log::info!(
                                    "[run_llm_step] Tool call received: name='{}' id='{}'",
                                    tool_call.name, tool_call.id
                                );
                                tool_calls.push(tool_call);
                            }
                            Some(StreamEvent::Done { stop_reason: reason, usage }) => {
                                stop_reason = reason;
                                tokens_in = usage.input_tokens as u64;
                                tokens_out = usage.output_tokens as u64;
                                log::info!(
                                    "[run_llm_step] Stream done: stop_reason={:?} \
                                     in={} out={} content_len={} tool_calls={}",
                                    stop_reason, tokens_in, tokens_out,
                                    iter_content.len(), tool_calls.len()
                                );
                                // TODO(telemetry): record token usage metrics
                                break;
                            }
                            Some(StreamEvent::Error { error }) => {
                                log::error!(
                                    "[run_llm_step] Stream error: {}", error
                                );
                                if stream_retry_count < MAX_STREAM_RETRIES
                                    && is_retryable_stream_error_str(&error)
                                {
                                    stream_retry_count += 1;
                                    log::warn!(
                                        "[run_llm_step] Stream error retryable \
                                         (attempt {}/{}) conv={}",
                                        stream_retry_count, MAX_STREAM_RETRIES,
                                        input.conversation_id
                                    );
                                    iter_content.clear();
                                    tool_calls.clear();
                                    stream_needs_retry = true;
                                    break;
                                }
                                let user_error = classify_llm_error_str(&error);
                                let _ = bus
                                    .emit(RuntimeEvent::new(
                                        session_id.clone(),
                                        run_id.clone(),
                                        RuntimeEventKind::StreamError {
                                            error: user_error.clone(),
                                            raw_error: Some(truncate_str(&error, 200)),
                                        },
                                    ))
                                    .await;
                                return Err(TurnError::LlmError(user_error));
                            }
                            None => {
                                log::info!(
                                    "[run_llm_step] Stream ended (None) conv={}",
                                    input.conversation_id
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // If a retry was requested, sleep and restart the gateway call
            if stream_needs_retry {
                stream_needs_retry = false;
                log::info!(
                    "[run_llm_step] Retrying after {}s (retry {}/{}) conv={}",
                    STREAM_RETRY_DELAY_SECS, stream_retry_count, MAX_STREAM_RETRIES,
                    input.conversation_id
                );
                tokio::time::sleep(std::time::Duration::from_secs(STREAM_RETRY_DELAY_SECS))
                    .await;
                continue;
            }

            // --- Block 19: determine result ---
            if tool_calls.is_empty() {
                // Block 20: warn if stop_reason is ToolUse but no calls arrived
                if stop_reason == StopReason::ToolUse && iter_content.is_empty() {
                    // Ghost call recovery: SSE chunk loss dropped all args.
                    // Return a special marker so the driver can inject a retry prompt.
                    // For now, treat as ContentComplete — driver (T13) can handle ghost recovery.
                    log::warn!(
                        "[run_llm_step] Ghost call: stop_reason=ToolUse but 0 tool calls \
                         and empty content. Treating as ContentComplete. conv={}",
                        input.conversation_id
                    );
                }
                if stop_reason != StopReason::EndTurn && stop_reason != StopReason::MaxTokens {
                    log::warn!(
                        "[run_llm_step] Unexpected stop_reason={:?} with no tool calls conv={}",
                        stop_reason, input.conversation_id
                    );
                }
                return Ok(LlmStepResult::ContentComplete {
                    content: iter_content,
                    tokens_in,
                    tokens_out,
                });
            }

            // Block 20: warn if stop_reason mismatch
            if stop_reason != StopReason::ToolUse {
                log::warn!(
                    "[run_llm_step] stop_reason={:?} but {} tool calls received — \
                     proceeding with tool execution (possible SSE chunk loss) conv={}",
                    stop_reason, tool_calls.len(), input.conversation_id
                );
            }

            // Convert streaming ToolCall to RuntimeToolCallRequest
            let requests: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallRequest> =
                tool_calls
                    .into_iter()
                    .map(|tc| crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                        tool_call_id: tc.id,
                        tool_name: tc.name,
                        args: tc.arguments,
                        purpose: None,
                    })
                    .collect();

            return Ok(LlmStepResult::ToolCalls {
                assistant_content: iter_content,
                tool_calls: requests,
                tokens_in,
                tokens_out,
            });
        }
    }

    async fn run_precompute(
        &self,
        _config: &TurnConfig,
        _state: &mut TurnIterationState,
    ) -> Result<Option<String>, TurnError> {
        // TODO(S4-T12): extract from agent_loop Block 6 (L1795-L2034)
        // Only runs when config.step_config.is_some() (analysis mode).
        // Default no-op returns Ok(None) — acceptable for daily mode.
        Ok(None)
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        // TODO(S4-T12): extract DB persistence logic from finish_agent() in chat_runtime_impl.rs
        // - unmask PII
        // - leak detection
        // - persist to AppStorage
        // - return message_id
        // Must NOT emit app.emit("message:updated") — that is driver's responsibility via bus
        Err(TurnError::PersistenceError(
            "persist_assistant_message not yet implemented (S4-T12)".to_string(),
        ))
    }

    async fn finalize_step(
        &self,
        _state: &TurnIterationState,
        _config: &TurnConfig,
    ) -> Result<(), TurnError> {
        // TODO(S4-T12): extract from agent_loop Block 34 (L3300-L3372)
        // Only runs when config.step_config.is_some() (analysis mode).
        // - auto_capture_step_context
        // - verify analysis notes
        // - orchestrator::advance_step
        // Default no-op is acceptable for daily mode.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Private helpers for run_llm_step
// ---------------------------------------------------------------------------

/// Check if an LLM / stream error string is transient and worth retrying.
fn is_retryable_stream_error_str(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("network")
        || lower.contains("429")
        || lower.contains("rate limit")
}

/// Classify an LLM error into a user-friendly Chinese message.
fn classify_llm_error_str(error: &str) -> String {
    let lower = error.to_lowercase();
    if lower.contains("429") || lower.contains("rate limit") {
        "AI 服务请求频率超限，请稍等片刻后重试。".to_string()
    } else if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
    {
        "API 密钥无效或已过期，请在设置中检查 API Key 配置。".to_string()
    } else if lower.contains("402") || lower.contains("insufficient") || lower.contains("quota") {
        "API 额度不足，请检查账户余额。".to_string()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "AI 服务连接超时，请检查网络连接后重试。".to_string()
    } else if lower.contains("connection") || lower.contains("network") {
        "网络连接异常，请检查网络后重试。".to_string()
    } else if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        "AI 服务暂时不可用，请稍后重试。".to_string()
    } else {
        format!("服务异常：{}。请重试。", truncate_str(error, 100))
    }
}

/// Truncate a string at a character boundary for safe UI display.
fn truncate_str(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let mut t = text.chars().take(max_len).collect::<String>();
        t.push_str("...");
        t
    }
}

/// Strip DeepSeek-style internal thinking markers from a content delta.
fn strip_thinking_tag(text: &str) -> String {
    text.replace("<｜end▁of▁thinking｜>", "")
        .replace("<｜begin▁of▁thinking｜>", "")
        .replace("<|end▁of▁thinking|>", "")
        .replace("<|begin▁of▁thinking|>", "")
}

#[derive(Clone)]
pub struct TauriChatCommandAdapter {
    runtime: SessionRuntime,
    services: TauriChatServices,
}

impl TauriChatCommandAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<AppStorage>,
        gateway: Arc<LlmGateway>,
        file_mgr: Arc<FileManager>,
        crypto: Option<Arc<SecureStorage>>,
        tool_registry: Arc<ToolRegistry>,
        skill_registry: Arc<SkillRegistry>,
        session_mgr: Arc<crate::python::session::PythonSessionManager>,
        auth_manager: Arc<AuthManager>,
        app: tauri::AppHandle,
    ) -> Self {
        let services = TauriChatServices {
            db,
            gateway,
            file_mgr,
            crypto,
            tool_registry,
            skill_registry,
            session_mgr,
            auth_manager,
            app,
        };
        let host = Arc::new(TauriRuntimeHost::new(services.app.clone()));
        let adapter = Arc::new(TauriEventAdapter::new(host));
        let bus = RuntimeEventBus::new();
        bus.subscribe(adapter);
        let mut runtime = SessionRuntime::with_executor(
            QueryEngine::new().with_workspace_path(services.file_mgr.workspace_path().to_path_buf()),
            bus,
            Arc::new(TauriLegacyTurnExecutor {
                services: services.clone(),
            }),
        );
        if let Some(facade) = services
            .app
            .try_state::<Arc<crate::storage::file_store::RuntimeRepositoryFacade>>()
        {
            runtime = runtime.with_authorized_workspace_store(
                facade.inner().clone_authorized_workspace_store(),
            );
        } else {
            log::warn!(
                "[TauriChatCommandAdapter] RuntimeRepositoryFacade not registered when \
                 chat adapter was constructed. authorized_workspace_store = None. \
                 Check initialization order in lib.rs — facade must be managed before \
                 TauriChatCommandAdapter::new() is called."
            );
        }
        Self { runtime, services }
    }

    pub async fn send_message(
        &self,
        conversation_id: String,
        content: String,
        file_ids: Vec<String>,
    ) -> Result<(), String> {
        self.runtime
            .run_chat_request(ChatTurnRequest::new(conversation_id, content, file_ids))
            .await
    }

    pub fn is_agent_busy(&self) -> Vec<String> {
        self.services.gateway.get_busy_conversations()
    }

    pub async fn stop_streaming(&self, conversation_id: String) -> Result<(), String> {
        conversation_service::stop_streaming(
            self.services.gateway.clone(),
            self.services.session_mgr.clone(),
            conversation_id,
        )
        .await
    }

    pub async fn get_messages(
        &self,
        conversation_id: String,
    ) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_messages(
            self.services.db.clone() as Arc<dyn ConversationStore>,
            conversation_id,
        )
        .await
    }

    pub async fn create_conversation(&self) -> Result<String, String> {
        conversation_service::create_conversation(
            self.services.db.clone() as Arc<dyn ConversationStore>,
        )
        .await
    }

    pub async fn delete_conversation(&self, conversation_id: String) -> Result<(), String> {
        let outcome = conversation_service::delete_conversation(
            self.services.db.clone(),
            self.services.gateway.clone(),
            self.services.file_mgr.clone(),
            self.services.session_mgr.clone(),
            conversation_id,
        )
        .await?;

        if outcome.cancelled_active_agent {
            let _ = self.services.app.emit(
                "streaming:done",
                serde_json::json!({
                    "conversationId": outcome.conversation_id,
                    "messageId": "",
                }),
            );
            let _ = self.services.app.emit(
                "agent:idle",
                serde_json::json!({
                    "conversationId": outcome.conversation_id,
                }),
            );
        }

        Ok(())
    }

    pub async fn rename_conversation(
        &self,
        conversation_id: String,
        new_title: String,
    ) -> Result<(), String> {
        let outcome = conversation_service::rename_conversation(
            self.services.db.clone() as Arc<dyn ConversationStore>,
            conversation_id,
            new_title,
        )
        .await?;
        let _ = self.services.app.emit(
            "conversation:title-updated",
            serde_json::json!({
                "conversationId": outcome.conversation_id,
                "title": outcome.new_title,
            }),
        );
        Ok(())
    }

    pub async fn get_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_conversations(
            self.services.db.clone() as Arc<dyn ConversationStore>,
        )
        .await
    }
}
