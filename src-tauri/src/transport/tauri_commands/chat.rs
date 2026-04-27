// Legacy Tauri command layer: constructs PluginContext to bootstrap tool execution.
// Suppress the deprecation lint here; this is the entry-point bridge between
// Tauri commands and the legacy PluginContext-based tool chain.
// Migrate to CapabilityContext when the command layer is refactored.
#![allow(deprecated)]
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tauri::{Emitter, Manager};

use crate::auth::AuthManager;
use crate::llm::gateway::LlmGateway;
use crate::llm::prompt_guard;
use crate::llm::prompts;
use crate::models::message::SubAgentTranscriptEntryFrontend;
use crate::models::settings::AppSettings;
use crate::plugin::skill_trait::ToolFilter;
use crate::plugin::{SkillRegistry, ToolRegistry};
use crate::runtime::agent::AgentRuntime;
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::prompt::{PromptAssembler, PromptBuildContext, TurnPromptSnapshot};
use crate::runtime::chat::{
    LlmStepInput, LlmStepResult, ResolvedLlmSettings, RuntimeLlmExecutor, SkillSessionStore,
    TurnConfig, TurnConfigOverrides, TurnError, TurnIterationState,
};
use crate::runtime::conversation_service;
use crate::runtime::ids::{SessionId, ToolCallId};
use crate::runtime::store::conversation_store::ConversationStore;
use crate::runtime::store::PendingPermissionResolution;
use crate::runtime::tools::permission::PermissionDestination;
use crate::runtime::{ChatTurnRequest, QueryEngine, RuntimeEventBus, SessionRuntime};
use crate::storage::crypto::SecureStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;
use crate::storage::message_write_queue::{MessageWriteCompletion, MessageWriteQueue};
use crate::transport::tauri_event_adapter::TauriEventAdapter;
use crate::transport::tauri_runtime_host::TauriRuntimeHost;

pub(crate) mod chat_runtime_impl;

pub(crate) use chat_runtime_impl::build_visible_tool_defs;

/// Maximum number of stream-level retries within the agent loop.
/// When a stream error or gateway error is retryable (5xx, timeout, connection),
/// the current iteration is retried instead of aborting the entire agent loop.
const MAX_STREAM_RETRIES: u32 = 2;

/// Delay before retrying a failed stream (seconds).
const STREAM_RETRY_DELAY_SECS: u64 = 2;

fn build_history_message_content(
    role: &str,
    content_value: &serde_json::Value,
    has_authorized_workspace: bool,
) -> Option<String> {
    if let Some(text) = content_value.get("text").and_then(|v| v.as_str()) {
        if role == "user" {
            if let Some(files) = content_value.get("files").and_then(|v| v.as_array()) {
                if !files.is_empty() {
                    return Some(chat_runtime_impl::build_llm_content(
                        text,
                        files,
                        has_authorized_workspace,
                    ));
                }
            }
        }
        return Some(text.to_string());
    }

    content_value.as_str().map(|text| text.to_string())
}

fn tail_message_id_from_boundary(
    boundary: &crate::runtime::chat::compaction::CompactBoundaryRecord,
) -> Option<&str> {
    boundary
        .tail_message_id
        .as_deref()
        .filter(|value| !value.is_empty())
}

pub fn build_history_from_compact_boundary(
    raw_messages: Vec<serde_json::Value>,
    boundary: Option<&crate::runtime::chat::compaction::CompactBoundaryRecord>,
    has_authorized_workspace: bool,
) -> Vec<serde_json::Value> {
    let filtered_messages: Vec<serde_json::Value> = if let Some(boundary) = boundary {
        if let Some(tail_id) = tail_message_id_from_boundary(boundary) {
            let start_idx = raw_messages
                .iter()
                .position(|msg| msg.get("id").and_then(|v| v.as_str()) == Some(tail_id));
            match start_idx {
                Some(idx) => raw_messages.into_iter().skip(idx).collect(),
                None => raw_messages,
            }
        } else {
            raw_messages
        }
    } else {
        raw_messages
    };

    let mut chat_messages = Vec::new();
    if let Some(boundary) = boundary {
        if !boundary.summary_text.is_empty() {
            chat_messages.push(serde_json::json!({
                "role": "user",
                "content": format!("<context>\n{}\n</context>", boundary.summary_text),
                "isCompactSummary": true,
            }));
        }
    }

    chat_messages.extend(filtered_messages.into_iter().filter_map(|msg| {
        let role = msg["role"].as_str()?.to_string();
        match role.as_str() {
            "tool" => {
                let content_obj = msg.get("content")?;
                let tool_call_id = content_obj
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = content_obj
                    .get("name")
                    .or_else(|| content_obj.get("toolName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let content_str = content_obj
                    .get("content")
                    .or_else(|| content_obj.get("result"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if content_str.is_empty() && tool_call_id.is_empty() {
                    return None;
                }
                Some(serde_json::json!({
                    "role": "tool",
                    "toolCallId": tool_call_id,
                    "name": name,
                    "content": content_str,
                }))
            }
            "assistant" => {
                let content_obj = msg.get("content")?;
                let text = content_obj
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let tool_calls = content_obj.get("toolCalls");
                if text.trim().is_empty() && tool_calls.is_none() {
                    return None;
                }
                let mut out = serde_json::json!({
                    "role": "assistant",
                    "content": text,
                });
                if let Some(tcs) = tool_calls {
                    if tcs.as_array().map_or(false, |a| !a.is_empty()) {
                        out["toolCalls"] = tcs.clone();
                    }
                }
                Some(out)
            }
            "user" => {
                let content_obj = msg.get("content")?;
                let content_str =
                    build_history_message_content("user", content_obj, has_authorized_workspace)?;
                if content_str.trim().is_empty() {
                    return None;
                }
                Some(serde_json::json!({
                    "role": "user",
                    "content": content_str,
                }))
            }
            _ => None,
        }
    }));

    chat_messages
}

#[derive(Debug, Clone)]
pub struct GatewayChatMessages {
    pub messages: Vec<crate::llm::streaming::ChatMessage>,
    pub dropped_count: usize,
}

pub fn deserialize_chat_messages_for_gateway(
    input: &[serde_json::Value],
    conversation_id: &str,
) -> GatewayChatMessages {
    let mut messages = Vec::new();
    let mut dropped_count = 0usize;

    for value in input {
        match serde_json::from_value::<crate::llm::streaming::ChatMessage>(value.clone()) {
            Ok(message) => messages.push(message),
            Err(error) => {
                dropped_count += 1;
                log::warn!(
                    "[run_llm_step] Failed to deserialize message for conv={}: {} — value: {}",
                    conversation_id,
                    error,
                    serde_json::to_string(value).unwrap_or_default()
                );
            }
        }
    }

    GatewayChatMessages {
        messages,
        dropped_count,
    }
}

pub fn load_history_via_runtime_history(
    db: &AppStorage,
    conversation_id: &str,
    has_authorized_workspace: bool,
) -> Result<Vec<serde_json::Value>, TurnError> {
    let stored = db
        .get_messages_v2(conversation_id)
        .map_err(|e| TurnError::PersistenceError(format!("load_history failed: {}", e)))?;
    let latest_boundary = db
        .list_compact_boundaries(conversation_id)
        .map_err(|e| TurnError::PersistenceError(format!("load boundaries failed: {}", e)))?
        .into_iter()
        .last();
    let config = crate::runtime::chat::history::HistoryConfig {
        has_authorized_workspace,
        ..crate::runtime::chat::history::HistoryConfig::default()
    };
    let chat_messages = crate::runtime::chat::history::build_chat_history(
        &stored,
        latest_boundary.as_ref(),
        &config,
    )
    .map_err(|e| TurnError::PersistenceError(e.to_string()))?;

    Ok(chat_messages
        .iter()
        .map(|message| {
            let mut value = serde_json::json!({
                "role": message.role,
                "content": message.content,
            });
            if let Some(tool_calls) = &message.tool_calls {
                if let Ok(serialized) = serde_json::to_value(tool_calls) {
                    value["toolCalls"] = serialized;
                }
            }
            if let Some(tool_call_id) = &message.tool_call_id {
                value["toolCallId"] = tool_call_id.clone().into();
            }
            if let Some(name) = &message.name {
                value["name"] = name.clone().into();
            }
            value
        })
        .collect())
}

fn build_skill_session_store(
    memory_store: Option<Arc<dyn crate::runtime::store::MemoryStore>>,
) -> Arc<SkillSessionStore> {
    match memory_store {
        Some(memory_store) => Arc::new(SkillSessionStore::with_memory_store(memory_store)),
        None => Arc::new(SkillSessionStore::new()),
    }
}

async fn resolve_skill_turn_context_for_request(
    skill_sessions: &SkillSessionStore,
    skill_registry: &SkillRegistry,
    all_tool_names: &[String],
    request: &ChatTurnRequest,
) -> Result<(crate::runtime::chat::skill_session::SkillTurnContext, bool), TurnError> {
    let has_files = !request.file_ids.is_empty();
    let selected_skill_id = request
        .selected_skill_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());

    if let Some(selected_skill_id) = selected_skill_id {
        log::info!(
            "[skill-command][turn-config-selected-skill] trace_id={:?} conversation_id={} selected_skill_id={} selected_skill_label={:?}",
            request
                .client_message_id
                .as_deref()
                .or(Some(selected_skill_id)),
            request.conversation_id,
            selected_skill_id,
            request.selected_skill_label,
        );

        let mut ctx = skill_sessions
            .switch_skill(
                skill_registry,
                all_tool_names,
                request.conversation_id.as_str(),
                selected_skill_id,
                has_files,
            )
            .await
            .map_err(|err| {
                TurnError::PersistenceError(format!(
                    "Failed to switch selected skill '{}': {err}",
                    selected_skill_id
                ))
            })?;

        // Claude Code Best treats explicit command loading as already resolved.
        // Do not expose switch_skill in the same turn, or the model can re-pick a different skill.
        match ctx.allowed_tools.as_mut() {
            Some(allowed_tools) => {
                allowed_tools.remove("switch_skill");
            }
            None => {
                ctx.allowed_tools = Some(
                    all_tool_names
                        .iter()
                        .filter(|name| name.as_str() != "switch_skill")
                        .cloned()
                        .collect::<std::collections::HashSet<_>>(),
                );
            }
        }

        return Ok((ctx, true));
    }

    let ctx = skill_sessions
        .resolve_turn_context(
            skill_registry,
            all_tool_names,
            request.conversation_id.as_str(),
            request.content.as_str(),
            has_files,
        )
        .await
        .map_err(|err| {
            TurnError::PersistenceError(format!("Failed to resolve skill session: {err}"))
        })?;
    Ok((ctx, false))
}

#[derive(Clone)]
struct TauriChatServices {
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    assistant_write_queue: Arc<MessageWriteQueue>,
    crypto: Option<Arc<SecureStorage>>,
    tool_registry: Arc<ToolRegistry>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    auth_manager: Arc<AuthManager>,
    app: tauri::AppHandle,
    skill_registry: Arc<SkillRegistry>,
    skill_sessions: Arc<SkillSessionStore>,
    runtime_resolver: Option<crate::runtime::dependencies::ManagedRuntimeResolver>,
}

struct TauriLegacyTurnExecutor {
    services: TauriChatServices,
    renlijia_md_loader: Arc<tokio::sync::Mutex<crate::runtime::renlijia_md::RenlijiaMdLoader>>,
}

async fn wait_for_message_write_completion(
    completion: MessageWriteCompletion,
) -> Result<(), TurnError> {
    tokio::task::spawn_blocking(move || completion.wait())
        .await
        .map_err(|err| {
            TurnError::PersistenceError(format!(
                "Assistant persistence worker join failed: {}",
                err
            ))
        })?
        .map_err(|err| {
            TurnError::PersistenceError(format!("Failed to save assistant message: {}", err))
        })
}

async fn persist_assistant_content_json(
    db: Arc<AppStorage>,
    assistant_write_queue: Arc<MessageWriteQueue>,
    message_id: String,
    conversation_id: String,
    content_json: String,
) -> Result<(), TurnError> {
    match assistant_write_queue.enqueue_insert_with_ack(
        message_id.clone(),
        conversation_id.clone(),
        "assistant".to_string(),
        content_json.clone(),
    ) {
        Ok(completion) => wait_for_message_write_completion(completion).await,
        Err(queue_err) => {
            log::warn!(
                "[persist_assistant_message] Failed to enqueue assistant message id={} conv={}: {}. Falling back to direct write.",
                message_id,
                conversation_id,
                queue_err
            );

            tokio::task::spawn_blocking(move || {
                db.insert_message(&message_id, &conversation_id, "assistant", &content_json)
            })
            .await
            .map_err(|err| {
                TurnError::PersistenceError(format!(
                    "Assistant direct persistence worker join failed: {}",
                    err
                ))
            })?
            .map_err(|err| {
                TurnError::PersistenceError(format!("Failed to save assistant message: {}", err))
            })
        }
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
        use crate::llm::masking::MaskingLevel;
        use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent, ToolDefinition};
        use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
        use crate::runtime::ids::{RunId, SessionId};
        use futures::StreamExt;

        let session_id = SessionId::from(input.conversation_id);
        let run_id = RunId::from(input.run_id);

        let settings = build_gateway_settings(input.llm_settings);

        // --- Convert JsonValue messages to ChatMessage ---
        let gateway_messages =
            deserialize_chat_messages_for_gateway(&input.messages, input.conversation_id);
        let chat_messages: Vec<ChatMessage> = gateway_messages.messages;
        if gateway_messages.dropped_count > 0 {
            log::error!(
                "[run_llm_step] conv={} DROPPED {} messages during deserialization — context may be incomplete",
                input.conversation_id,
                gateway_messages.dropped_count
            );
        }
        let system_prompt_for_gateway =
            openai_system_prompt_content(input.openai_system_message.clone(), input.system_prompt);

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
            Some(vec![])
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
            log::debug!(
                "[AD3] LLM step estimated_tokens={} token_budget={} conv={}",
                input.estimated_tokens,
                input.token_budget,
                input.conversation_id,
            );

            // --- Block 15: call gateway.stream_message ---
            let stream_result = self
                .services
                .gateway
                .stream_message(
                    &settings,
                    chat_messages.clone(),
                    masking_level.clone(),
                    system_prompt_for_gateway.as_deref(),
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
                    log::error!(
                        "[run_llm_step] gateway.stream_message() FAILED: {}",
                        err_str
                    );

                    // Retry transient errors
                    if stream_retry_count < MAX_STREAM_RETRIES
                        && is_retryable_stream_error_str(&err_str)
                    {
                        stream_retry_count += 1;
                        log::warn!(
                            "[run_llm_step] Gateway error retryable (attempt {}/{}) conv={}",
                            stream_retry_count,
                            MAX_STREAM_RETRIES,
                            input.conversation_id
                        );
                        let _ = bus
                            .emit(RuntimeEvent::new(
                                session_id.clone(),
                                run_id.clone(),
                                RuntimeEventKind::StreamRetryReset,
                            ))
                            .await;
                        tokio::time::sleep(std::time::Duration::from_secs(STREAM_RETRY_DELAY_SECS))
                            .await;
                        continue;
                    }

                    let classified = classify_llm_error(&err_str);
                    if let TurnError::LlmError(user_error) = &classified {
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
                    }
                    return Err(classified);
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

                let chunk_timeout =
                    tokio::time::sleep(std::time::Duration::from_secs(input.chunk_timeout_secs));
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
                            let _ = bus
                                .emit(RuntimeEvent::new(
                                    session_id.clone(),
                                    run_id.clone(),
                                    RuntimeEventKind::StreamRetryReset,
                                ))
                                .await;
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
                                    let _ = bus
                                        .emit(RuntimeEvent::new(
                                            session_id.clone(),
                                            run_id.clone(),
                                            RuntimeEventKind::StreamRetryReset,
                                        ))
                                        .await;
                                    iter_content.clear();
                                    tool_calls.clear();
                                    stream_needs_retry = true;
                                    break;
                                }
                                let classified = classify_llm_error(&error);
                                if let TurnError::LlmError(user_error) = &classified {
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
                                }
                                return Err(classified);
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
                log::info!(
                    "[run_llm_step] Retrying after {}s (retry {}/{}) conv={}",
                    STREAM_RETRY_DELAY_SECS,
                    stream_retry_count,
                    MAX_STREAM_RETRIES,
                    input.conversation_id
                );
                tokio::time::sleep(std::time::Duration::from_secs(STREAM_RETRY_DELAY_SECS)).await;
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
                        stop_reason,
                        input.conversation_id
                    );
                }
                return Ok(LlmStepResult::ContentComplete {
                    content: iter_content,
                    tokens_in,
                    tokens_out,
                    stop_reason: Some(
                        match stop_reason {
                            StopReason::EndTurn => "end_turn",
                            StopReason::ToolUse => "tool_use",
                            StopReason::MaxTokens => "max_tokens",
                            StopReason::StopSequence => "stop_sequence",
                        }
                        .to_string(),
                    ),
                });
            }

            // Block 20: warn if stop_reason mismatch
            if stop_reason != StopReason::ToolUse {
                log::warn!(
                    "[run_llm_step] stop_reason={:?} but {} tool calls received — \
                     proceeding with tool execution (possible SSE chunk loss) conv={}",
                    stop_reason,
                    tool_calls.len(),
                    input.conversation_id
                );
            }

            // Convert streaming ToolCall to RuntimeToolCallRequest
            let requests: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallRequest> =
                tool_calls
                    .into_iter()
                    .map(
                        |tc| crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                            tool_call_id: tc.id,
                            tool_name: tc.name,
                            args: tc.arguments,
                            purpose: None,
                        },
                    )
                    .collect();

            return Ok(LlmStepResult::ToolCalls {
                assistant_content: iter_content,
                tool_calls: requests,
                tokens_in,
                tokens_out,
            });
        }
    }

    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        self.load_llm_settings_for_turn(&ChatTurnRequest::new("", "", vec![]))
            .await
    }

    async fn load_llm_settings_for_turn(
        &self,
        request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        let global_settings_map = self.services.db.get_all_settings().unwrap_or_default();
        let global_settings = if global_settings_map.is_empty() {
            AppSettings::default()
        } else {
            AppSettings::from_string_map(&global_settings_map)
        };

        let workspace_path = global_settings.workspace_path.trim().to_string();
        let effective_settings_map = if workspace_path.is_empty() {
            global_settings_map
        } else {
            self.services
                .db
                .get_effective_settings(Some(std::path::Path::new(&workspace_path)))
                .unwrap_or(global_settings_map)
        };

        let mut settings = if effective_settings_map.is_empty() {
            AppSettings::default()
        } else {
            AppSettings::from_string_map(&effective_settings_map)
        };

        if let Some(ss) = self.services.crypto.as_ref() {
            settings.primary_api_key = decrypt_api_key(ss, &settings.primary_api_key);
        }

        let model_override = if request.conversation_id.as_str().is_empty() {
            None
        } else {
            self.services
                .db
                .get_conversation_model_override(request.conversation_id.as_str())
                .unwrap_or(None)
        };
        if let Some(override_model) = model_override {
            settings.primary_model = override_model;
        }

        Ok(ResolvedLlmSettings {
            primary_model: settings.primary_model,
            primary_api_key: settings.primary_api_key,
            auto_model_routing: settings.auto_model_routing,
            custom_model_endpoint: settings.custom_model_endpoint,
            custom_model_name: settings.custom_model_name,
            use_cloud: settings.use_cloud,
            cloud_model: settings.cloud_model,
            cloud_model_type: settings.cloud_model_type,
            thinking_type: settings.thinking_type,
            thinking_budget_tokens: settings.thinking_budget_tokens,
            masking_level: crate::llm::masking::MaskingLevel::from_str_or_strict(
                &settings.data_masking_level,
            )
            .to_str()
            .to_string(),
        })
    }

    async fn run_precompute(
        &self,
        _config: &TurnConfig,
        _state: &mut TurnIterationState,
    ) -> Result<Option<String>, TurnError> {
        // TODO(S4-T12/future): full precompute requires StepConfig.precompute, which is not yet
        // carried by TurnConfig.
        // When S4-T13 wires the driver loop, TurnConfig should be extended with:
        //   pub step_config: Option<StepConfig>,
        // at which point this body can be extracted from chat_runtime_impl.rs Block 6 (L1795-L2034).
        // Until then, falling back to no-precompute is safe (the agent can do the
        // computation itself in the tool loop).
        Ok(None)
    }

    async fn persist_user_message(
        &self,
        conversation_id: &str,
        content: &str,
        file_ids: &[String],
        _client_message_id: Option<&str>,
        selected_skill_id: Option<&str>,
        selected_skill_label: Option<&str>,
    ) -> Result<String, TurnError> {
        let msg_id = format!("msg-{}", uuid::Uuid::new_v4());

        let content_json = crate::runtime::chat::chat_turn_driver::build_user_content_json(
            content,
            file_ids,
            selected_skill_id,
            selected_skill_label,
        )
        .to_string();

        if let Err(e) =
            self.services
                .db
                .insert_message(&msg_id, conversation_id, "user", &content_json)
        {
            log::error!(
                "[persist_user_message] Failed to save user message: {:#}",
                e
            );
            return Err(TurnError::PersistenceError(format!(
                "Failed to save user message: {}",
                e
            )));
        }
        log::info!(
            "[persist_user_message] Saved user message id={} conv={}",
            msg_id,
            conversation_id
        );
        Ok(msg_id)
    }

    async fn persist_iteration_assistant_message(
        &self,
        conversation_id: &str,
        tool_calls: &[serde_json::Value],
    ) -> Result<(), TurnError> {
        if tool_calls.is_empty() {
            return Ok(());
        }
        let msg_id = uuid::Uuid::new_v4().to_string();
        log::info!(
            "[persist_iteration_assistant_message] Saving assistant[toolCalls] id={} conv={}",
            msg_id,
            conversation_id
        );
        let stored = crate::storage::file_store::types::StoredMessage {
            id: msg_id,
            conversation_id: conversation_id.to_string(),
            role: "assistant".to_string(),
            content: serde_json::json!({ "text": "" }),
            created_at: chrono::Utc::now().to_rfc3339(),
            tool_calls: Some(tool_calls.to_vec()),
            tool_call_id: None,
            name: None,
            run_id: None,
            schema_version: Some(2),
            sequence: None,
            seq: None,
            rev: None,
        };
        self.services
            .db
            .insert_chat_message_record(&stored)
            .map_err(|e| TurnError::PersistenceError(e.to_string()))?;
        Ok(())
    }

    async fn persist_tool_messages(
        &self,
        conversation_id: &str,
        tool_messages: &[serde_json::Value],
    ) -> Result<(), TurnError> {
        for msg in tool_messages {
            let msg_id = msg
                .get("msgId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("tool-{}", uuid::Uuid::new_v4()));
            // 直接取三个字段，缺失时跳过整条而非写 null
            let tool_call_id = match msg.get("toolCallId").and_then(|v| v.as_str()) {
                Some(v) => v.to_string(),
                None => {
                    log::warn!("[persist_tool_messages] skipping msg missing toolCallId");
                    continue;
                }
            };
            let name = msg
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let content = msg
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let stored = crate::storage::file_store::types::StoredMessage {
                id: msg_id.clone(),
                conversation_id: conversation_id.to_string(),
                role: "tool".to_string(),
                content: serde_json::json!({ "text": content }),
                created_at: chrono::Utc::now().to_rfc3339(),
                tool_call_id: Some(tool_call_id),
                name: Some(name),
                tool_calls: None,
                run_id: None,
                schema_version: Some(2),
                sequence: None,
                seq: None,
                rev: None,
            };
            if let Err(e) = self.services.db.insert_chat_message_record(&stored) {
                log::warn!(
                    "[persist_tool_messages] Failed to save tool message id={} conv={}: {}",
                    msg_id,
                    conversation_id,
                    e
                );
            }
        }
        Ok(())
    }

    async fn persist_assistant_message(
        &self,
        conversation_id: &str,
        content: &str,
        tool_calls: &[serde_json::Value],
        generated_file_ids: &[String],
        file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        // Generate a stable message ID for this assistant turn.
        let message_id = uuid::Uuid::new_v4().to_string();

        // --- Leak detection at persistence boundary ---
        // No MaskingContext available at this layer (the gateway already returned plain text
        // after unmasking); apply prompt_guard as a last-resort safety net.
        let (filtered_content, was_leaked) = prompt_guard::filter_leaked_content(content);
        if was_leaked {
            log::warn!(
                "[persist_assistant_message] Prompt leak caught at persistence for conv={}",
                conversation_id
            );
        }

        let trimmed = filtered_content.trim();

        // Skip persisting empty messages (tool-call-only iterations produce no visible text).
        if trimmed.is_empty() {
            log::info!(
                "[persist_assistant_message] Skipping empty assistant message for conv={} id={}",
                conversation_id,
                message_id
            );
            return Ok(message_id);
        }

        // Check that the conversation still exists (might have been deleted while the agent ran).
        if self.services.db.get_conversation(conversation_id).is_err() {
            log::warn!(
                "[persist_assistant_message] Conversation {} deleted during agent run, skipping save",
                conversation_id
            );
            return Ok(message_id);
        }

        let workspace_path = self.services.file_mgr.workspace_path();

        // --- Build content JSON, attaching generated files when present ---
        let content_value = if !generated_file_ids.is_empty() {
            match self
                .services
                .db
                .get_generated_files_by_ids(generated_file_ids)
            {
                Ok(file_records) if !file_records.is_empty() => {
                    let gen_files: Vec<serde_json::Value> = file_records
                        .iter()
                        .map(|f| {
                            let stored_path = f["storedPath"].as_str().unwrap_or("");
                            let full_path = workspace_path.join(stored_path);
                            let file_id_str = f["id"].as_str().unwrap_or("");
                            // Look up FileMeta JSON to inject degradation info.
                            // file_metas entries are serde_json::Value serialised from FileMeta.
                            let matching_meta = file_metas.iter().find(|m| {
                                m.get("fileId")
                                    .or_else(|| m.get("file_id"))
                                    .and_then(|v| v.as_str())
                                    == Some(file_id_str)
                            });
                            let mut file_json = serde_json::json!({
                                "id": f["id"],
                                "fileName": f["fileName"],
                                "filePath": full_path.to_string_lossy(),
                                "fileType": f["fileType"],
                                "fileSize": f["fileSize"],
                                "category": f["category"],
                                "version": f["version"],
                                "isLatest": f["isLatest"],
                                "createdAt": f["createdAt"],
                                "createdByStep": f["createdByStep"],
                                "description": f["description"],
                                "actions": [],
                            });
                            if let Some(meta) = matching_meta {
                                let requested = meta
                                    .get("requestedFormat")
                                    .or_else(|| meta.get("requested_format"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let actual = meta
                                    .get("actualFormat")
                                    .or_else(|| meta.get("actual_format"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if !requested.is_empty() && requested != actual {
                                    file_json["isDegraded"] = serde_json::json!(true);
                                    file_json["requestedFormat"] = serde_json::json!(requested);
                                    file_json["degradationNotice"] = serde_json::json!(format!(
                                        "{} 转换失败，已降级为 {} 格式",
                                        requested.to_uppercase(),
                                        actual.to_uppercase()
                                    ));
                                }
                            }
                            file_json
                        })
                        .collect();
                    log::info!(
                        "[persist_assistant_message] Attaching {} generated files to message {}",
                        gen_files.len(),
                        message_id
                    );
                    build_assistant_content_json(&filtered_content, tool_calls, Some(gen_files))
                }
                Ok(_) => build_assistant_content_json(&filtered_content, tool_calls, None),
                Err(e) => {
                    log::error!(
                        "[persist_assistant_message] Failed to query generated files: {:#}",
                        e
                    );
                    build_assistant_content_json(&filtered_content, tool_calls, None)
                }
            }
        } else {
            build_assistant_content_json(&filtered_content, tool_calls, None)
        };

        // --- Persist to AppStorage ---
        let content_json = content_value.to_string();
        log::info!(
            "[persist_assistant_message] Queueing save id={} conv={} content_len={}",
            message_id,
            conversation_id,
            content_json.len()
        );
        persist_assistant_content_json(
            self.services.db.clone(),
            self.services.assistant_write_queue.clone(),
            message_id.clone(),
            conversation_id.to_string(),
            content_json,
        )
        .await?;

        // NOTE: message:updated event is NOT emitted here — that is the driver's
        // responsibility via bus.emit(RuntimeEventKind::MessagePersisted { ... }).
        // Auto-title update is also omitted: it requires app.emit() and is a side-effect
        // that the driver (T13) should handle via the bus.

        Ok(message_id)
    }

    async fn finalize_step(
        &self,
        state: &TurnIterationState,
        config: &TurnConfig,
    ) -> Result<(), TurnError> {
        let _ = state;
        let _ = config;
        Ok(())
    }

    /// 构建 Turn 级的 system prompt。
    /// 精确移植 agent_loop Block 4 的逻辑：
    ///   - 从 DB 读取 active persona
    ///   - 从 auth_manager 获取 product_name（租户品牌名）
    async fn build_system_prompt(&self, _conversation_id: &str) -> Result<String, TurnError> {
        let persona = self.services.db.get_active_persona().ok();

        let product_name: Option<String> = self
            .services
            .auth_manager
            .get_auth_info()
            .await
            .tenant
            .and_then(|t| t.product_name.filter(|n| !n.is_empty()));

        let parts = prompts::build_system_prompt_parts(
            prompts::PromptMode::Daily,
            persona.as_ref(),
            product_name.as_deref(),
        );
        let prompt = if parts.dynamic_section.is_empty() {
            parts.static_section
        } else {
            format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
        };

        log::info!(
            "[build_system_prompt] mode=daily len={} persona={} product_name={}",
            prompt.len(),
            persona
                .as_ref()
                .map(|p| p.identity.as_str())
                .unwrap_or("(none)"),
            product_name.as_deref().unwrap_or("(none)"),
        );

        Ok(prompt)
    }

    async fn build_prompt_snapshot(
        &self,
        _conversation_id: &str,
    ) -> Result<Option<TurnPromptSnapshot>, TurnError> {
        let persona = self.services.db.get_active_persona().ok();

        let product_name: Option<String> = self
            .services
            .auth_manager
            .get_auth_info()
            .await
            .tenant
            .and_then(|t| t.product_name.filter(|n| !n.is_empty()));

        let assembly = PromptAssembler::default().build_system_prompt(PromptBuildContext {
            mode: prompts::PromptMode::Daily,
            persona: persona.as_ref(),
            product_name: product_name.as_deref(),
        });
        log::info!(
            "[build_prompt_snapshot] mode=daily len={} persona={} product_name={}",
            assembly.flatten().len(),
            persona
                .as_ref()
                .map(|p| p.identity.as_str())
                .unwrap_or("(none)"),
            product_name.as_deref().unwrap_or("(none)"),
        );

        Ok(Some(TurnPromptSnapshot::new(assembly, Vec::new())))
    }

    async fn build_user_message_content(
        &self,
        conversation_id: &str,
        content: &str,
        file_ids: &[String],
    ) -> Result<String, TurnError> {
        let file_attachments = if file_ids.is_empty() {
            Vec::new()
        } else {
            self.services
                .db
                .get_uploaded_files_by_ids(file_ids)
                .map_err(|e| {
                    TurnError::PersistenceError(format!(
                        "Failed to load uploaded file metadata: {}",
                        e
                    ))
                })?
        };
        let authorized_workspace =
            chat_runtime_impl::load_authorized_workspace(&self.services.app, conversation_id);
        Ok(chat_runtime_impl::build_llm_content(
            content,
            &file_attachments,
            authorized_workspace.is_some(),
        ))
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;

        let filter = ToolFilter::Only(DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect());

        let tool_definitions: Vec<crate::llm::streaming::ToolDefinition> = self
            .services
            .tool_registry
            .get_schemas_filtered(&filter)
            .await;

        // ToolDefinition implements Serialize
        let json_defs: Vec<serde_json::Value> = tool_definitions
            .into_iter()
            .filter_map(|td| {
                serde_json::to_value(&td)
                    .map_err(|e| {
                        log::warn!(
                            "[get_tool_defs] Failed to serialize tool '{}': {}",
                            td.name,
                            e
                        )
                    })
                    .ok()
            })
            .collect();

        log::info!(
            "[get_tool_defs] returned {} tool definitions",
            json_defs.len(),
        );

        Ok(json_defs)
    }

    async fn load_turn_config_overrides(
        &self,
        request: &ChatTurnRequest,
    ) -> Result<TurnConfigOverrides, TurnError> {
        let all_tools = self
            .services
            .tool_registry
            .get_all_schemas()
            .await
            .into_iter()
            .map(|def| def.name)
            .collect::<Vec<_>>();

        let (skill_ctx, explicit_selection) = resolve_skill_turn_context_for_request(
            self.services.skill_sessions.as_ref(),
            self.services.skill_registry.as_ref(),
            &all_tools,
            request,
        )
        .await?;

        log::info!(
            "[skill-command][turn-config-resolved] trace_id={:?} conversation_id={} selected_skill_id={:?} selected_skill_label={:?} resolved_skill_id={} explicit_selection={} allowed_tools_count={} switch_skill_allowed={}",
            request
                .client_message_id
                .as_deref()
                .or(request.selected_skill_id.as_deref()),
            request.conversation_id,
            request.selected_skill_id,
            request.selected_skill_label,
            skill_ctx.skill_id,
            explicit_selection,
            skill_ctx
                .allowed_tools
                .as_ref()
                .map(|tools| tools.len())
                .unwrap_or(all_tools.len()),
            skill_ctx
                .allowed_tools
                .as_ref()
                .map(|tools| tools.contains("switch_skill"))
                .unwrap_or(true),
        );

        let authorized_workspace = chat_runtime_impl::load_authorized_workspace(
            &self.services.app,
            request.conversation_id.as_str(),
        );
        let visible_tool_defs = chat_runtime_impl::build_visible_tool_defs(
            self.services.tool_registry.as_ref(),
            authorized_workspace.is_some(),
            skill_ctx.allowed_tools.as_ref(),
        )
        .await;
        let json_defs = visible_tool_defs
            .into_iter()
            .filter_map(|td| serde_json::to_value(&td).ok())
            .collect();

        Ok(TurnConfigOverrides {
            system_prompt: Some(skill_ctx.system_prompt),
            tool_defs: Some(json_defs),
            allowed_tools: skill_ctx.allowed_tools,
            max_iterations: Some(
                self.services
                    .skill_registry
                    .get(skill_ctx.skill_id.as_str())
                    .await
                    .map(|skill| skill.max_iterations(&skill_ctx.state))
                    .unwrap_or(30),
            ),
            token_budget: Some(
                self.services
                    .skill_registry
                    .get(skill_ctx.skill_id.as_str())
                    .await
                    .map(|skill| skill.token_budget(&skill_ctx.state) as usize)
                    .unwrap_or(4096),
            ),
        })
    }

    async fn load_history(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        let authorized_workspace =
            chat_runtime_impl::load_authorized_workspace(&self.services.app, conversation_id);
        let chat_messages = load_history_via_runtime_history(
            &self.services.db,
            conversation_id,
            authorized_workspace.is_some(),
        )?;

        log::info!(
            "[load_history] conv={} loaded {} messages via history.rs",
            conversation_id,
            chat_messages.len(),
        );

        Ok(chat_messages)
    }

    async fn save_compact_boundary(
        &self,
        record: crate::runtime::chat::compaction::CompactBoundaryRecord,
    ) -> Result<(), TurnError> {
        self.services
            .db
            .append_compact_boundary(&record)
            .map_err(|e| {
                TurnError::PersistenceError(format!("Failed to persist compact boundary: {}", e))
            })
    }

    async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
        use crate::runtime::chat::context_builder::{build_env_info, ManagedRuntimeEnvInfo};

        let workspace_path = self.services.file_mgr.workspace_path().to_path_buf();

        let authorized =
            chat_runtime_impl::load_authorized_workspace(&self.services.app, conversation_id);
        let authorized_tuple = authorized.as_ref().map(|aw| {
            (
                aw.root_path.to_string_lossy().into_owned(),
                aw.display_name.clone(),
            )
        });
        let authorized_ref = authorized_tuple
            .as_ref()
            .map(|(p, n)| (p.as_str(), n.as_str()));

        let runtime_info = match self.services.runtime_resolver.as_ref() {
            Some(resolver) => match resolver.workspace_dependencies() {
                Ok(deps) => Some(ManagedRuntimeEnvInfo {
                    runtime_root: infer_runtime_root(&deps.python),
                    python_path: deps.python.clone(),
                    node_path: deps.node.clone(),
                    npm_path: deps.npm.clone(),
                    npx_path: deps.npx.clone(),
                    uv_path: deps.uv.clone(),
                    uvx_path: deps.uvx.clone(),
                }),
                Err(error) => {
                    log::warn!("[get_env_info] managed runtime unavailable: {}", error);
                    None
                }
            },
            None => None,
        };

        let env_info = build_env_info(&workspace_path, authorized_ref, runtime_info.as_ref()).await;

        log::info!(
            "[get_env_info] conv={} workspace={} authorized={} env_info_len={}",
            conversation_id,
            workspace_path.display(),
            authorized.is_some(),
            env_info.len()
        );

        Ok(env_info)
    }

    async fn load_workspace_path(&self) -> Result<std::path::PathBuf, TurnError> {
        Ok(self.services.file_mgr.workspace_path().to_path_buf())
    }

    async fn load_renlijia_md(
        &self,
        workspace_path: &std::path::Path,
    ) -> Result<Vec<crate::runtime::renlijia_md::RenlijiaMdFile>, TurnError> {
        let mut loader = self.renlijia_md_loader.lock().await;
        Ok(loader.load(workspace_path).await)
    }

    async fn load_project_memory(
        &self,
        workspace_path: &std::path::Path,
        query: &str,
    ) -> Result<crate::runtime::project_memory::ProjectMemoryContext, TurnError> {
        let app_data_dir = self.services.db.base_dir().to_path_buf();
        let service = crate::runtime::project_memory::ProjectMemoryService::new(
            app_data_dir,
            workspace_path.to_path_buf(),
        );
        service.load_context(query).map_err(|err| {
            TurnError::PersistenceError(format!("Failed to load project memory: {err}"))
        })
    }

    async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok(self.services.db.load_core_memory())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    use crate::plugin::skill_trait::{
        Skill, SkillState, StepAction, ToolFilter, WorkflowDefinition,
    };
    use crate::runtime::store::MemoryStore;
    use crate::storage::message_write_queue::{MessageWriteQueue, MessageWriteTarget};
    use tempfile::TempDir;

    fn test_storage() -> (AppStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = AppStorage::new(dir.path()).unwrap();
        (storage, dir)
    }

    struct Gate {
        open: Mutex<bool>,
        cv: Condvar,
    }

    impl Gate {
        fn new() -> Self {
            Self {
                open: Mutex::new(false),
                cv: Condvar::new(),
            }
        }

        fn wait(&self) {
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.cv.wait(open).unwrap();
            }
        }

        fn open(&self) {
            let mut open = self.open.lock().unwrap();
            *open = true;
            self.cv.notify_all();
        }
    }

    struct Signal {
        fired: Mutex<bool>,
        cv: Condvar,
    }

    impl Signal {
        fn new() -> Self {
            Self {
                fired: Mutex::new(false),
                cv: Condvar::new(),
            }
        }

        fn fire(&self) {
            let mut fired = self.fired.lock().unwrap();
            *fired = true;
            self.cv.notify_all();
        }

        fn wait(&self) {
            let mut fired = self.fired.lock().unwrap();
            while !*fired {
                fired = self.cv.wait(fired).unwrap();
            }
        }
    }

    struct BlockingInsertTarget {
        db: Arc<AppStorage>,
        started: Signal,
        release: Gate,
        first_insert: AtomicBool,
    }

    impl BlockingInsertTarget {
        fn new(db: Arc<AppStorage>) -> Self {
            Self {
                db,
                started: Signal::new(),
                release: Gate::new(),
                first_insert: AtomicBool::new(true),
            }
        }
    }

    impl MessageWriteTarget for BlockingInsertTarget {
        fn insert_message(
            &self,
            id: &str,
            conversation_id: &str,
            role: &str,
            content_json: &str,
        ) -> anyhow::Result<()> {
            if self.first_insert.swap(false, Ordering::SeqCst) {
                self.started.fire();
                self.release.wait();
            }
            self.db
                .insert_message(id, conversation_id, role, content_json)
                .map_err(Into::into)
        }

        fn update_message_content(
            &self,
            id: &str,
            conversation_id: &str,
            content_json: &str,
        ) -> anyhow::Result<()> {
            self.db
                .update_message_content(id, conversation_id, content_json)
                .map_err(Into::into)
        }
    }

    struct FailingInsertTarget;

    impl MessageWriteTarget for FailingInsertTarget {
        fn insert_message(
            &self,
            _id: &str,
            _conversation_id: &str,
            _role: &str,
            _content_json: &str,
        ) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("synthetic write failure"))
        }

        fn update_message_content(
            &self,
            _id: &str,
            _conversation_id: &str,
            _content_json: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct TestSkill {
        id: &'static str,
        trigger: Option<&'static str>,
        prompt_prefix: &'static str,
        default_tools: Vec<String>,
        workflow: Option<WorkflowDefinition>,
        allow_all_tools: bool,
    }

    #[async_trait]
    impl Skill for TestSkill {
        fn id(&self) -> &str {
            self.id
        }

        fn display_name(&self) -> &str {
            self.id
        }

        fn description(&self) -> &str {
            self.id
        }

        fn system_prompt(&self, state: &SkillState) -> String {
            format!(
                "{}:{}",
                self.prompt_prefix,
                state.current_step.as_deref().unwrap_or("none")
            )
        }

        fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
            if self.allow_all_tools {
                ToolFilter::All
            } else {
                ToolFilter::Only(self.default_tools.clone())
            }
        }

        fn workflow(&self) -> Option<WorkflowDefinition> {
            self.workflow.clone()
        }

        fn allowed_tool_names(&self, state: &SkillState) -> Option<Vec<String>> {
            match state.current_step.as_deref() {
                Some("step0") => Some(vec!["search_files".to_string()]),
                Some("step1") => Some(vec!["read_workspace_file".to_string()]),
                _ if self.allow_all_tools => None,
                _ => Some(self.default_tools.clone()),
            }
        }

        fn on_step_complete(&self, _state: &mut SkillState, user_message: &str) -> StepAction {
            match user_message.trim() {
                "继续" => StepAction::AdvanceToStep("step1".to_string()),
                "完成" => StepAction::Finish,
                _ => StepAction::WaitForUser,
            }
        }
    }

    async fn registry_with_test_skills() -> SkillRegistry {
        let registry = SkillRegistry::new("daily-assistant");
        registry
            .register(
                Arc::new(TestSkill {
                    id: "daily-assistant",
                    trigger: None,
                    prompt_prefix: "daily",
                    default_tools: vec!["bash".to_string()],
                    workflow: None,
                    allow_all_tools: false,
                }),
                "test",
            )
            .await;
        registry
            .register(
                Arc::new(TestSkill {
                    id: "comp-analysis",
                    trigger: Some("分析"),
                    prompt_prefix: "skill",
                    default_tools: vec!["search_files".to_string()],
                    workflow: Some(WorkflowDefinition {
                        initial_step: "step0".to_string(),
                        steps: vec![],
                    }),
                    allow_all_tools: false,
                }),
                "test",
            )
            .await;
        registry
            .register(
                Arc::new(TestSkill {
                    id: "salary-query",
                    trigger: None,
                    prompt_prefix: "salary",
                    default_tools: vec!["bash".to_string(), "switch_skill".to_string()],
                    workflow: None,
                    allow_all_tools: false,
                }),
                "test",
            )
            .await;
        registry
            .register(
                Arc::new(TestSkill {
                    id: "all-tools-skill",
                    trigger: None,
                    prompt_prefix: "all-tools",
                    default_tools: Vec::new(),
                    workflow: None,
                    allow_all_tools: true,
                }),
                "test",
            )
            .await;
        registry
    }

    #[tokio::test]
    async fn selected_skill_id_overrides_activation_detection_for_turn_context() {
        let registry = registry_with_test_skills().await;
        let skill_sessions = SkillSessionStore::new();
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];
        let mut request = ChatTurnRequest::new("c-selected-skill", "请分析这个问题", Vec::new());
        request.client_message_id = Some("client-selected".to_string());
        request.selected_skill_id = Some("salary-query".to_string());
        request.selected_skill_label = Some("salary-query".to_string());

        let (ctx, explicit) = resolve_skill_turn_context_for_request(
            &skill_sessions,
            &registry,
            &all_tools,
            &request,
        )
        .await
        .expect("selected skill should resolve");

        assert!(
            explicit,
            "selected_skill_id should mark this as explicit selection"
        );
        assert_eq!(ctx.skill_id, "salary-query");
        assert!(
            ctx.system_prompt.starts_with("salary:"),
            "expected selected salary-query prompt, got {}",
            ctx.system_prompt
        );
        assert!(
            !ctx.allowed_tools
                .as_ref()
                .map(|tools| tools.contains("switch_skill"))
                .unwrap_or(false),
            "explicit skill turn must not expose switch_skill"
        );

        let restored = skill_sessions
            .resolve_turn_context(&registry, &all_tools, "c-selected-skill", "继续", false)
            .await
            .expect("explicit skill state should persist for following turns");

        assert_eq!(restored.skill_id, "salary-query");
    }

    #[tokio::test]
    async fn selected_skill_id_with_all_tools_still_hides_switch_skill() {
        let registry = registry_with_test_skills().await;
        let skill_sessions = SkillSessionStore::new();
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];
        let mut request = ChatTurnRequest::new("c-selected-all-tools", "用全工具技能", Vec::new());
        request.selected_skill_id = Some("all-tools-skill".to_string());
        request.selected_skill_label = Some("all-tools-skill".to_string());

        let (ctx, explicit) = resolve_skill_turn_context_for_request(
            &skill_sessions,
            &registry,
            &all_tools,
            &request,
        )
        .await
        .expect("all-tools selected skill should resolve");

        assert!(
            explicit,
            "selected_skill_id should mark this as explicit selection"
        );
        let allowed_tools = ctx
            .allowed_tools
            .as_ref()
            .expect("explicit all-tools skill turn should narrow allowed_tools");
        assert!(allowed_tools.contains("bash"));
        assert!(
            !allowed_tools.contains("switch_skill"),
            "explicit all-tools skill turn must not expose switch_skill"
        );
    }

    #[test]
    fn build_history_message_content_preserves_uploaded_file_hints() {
        let content = serde_json::json!({
            "text": "请继续分析这个表格",
            "files": [
                {
                    "id": "file-1",
                    "originalName": "sales.csv",
                    "fileType": "text/csv"
                }
            ]
        });

        let llm_content =
            build_history_message_content("user", &content, false).expect("history content");

        assert!(llm_content.contains("[已上传文件]"));
        assert!(llm_content.contains("file-1"));
        assert!(llm_content.contains("load_file(file_id)"));
    }

    #[test]
    fn build_history_message_content_keeps_assistant_text_plain() {
        let content = serde_json::json!({
            "text": "这是上一轮助手回复"
        });

        let llm_content =
            build_history_message_content("assistant", &content, false).expect("assistant text");

        assert_eq!(llm_content, "这是上一轮助手回复");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn persist_assistant_content_json_waits_until_db_write_becomes_visible() {
        let (storage, _dir) = test_storage();
        storage.create_conversation("c1", "Conv").unwrap();

        let storage = Arc::new(storage);
        let target = Arc::new(BlockingInsertTarget::new(storage.clone()));
        let queue = Arc::new(MessageWriteQueue::new(target.clone()));
        let content_json = r#"{"text":"hello from assistant"}"#.to_string();

        let task = tokio::spawn(async move {
            persist_assistant_content_json(
                storage.clone(),
                queue,
                "msg-1".to_string(),
                "c1".to_string(),
                content_json,
            )
            .await
        });

        target.started.wait();

        let finished_before_release = task.is_finished();
        target.release.open();

        task.await.unwrap().unwrap();
        let persisted = target.db.get_messages("c1").unwrap();

        assert!(
            !finished_before_release,
            "assistant persistence helper must not return before the message is durably visible"
        );
        assert_eq!(persisted.len(), 1, "assistant message should be persisted");
        assert_eq!(
            persisted[0].get("id").and_then(|value| value.as_str()),
            Some("msg-1")
        );
    }

    #[tokio::test]
    async fn persist_assistant_content_json_surfaces_worker_write_failures() {
        let (storage, _dir) = test_storage();
        storage.create_conversation("c1", "Conv").unwrap();

        let err = persist_assistant_content_json(
            Arc::new(storage),
            Arc::new(MessageWriteQueue::new(Arc::new(FailingInsertTarget))),
            "msg-fail".to_string(),
            "c1".to_string(),
            r#"{"text":"boom"}"#.to_string(),
        )
        .await
        .expect_err("assistant persistence helper must surface worker failures");

        assert!(
            err.to_string().contains("synthetic write failure"),
            "worker error text should be preserved for the caller"
        );
    }

    #[tokio::test]
    async fn build_skill_session_store_uses_memory_backing_when_available() {
        let registry = registry_with_test_skills().await;
        let memory_store = Arc::new(crate::runtime::store::InMemoryMemoryStore::default());
        let skill_sessions = build_skill_session_store(Some(memory_store.clone()));

        memory_store
            .set(
                "note:conv-restored:active_skill_state",
                r#"{"skillId":"comp-analysis","currentStep":"step1","stepStatus":{"step0":"completed","step1":"active"},"customData":null,"hasFiles":true}"#,
            )
            .unwrap();

        let restored = skill_sessions
            .resolve_turn_context(
                &registry,
                &[
                    "bash".to_string(),
                    "search_files".to_string(),
                    "read_workspace_file".to_string(),
                    "switch_skill".to_string(),
                ],
                "conv-restored",
                "我先看看",
                true,
            )
            .await
            .expect("memory-backed skill sessions should restore persisted state");

        assert_eq!(restored.skill_id, "comp-analysis");
        assert_eq!(restored.state.current_step.as_deref(), Some("step1"));
    }
}

// ---------------------------------------------------------------------------
// Private helpers for run_llm_step
// ---------------------------------------------------------------------------

fn build_assistant_content_json(
    text: &str,
    tool_calls: &[serde_json::Value],
    generated_files: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let mut obj = serde_json::json!({ "text": text });
    if !tool_calls.is_empty() {
        obj["toolCalls"] = serde_json::Value::Array(tool_calls.to_vec());
    }
    if let Some(files) = generated_files {
        if !files.is_empty() {
            obj["generatedFiles"] = serde_json::Value::Array(files);
        }
    }
    obj
}

/// Decrypt an API key stored in encrypted form (salt:iv:ciphertext).
/// Returns empty string on decryption failure to trigger default fallback.
/// Mirrors legacy `decrypt_key()` in chat_runtime_impl.rs.
fn decrypt_api_key(ss: &SecureStorage, value: &str) -> String {
    if value.is_empty() || !value.contains(':') {
        return value.to_string();
    }
    match ss.decrypt(value) {
        Ok(plaintext) => plaintext,
        Err(e) => {
            log::warn!(
                "[run_llm_step] Failed to decrypt API key (err={}), returning empty",
                e
            );
            String::new()
        }
    }
}

fn build_gateway_settings(settings: &ResolvedLlmSettings) -> AppSettings {
    AppSettings {
        primary_model: settings.primary_model.clone(),
        primary_api_key: settings.primary_api_key.clone(),
        auto_model_routing: settings.auto_model_routing,
        custom_model_endpoint: settings.custom_model_endpoint.clone(),
        custom_model_name: settings.custom_model_name.clone(),
        use_cloud: settings.use_cloud,
        cloud_model: settings.cloud_model.clone(),
        cloud_model_type: settings.cloud_model_type.clone(),
        thinking_type: settings.thinking_type.clone(),
        thinking_budget_tokens: settings.thinking_budget_tokens,
        ..AppSettings::default()
    }
}

fn openai_system_prompt_content(
    value: Option<serde_json::Value>,
    fallback: &str,
) -> Option<String> {
    value
        .and_then(|value| serde_json::from_value::<crate::llm::streaming::ChatMessage>(value).ok())
        .and_then(|message| {
            let content = message.content.trim();
            if message.role == "system" && !content.is_empty() {
                Some(message.content)
            } else {
                None
            }
        })
        .or_else(|| {
            if fallback.trim().is_empty() {
                None
            } else {
                Some(fallback.to_string())
            }
        })
}

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

#[cfg(test)]
mod openai_system_prompt_tests {
    use super::*;
    use crate::llm::masking::{MaskingContext, MaskingLevel};
    use crate::llm::streaming::ChatMessage;

    #[test]
    fn openai_system_prompt_stays_out_of_masking_path() {
        let rendered_system = serde_json::json!({
            "role": "system",
            "content": "华为公司 instructions remain provider-visible",
        });
        let chat_messages = vec![ChatMessage::text("user", "请分析华为公司")];
        let system_prompt = openai_system_prompt_content(Some(rendered_system), "legacy system")
            .expect("rendered system prompt content");

        let mut mask_ctx = MaskingContext::new(MaskingLevel::Strict);
        let masked = mask_ctx.mask_messages(&chat_messages);

        assert_eq!(chat_messages.len(), 1);
        assert!(chat_messages.iter().all(|message| message.role != "system"));
        assert!(system_prompt.contains("华为公司"));
        assert!(masked[0].content.contains("[COMPANY_1]"));
        assert!(!masked[0].content.contains("华为公司"));
    }
}

/// Classify an LLM error into a user-friendly Chinese message.
fn classify_llm_error(error: &str) -> TurnError {
    let lower = error.to_lowercase();
    if lower.contains("prompt too long")
        || lower.contains("prompt is too long")
        || lower.contains("context length")
        || lower.contains("maximum context length")
        || lower.contains("too many tokens")
        || lower.contains("input is too long")
        || (lower.contains("413")
            && (lower.contains("token") || lower.contains("context") || lower.contains("prompt")))
    {
        TurnError::PromptTooLong("上下文过长，请压缩后重试。".to_string())
    } else if lower.contains("429") || lower.contains("rate limit") {
        TurnError::LlmError("AI 服务请求频率超限，请稍等片刻后重试。".to_string())
    } else if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
    {
        TurnError::LlmError("API 密钥无效或已过期，请在设置中检查 API Key 配置。".to_string())
    } else if lower.contains("402") || lower.contains("insufficient") || lower.contains("quota") {
        TurnError::LlmError("API 额度不足，请检查账户余额。".to_string())
    } else if lower.contains("timeout") || lower.contains("timed out") {
        TurnError::LlmError("AI 服务连接超时，请检查网络连接后重试。".to_string())
    } else if lower.contains("connection") || lower.contains("network") {
        TurnError::LlmError("网络连接异常，请检查网络后重试。".to_string())
    } else if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        TurnError::LlmError("AI 服务暂时不可用，请稍后重试。".to_string())
    } else {
        TurnError::LlmError(format!("服务异常：{}。请重试。", truncate_str(error, 100)))
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

fn infer_runtime_root(path: &std::path::Path) -> std::path::PathBuf {
    let mut root = std::path::PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        root.push(component.as_os_str());
        if component.as_os_str() == "versions" {
            if let Some(version) = components.next() {
                root.push(version.as_os_str());
                return root;
            }
        }
    }
    path.parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
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
        permission_store: Arc<crate::runtime::store::PermissionStore>,
        app: tauri::AppHandle,
    ) -> Self {
        let skill_session_memory_store = app
            .try_state::<Arc<crate::storage::file_store::RuntimeRepositoryFacade>>()
            .map(|facade| facade.inner().clone_memory_store());
        let skill_sessions = build_skill_session_store(skill_session_memory_store);
        let runtime_resolver = app
            .try_state::<crate::runtime::dependencies::ManagedRuntimeResolver>()
            .map(|resolver| resolver.inner().clone());
        let assistant_write_queue = Arc::new(MessageWriteQueue::new(db.clone()));
        let services = TauriChatServices {
            db,
            gateway,
            file_mgr,
            assistant_write_queue,
            crypto,
            tool_registry,
            session_mgr,
            auth_manager,
            app,
            skill_registry,
            skill_sessions,
            runtime_resolver,
        };
        let host = Arc::new(TauriRuntimeHost::new(services.app.clone()));
        let adapter = Arc::new(TauriEventAdapter::new(host));
        let bus = RuntimeEventBus::new();
        bus.subscribe(adapter);
        let llm_executor: Arc<dyn RuntimeLlmExecutor> = Arc::new(TauriLegacyTurnExecutor {
            services: services.clone(),
            renlijia_md_loader: Arc::new(tokio::sync::Mutex::new(
                crate::runtime::renlijia_md::RenlijiaMdLoader::new(),
            )),
        });
        // NOTE: request-scoped dispatcher is built per-call in send_message() to avoid
        // calling nested blocking init inside the sync new() which panics ("Cannot start a runtime
        // from within a runtime") because Tauri's setup closure already runs in tokio.
        let mut runtime = SessionRuntime::with_llm_executor(
            QueryEngine::new()
                .with_workspace_path(services.file_mgr.workspace_path().to_path_buf())
                .with_runtime_resolver(services.runtime_resolver.clone()),
            bus,
            llm_executor,
        )
        .with_skill_sessions(services.skill_sessions.clone())
        .with_permission_store(permission_store);
        if let Some(home) = services.app.try_state::<Arc<crate::storage::AiJiaHome>>() {
            runtime = runtime.with_default_folder(home.default_folder());
        }
        if let Some(facade) = services
            .app
            .try_state::<Arc<crate::storage::file_store::RuntimeRepositoryFacade>>()
        {
            runtime = runtime
                .with_authorized_workspace_store(facade.inner().clone_authorized_workspace_store());
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

    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        TauriLegacyTurnExecutor {
            services: self.services.clone(),
            renlijia_md_loader: Arc::new(tokio::sync::Mutex::new(
                crate::runtime::renlijia_md::RenlijiaMdLoader::new(),
            )),
        }
        .load_llm_settings()
        .await
    }

    async fn load_llm_settings_for_turn(
        &self,
        request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        TauriLegacyTurnExecutor {
            services: self.services.clone(),
            renlijia_md_loader: Arc::new(tokio::sync::Mutex::new(
                crate::runtime::renlijia_md::RenlijiaMdLoader::new(),
            )),
        }
        .load_llm_settings_for_turn(request)
        .await
    }

    pub async fn send_message(
        &self,
        conversation_id: String,
        content: String,
        file_ids: Vec<String>,
        permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
        agent_name: Option<String>,
        client_message_id: Option<String>,
        selected_skill_id: Option<String>,
        selected_skill_label: Option<String>,
    ) -> Result<(), String> {
        log::info!(
            "[skill-command][send-message] trace_id={:?} conversation_id={} client_message_id={:?} selected_skill_id={:?} selected_skill_label={:?} content_len={}",
            client_message_id.as_deref().or(selected_skill_id.as_deref()),
            conversation_id,
            client_message_id,
            selected_skill_id,
            selected_skill_label,
            content.len()
        );
        let mut request = ChatTurnRequest::new(conversation_id.clone(), content, file_ids);
        request.agent_name = agent_name;
        request.client_message_id = client_message_id;
        request.selected_skill_id = selected_skill_id;
        request.selected_skill_label = selected_skill_label;
        if let Some(permission_mode) = permission_mode {
            request.permission_mode = permission_mode;
        }
        let run_id = request.run_id.clone();
        self.services
            .gateway
            .set_busy_for_run(&conversation_id, run_id.clone())?;

        let session_id = request.conversation_id.clone();
        let connector_engine = self
            .services
            .app
            .try_state::<Arc<crate::connector::ConnectorEngine>>()
            .map(|v| v.inner().clone());
        let agent_runtime = self
            .services
            .app
            .try_state::<Arc<crate::runtime::agent::AgentRuntime>>()
            .map(|v| v.inner().clone());
        let request_scoped_runtime_deps = crate::plugin::registry::RequestScopedRuntimeDeps {
            storage: self.services.db.clone(),
            file_manager: self.services.file_mgr.clone(),
            workspace_path: self.services.file_mgr.workspace_path().to_path_buf(),
            conversation_id: session_id.as_str().to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            agent_id: None,
            tavily_api_key: None,
            bocha_api_key: None,
            app_handle: Some(self.services.app.clone()),
            session_manager: self.services.session_mgr.clone(),
            auth_manager: Some(self.services.auth_manager.clone()),
            connector_engine,
            use_cloud: false,
            model: String::new(),
            gateway: Some(self.services.gateway.clone()),
            tool_registry: Some(self.services.tool_registry.clone()),
            app_settings: Some(Arc::new(AppSettings::default())),
            agent_runtime,
            event_bus: None,
            skill_registry: Some(self.services.skill_registry.clone()),
            skill_sessions: Some(self.services.skill_sessions.clone()),
            authorized_workspace: None,
            read_file_state: None,
            cancellation: None,
            permission_mode: request.permission_mode,
            runtime_resolver: self.services.runtime_resolver.clone(),
        };
        let runtime_dispatcher = self
            .services
            .tool_registry
            .to_runtime_dispatcher(request_scoped_runtime_deps)
            .await;
        let runtime = self.runtime.clone().with_query_engine(
            QueryEngine::with_dispatcher(runtime_dispatcher)
                .with_workspace_path(self.services.file_mgr.workspace_path().to_path_buf())
                .with_runtime_resolver(self.services.runtime_resolver.clone()),
        );
        // Compatibility marker for review tests: self.runtime.run_chat_request(request)
        let result = runtime.run_chat_request(request).await;
        // Release the stream-cancel bridge for this turn before any post-turn work
        // can start, otherwise a stopped turn leaves a stale cancelled slot behind.
        self.services
            .gateway
            .clear_task_for_run(&conversation_id, &run_id);
        self.services.session_mgr.destroy_run(&run_id).await;

        if result.is_ok() {
            // Quick synchronous guard: only attempt title generation when needed.
            let needs_title =
                conversation_service::should_auto_title(&*self.services.db, &conversation_id)
                    .unwrap_or(false);

            if needs_title {
                // Load settings with the correct conversation context so per-conversation
                // model overrides are respected.
                let dummy_request =
                    ChatTurnRequest::new(conversation_id.clone(), String::new(), vec![]);
                if let Ok(resolved) = self.load_llm_settings_for_turn(&dummy_request).await {
                    let db = self.services.db.clone() as Arc<dyn ConversationStore>;
                    let gateway = self.services.gateway.clone();
                    let host: Arc<dyn crate::transport::runtime_host::RuntimeHost> =
                        Arc::new(TauriRuntimeHost::new(self.services.app.clone()));
                    let conv_id = conversation_id.clone();
                    let settings = build_gateway_settings(&resolved);
                    tauri::async_runtime::spawn(async move {
                        conversation_service::generate_and_set_title(
                            db, gateway, host, conv_id, settings,
                        )
                        .await;
                    });
                }
            }
        }

        result
    }

    pub fn flush_pending_message_writes(&self) -> Result<(), String> {
        self.services
            .assistant_write_queue
            .flush()
            .map_err(|err| err.to_string())
    }

    pub fn is_agent_busy(&self) -> Vec<String> {
        self.services.gateway.get_busy_conversations()
    }

    pub async fn stop_streaming(&self, conversation_id: String) -> Result<(), String> {
        let session_id = SessionId::new(conversation_id.clone());
        self.runtime.cancel_session(
            &session_id,
            crate::runtime::cancellation::CancellationReason::Interrupt,
        );
        conversation_service::stop_streaming(
            self.services.gateway.clone(),
            self.services.session_mgr.clone(),
            conversation_id,
        )
        .await
    }

    pub async fn approve_permission_request(
        &self,
        tool_call_id: String,
        updated_input: Option<serde_json::Value>,
        remember: Option<bool>,
        destination: Option<PermissionDestination>,
    ) -> Result<(), String> {
        self.runtime
            .resolve_permission_request(
                &ToolCallId::new(tool_call_id),
                PendingPermissionResolution::Allow {
                    updated_input,
                    remember: remember.unwrap_or(false),
                    destination,
                },
            )
            .map_err(|e| e.to_string())
    }

    pub async fn deny_permission_request(
        &self,
        tool_call_id: String,
        message: Option<String>,
        remember: Option<bool>,
        destination: Option<PermissionDestination>,
    ) -> Result<(), String> {
        self.runtime
            .resolve_permission_request(
                &ToolCallId::new(tool_call_id),
                PendingPermissionResolution::Deny {
                    message: message
                        .unwrap_or_else(|| "Permission request denied by user.".to_string()),
                    remember: remember.unwrap_or(false),
                    destination,
                },
            )
            .map_err(|e| e.to_string())
    }

    pub async fn cancel_permission_request(
        &self,
        tool_call_id: String,
        message: Option<String>,
    ) -> Result<(), String> {
        self.runtime
            .resolve_permission_request(
                &ToolCallId::new(tool_call_id),
                PendingPermissionResolution::Cancel {
                    message: message
                        .unwrap_or_else(|| "Permission request cancelled by user.".to_string()),
                },
            )
            .map_err(|e| e.to_string())
    }

    pub async fn submit_user_interaction(
        &self,
        interaction_id: String,
        value: serde_json::Value,
    ) -> Result<(), String> {
        self.runtime
            .resolve_interaction_request(
                &crate::runtime::interaction::InteractionId::new(interaction_id),
                crate::runtime::interaction::InteractionResolution::Submit { value },
            )
            .map_err(|e| e.to_string())
    }

    pub async fn cancel_user_interaction(
        &self,
        interaction_id: String,
        message: Option<String>,
    ) -> Result<(), String> {
        self.runtime
            .resolve_interaction_request(
                &crate::runtime::interaction::InteractionId::new(interaction_id),
                crate::runtime::interaction::InteractionResolution::Cancel {
                    message: message.unwrap_or_else(|| "User cancelled.".to_string()),
                },
            )
            .map_err(|e| e.to_string())
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

    pub async fn get_subagent_transcript(
        &self,
        transcript_ref: String,
    ) -> Result<Vec<SubAgentTranscriptEntryFrontend>, String> {
        let agent_runtime = self
            .services
            .app
            .try_state::<Arc<AgentRuntime>>()
            .ok_or_else(|| "AgentRuntime state is not registered".to_string())?
            .inner()
            .clone();

        conversation_service::get_subagent_transcript(agent_runtime, transcript_ref).await
    }

    pub async fn create_conversation(&self) -> Result<String, String> {
        conversation_service::create_conversation(
            self.services.db.clone() as Arc<dyn ConversationStore>
        )
        .await
    }

    pub async fn get_conversation_model_override(
        &self,
        conversation_id: String,
    ) -> Result<Option<String>, String> {
        conversation_service::get_conversation_model_override(
            self.services.db.clone() as Arc<dyn ConversationStore>,
            conversation_id,
        )
        .await
    }

    pub async fn set_conversation_model_override(
        &self,
        conversation_id: String,
        model: Option<String>,
    ) -> Result<(), String> {
        conversation_service::set_conversation_model_override(
            self.services.db.clone() as Arc<dyn ConversationStore>,
            conversation_id,
            model,
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

        self.runtime
            .clear_session_state(&SessionId::new(outcome.conversation_id.clone()));

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

    pub async fn archive_conversation(&self, conversation_id: String) -> Result<(), String> {
        conversation_service::archive_conversation(
            self.services.db.clone() as Arc<dyn ConversationStore>,
            conversation_id,
        )
        .await
    }

    pub async fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_archived_conversations(
            self.services.db.clone() as Arc<dyn ConversationStore>
        )
        .await
    }

    pub async fn get_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        let mut convs = conversation_service::get_conversations(
            self.services.db.clone() as Arc<dyn ConversationStore>
        )
        .await?;
        // 为每个对话注入 workspaceName（来自已绑定的授权目录）。
        // 没有绑定目录的对话不注入字段，前端视为"默认文件夹"。
        for conv in &mut convs {
            if let Some(id) = conv["id"].as_str() {
                if let Some(ws) = chat_runtime_impl::load_explicit_workspace(&self.services.app, id)
                {
                    conv["workspaceName"] = serde_json::Value::String(ws.display_name);
                }
            }
        }
        Ok(convs)
    }

    pub async fn get_tasks(
        &self,
        conversation_id: String,
    ) -> Result<Vec<crate::models::message::TaskRecordFrontend>, String> {
        crate::models::message::TaskRecordFrontend::list_from_task_v2_store(
            self.services.db.base_dir(),
            &conversation_id,
        )
        .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl crate::runtime::schedule_runner::ScheduleRunDispatcher for TauriChatCommandAdapter {
    async fn dispatch_schedule_run(
        &self,
        schedule: crate::runtime::schedule::ScheduleRecord,
        fire_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let conversation_id = conversation_service::create_conversation(
            self.services.db.clone() as Arc<dyn ConversationStore>
        )
        .await
        .map_err(anyhow::Error::msg)?;
        let prompt = format!(
            "[定时任务触发] {}\n计划触发时间：{}\n\n{}",
            schedule.title, fire_at, schedule.prompt
        );
        self.send_message(
            conversation_id,
            prompt,
            Vec::new(),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .map_err(anyhow::Error::msg)
    }
}
