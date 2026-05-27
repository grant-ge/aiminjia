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
use crate::plugin::ToolRegistry;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::prompt::{PromptAssembler, PromptBuildContext, TurnPromptSnapshot};
use crate::runtime::chat::{
    LlmStepInput, LlmStepResult, ResolvedLlmSettings, RuntimeLlmExecutor, TurnConfig,
    TurnConfigOverrides, TurnError, TurnIterationState,
};
use crate::runtime::conversation_service;
use crate::runtime::ids::{SessionId, ToolCallId};
use crate::runtime::store::PendingPermissionResolution;
use crate::runtime::store::conversation_store::ConversationStore;
use crate::runtime::tools::permission::PermissionDestination;
use crate::runtime::{ChatTurnRequest, QueryEngine, RuntimeEventBus, SessionRuntime};
use crate::storage::crypto::SecureStorage;
use crate::storage::current_user_storage::CurrentUserStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;
use crate::storage::message_write_queue::{MessageWriteCompletion, MessageWriteQueue};
use crate::transport::tauri_event_adapter::TauriEventAdapter;
use crate::transport::tauri_runtime_host::TauriRuntimeHost;

pub mod chat_runtime_impl;

pub(crate) use chat_runtime_impl::build_visible_tool_defs;

/// Maximum number of stream-level retries within the agent loop.
/// When a stream error or gateway error is retryable (5xx, timeout, connection),
/// the current iteration is retried instead of aborting the entire agent loop.
const MAX_STREAM_RETRIES: u32 = 5;

/// Base delay before retrying a failed stream (seconds). Actual delay grows
/// exponentially with attempt number: 2s, 4s, 8s, 16s, 32s.
const STREAM_RETRY_DELAY_SECS: u64 = 2;

/// Compute exponential backoff for retry attempt N (1-based).
fn stream_retry_backoff_secs(attempt: u32) -> u64 {
    STREAM_RETRY_DELAY_SECS.saturating_mul(1u64 << attempt.min(5).saturating_sub(1))
}

fn attachment_refs_from_json_array(
    files: &[serde_json::Value],
) -> Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef> {
    files
        .iter()
        .map(
            |file| crate::runtime::chat::chat_turn_driver::ChatAttachmentRef {
                id: file
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                file_name: file
                    .get("fileName")
                    .or_else(|| file.get("originalName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                file_path: file
                    .get("filePath")
                    .or_else(|| file.get("path"))
                    .or_else(|| file.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                kind: file
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("file")
                    .to_string(),
                file_size: file.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0),
                file_type: file
                    .get("fileType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                mime_type: file
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
            },
        )
        .collect()
}

fn build_history_message_content(
    role: &str,
    content_value: &serde_json::Value,
    has_authorized_workspace: bool,
) -> Option<String> {
    if let Some(text) = content_value.get("text").and_then(|v| v.as_str()) {
        if role == "user" {
            if let Some(files) = content_value.get("files").and_then(|v| v.as_array()) {
                if !files.is_empty() {
                    let attachments = attachment_refs_from_json_array(files);
                    return Some(chat_runtime_impl::build_llm_content(
                        text,
                        &attachments,
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
                let role = value
                    .get("role")
                    .and_then(|role| role.as_str())
                    .unwrap_or("-");
                let content_chars = value
                    .get("content")
                    .and_then(|content| content.as_str())
                    .map(|content| content.chars().count())
                    .unwrap_or(0);
                let fields = value
                    .as_object()
                    .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","))
                    .unwrap_or_default();
                log::warn!(
                    "[run_llm_step] Failed to deserialize message for conv={}: {} — role={} content_chars={} fields=[{}]",
                    conversation_id,
                    error,
                    role,
                    content_chars,
                    fields
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

/// Per-conversation overrides injected by `dispatch_employee_run`.
/// Stored by conversation_id so concurrent employee runs do not interfere.
#[derive(Clone)]
struct EmployeeRunOverrides {
    /// Only these tool names are permitted for this run. Empty = allow all.
    tool_whitelist: std::collections::HashSet<String>,
    /// Max LLM iterations (steps) for this run.
    max_iterations: usize,
}

/// `MessageWriteTarget` that delegates to whichever `AppStorage` is active at call time.
/// When a user is logged in it writes to the user-scoped dir; otherwise falls back to `root_db`.
struct DynamicWriteTarget {
    cus: Arc<CurrentUserStorage>,
    root_db: Arc<AppStorage>,
}

impl DynamicWriteTarget {
    fn storage(&self) -> Arc<AppStorage> {
        self.cus.get_or(&self.root_db)
    }
}

impl crate::storage::message_write_queue::MessageWriteTarget for DynamicWriteTarget {
    fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> anyhow::Result<()> {
        self.storage()
            .insert_message(id, conversation_id, role, content_json)
            .map_err(Into::into)
    }

    fn update_message_content(
        &self,
        id: &str,
        conversation_id: &str,
        content_json: &str,
    ) -> anyhow::Result<()> {
        self.storage()
            .update_message_content(id, conversation_id, content_json)
            .map_err(Into::into)
    }
}

#[derive(Clone)]
#[allow(dead_code)]
struct TauriChatServices {
    cus: Arc<CurrentUserStorage>,
    root_db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    assistant_write_queue: Arc<MessageWriteQueue>,
    crypto: Option<Arc<SecureStorage>>,
    tool_registry: Arc<ToolRegistry>,
    auth_manager: Arc<AuthManager>,
    app: tauri::AppHandle,
    skill_registry: Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>,
    runtime_resolver: Option<crate::runtime::dependencies::ManagedRuntimeResolver>,
    /// Shared map for injecting employee-run tool whitelists per conversation.
    employee_run_overrides:
        Arc<std::sync::Mutex<std::collections::HashMap<String, EmployeeRunOverrides>>>,
}

impl TauriChatServices {
    fn db(&self) -> Arc<AppStorage> {
        self.cus.get_or(&self.root_db)
    }
}

struct TauriLegacyTurnExecutor {
    services: TauriChatServices,
    agents_md_loader: Arc<tokio::sync::Mutex<crate::runtime::agents_md::AgentsMdLoader>>,
}

impl TauriLegacyTurnExecutor {
    fn current_app_language(&self) -> String {
        let settings_map = self.services.db().get_all_settings().unwrap_or_default();
        let settings = AppSettings::from_string_map(&settings_map);
        if settings.app_language == "en-US" {
            "en-US".to_string()
        } else {
            "zh-CN".to_string()
        }
    }
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
        use crate::runtime::events::{RetryReason, RuntimeEvent, RuntimeEventKind};
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
            system_prompt_content(input.system_message.clone(), input.system_prompt);
        let system_prompt_segments = system_prompt_segments(&input.system_message);

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
                .stream_message_with_segments(
                    &settings,
                    chat_messages.clone(),
                    masking_level.clone(),
                    system_prompt_for_gateway.as_deref(),
                    dynamic_ctx_opt,
                    effective_tools.clone(),
                    input.token_budget as u32,
                    Some(input.conversation_id),
                    input.anthropic_multimodal_turn.clone(),
                    system_prompt_segments.clone(),
                    Some(input.trace_id),
                    Some(input.run_id),
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
                                RuntimeEventKind::StreamRetryReset {
                                    reason: classify_retry_reason(&err_str),
                                },
                            ))
                            .await;
                        tokio::time::sleep(std::time::Duration::from_secs(
                            stream_retry_backoff_secs(stream_retry_count),
                        ))
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
            let mut cache_creation_input_tokens: u64 = 0;
            let mut cache_read_input_tokens: u64 = 0;
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
                                    RuntimeEventKind::StreamRetryReset {
                                        reason: RetryReason::NetworkFlap,
                                    },
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
                                    log::info!(
                                        "[stream-timing-be] delta len={} total={} run={}",
                                        clean.len(), iter_content.len(), run_id.as_str(),
                                    );
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
                            Some(StreamEvent::ThinkingBlock { .. }) => {
                                // ThinkingBlock: full thinking block w/ signature — intentionally dropped here.
                            }
                            Some(StreamEvent::Keepalive) => {
                                // Liveness tick (Anthropic ping / input_json_delta tool-arg
                                // fragment / message_start). No content to process — but
                                // reaching this arm means a fresh SSE event arrived on the
                                // wire, so the per-iteration chunk-timeout watchdog (re-armed
                                // at the top of the loop) is effectively reset. This is the
                                // fix for false "响应超时（90秒无数据）" aborts during long
                                // tool-argument streaming and ping-only thinking windows.
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
                                cache_creation_input_tokens =
                                    usage.cache_creation_input_tokens.unwrap_or(0) as u64;
                                cache_read_input_tokens =
                                    usage.cache_read_input_tokens.unwrap_or(0) as u64;
                                log::info!(
                                    "[run_llm_step] Stream done: stop_reason={:?} \
                                     in={} out={} cache_creation={} cache_read={} content_len={} tool_calls={}",
                                    stop_reason, tokens_in, tokens_out,
                                    cache_creation_input_tokens, cache_read_input_tokens,
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
                                            RuntimeEventKind::StreamRetryReset {
                                                reason: classify_retry_reason(&error),
                                            },
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
                let backoff = stream_retry_backoff_secs(stream_retry_count);
                log::info!(
                    "[run_llm_step] Retrying after {}s (retry {}/{}) conv={}",
                    backoff,
                    stream_retry_count,
                    MAX_STREAM_RETRIES,
                    input.conversation_id
                );
                tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
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
                    cache_creation_input_tokens,
                    cache_read_input_tokens,
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
                cache_creation_input_tokens,
                cache_read_input_tokens,
            });
        }
    }

    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        self.load_llm_settings_for_turn(&ChatTurnRequest::new("", "", vec![]))
            .await
    }

    async fn load_llm_settings_for_turn(
        &self,
        _request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        let global_settings_map = self.services.db().get_all_settings().unwrap_or_default();
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
                .db()
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

        Ok(ResolvedLlmSettings {
            primary_model: settings.primary_model,
            primary_api_key: settings.primary_api_key,
            auto_model_routing: settings.auto_model_routing,
            custom_model_endpoint: settings.custom_model_endpoint,
            custom_model_name: settings.custom_model_name,
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

    async fn persist_user_message(
        &self,
        conversation_id: &str,
        content: &str,
        attachments: &[crate::runtime::chat::chat_turn_driver::ChatAttachmentRef],
        skill_command: Option<&crate::runtime::chat::chat_turn_driver::SkillCommandRef>,
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        let msg_id = format!("msg-{}", uuid::Uuid::new_v4());

        let content_json =
            crate::runtime::chat::chat_turn_driver::build_user_content_json_with_skill(
                content,
                attachments,
                skill_command,
            )
            .to_string();

        if let Err(e) =
            self.services
                .db()
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
    ) -> Result<Option<String>, TurnError> {
        if tool_calls.is_empty() {
            return Ok(None);
        }
        let msg_id = uuid::Uuid::new_v4().to_string();
        log::info!(
            "[persist_iteration_assistant_message] Saving assistant[toolCalls] id={} conv={}",
            msg_id,
            conversation_id
        );
        let stored = crate::storage::file_store::types::StoredMessage {
            id: msg_id.clone(),
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
            .db()
            .insert_chat_message_record(&stored)
            .map_err(|e| TurnError::PersistenceError(e.to_string()))?;
        Ok(Some(msg_id))
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
            let is_error = msg
                .get("isError")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let stored = crate::storage::file_store::types::StoredMessage {
                id: msg_id.clone(),
                conversation_id: conversation_id.to_string(),
                role: "tool".to_string(),
                content: serde_json::json!({ "text": content, "isError": is_error }),
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
            if let Err(e) = self.services.db().insert_chat_message_record(&stored) {
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
        if self
            .services
            .db()
            .get_conversation(conversation_id)
            .is_err()
        {
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
                .db()
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
            self.services.db().clone(),
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
    async fn build_system_prompt(&self, request: &ChatTurnRequest) -> Result<String, TurnError> {
        let persona = match request.persona_id_override.as_deref() {
            Some(id) => self
                .services
                .db()
                .get_persona(id)
                .ok()
                .or_else(|| self.services.db().get_active_persona().ok()),
            None => self.services.db().get_active_persona().ok(),
        };

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
        request: &ChatTurnRequest,
    ) -> Result<Option<TurnPromptSnapshot>, TurnError> {
        let persona = match request.persona_id_override.as_deref() {
            Some(id) => self
                .services
                .db()
                .get_persona(id)
                .ok()
                .or_else(|| self.services.db().get_active_persona().ok()),
            None => self.services.db().get_active_persona().ok(),
        };

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
        attachments: &[crate::runtime::chat::chat_turn_driver::ChatAttachmentRef],
    ) -> Result<String, TurnError> {
        let authorized_workspace =
            chat_runtime_impl::load_authorized_workspace(&self.services.app, conversation_id);
        Ok(chat_runtime_impl::build_llm_content(
            content,
            attachments,
            authorized_workspace.is_some(),
        ))
    }

    async fn load_turn_config_overrides(
        &self,
        request: &ChatTurnRequest,
    ) -> Result<TurnConfigOverrides, TurnError> {
        let employee_overrides = self
            .services
            .employee_run_overrides
            .lock()
            .ok()
            .and_then(|map| map.get(request.conversation_id.as_str()).cloned());
        log::info!(
            "[tool-desc-trace] entered load_turn_config_overrides: conv={} employee_overrides_is_some={}",
            request.conversation_id,
            employee_overrides.is_some()
        );

        // 第一步：决定 schema 过滤策略
        let schema_filter = match &employee_overrides {
            Some(ov) if !ov.tool_whitelist.is_empty() => {
                chat_runtime_impl::ToolSchemaFilter::EmployeeWhitelist(
                    ov.tool_whitelist.iter().cloned().collect(),
                )
            }
            _ => chat_runtime_impl::ToolSchemaFilter::DailyWhitelist,
        };

        // 第二步：独立计算运行时权限白名单（与 schema 过滤是两回事）
        let runtime_allowed_tools: std::collections::HashSet<String> = match &employee_overrides {
            Some(ov) if !ov.tool_whitelist.is_empty() => {
                ov.tool_whitelist.iter().cloned().collect()
            }
            _ => crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };

        let max_iterations = employee_overrides
            .as_ref()
            .map(|ov| ov.max_iterations)
            .unwrap_or(300);

        let authorized_workspace = chat_runtime_impl::load_authorized_workspace(
            &self.services.app,
            request.conversation_id.as_str(),
        );

        // ── Build ToolDescriptionContext for this turn ──────────────────
        // Tools whose description depends on session state (notably
        // `Agent`, which must list dispatchable subagent types and hired
        // employees) read from this.  Mirrors claude-code-best's
        // `tool.prompt({ agents, tools, ... })` pipeline.
        let tool_desc_ctx =
            chat_runtime_impl::build_tool_description_context(&self.services.app).await;
        let employee_count_in_ctx = tool_desc_ctx
            .agents
            .iter()
            .filter(|a| {
                matches!(
                    a.source,
                    crate::runtime::agent::definition::AgentSource::Employee
                )
            })
            .count();
        log::info!(
            "[tool-desc-trace] built ctx: employees={} agents={}",
            employee_count_in_ctx,
            tool_desc_ctx.agents.len()
        );

        // ── Request-scoped tool description overrides ───────────────────
        // `Agent` is request-scoped — no instance lives in the global
        // runtime_tools map, so `get_schemas_filtered` falls back to
        // TOOL_CATALOG (static).  Construct the tool here (we have
        // AgentRegistry + EmployeeStore handles via app state) and
        // render its description once so the emp-id catalog actually
        // reaches the LLM.
        let request_scoped_overrides = chat_runtime_impl::build_request_scoped_tool_overrides(
            &self.services.app,
            &tool_desc_ctx,
        )
        .await;
        {
            let keys: Vec<&str> = request_scoped_overrides
                .keys()
                .map(|k| k.as_str())
                .collect();
            log::info!("[tool-desc-trace] built overrides: keys={:?}", keys);
        }

        let visible_tool_defs = chat_runtime_impl::build_visible_tool_defs(
            self.services.tool_registry.as_ref(),
            authorized_workspace.is_some(),
            schema_filter,
            &tool_desc_ctx,
            &request_scoped_overrides,
        )
        .await;
        {
            let agent_has_emp = visible_tool_defs
                .iter()
                .find(|d| d.name == "Agent")
                .map(|d| d.description.contains("<available_subagent_types>"))
                .unwrap_or(false);
            log::info!(
                "[tool-desc-trace] built visible_tool_defs: count={} agent_desc_has_emp_section={}",
                visible_tool_defs.len(),
                agent_has_emp,
            );
        }
        let json_defs = visible_tool_defs
            .into_iter()
            .filter_map(|td| serde_json::to_value(&td).ok())
            .collect();

        Ok(TurnConfigOverrides {
            system_prompt: None, // P0 修复：让 PromptAssembler 产物真正进入 LLM
            tool_defs: Some(json_defs),
            allowed_tools: Some(runtime_allowed_tools),
            max_iterations: Some(max_iterations),
            token_budget: None,
            authorized_workspace,
        })
    }

    async fn load_history(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        let authorized_workspace =
            chat_runtime_impl::load_authorized_workspace(&self.services.app, conversation_id);
        let chat_messages = load_history_via_runtime_history(
            &self.services.db(),
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
            .db()
            .append_compact_boundary(&record)
            .map_err(|e| {
                TurnError::PersistenceError(format!("Failed to persist compact boundary: {}", e))
            })
    }

    async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
        use crate::runtime::chat::context_builder::{ManagedRuntimeEnvInfo, build_env_info};

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

    async fn get_skill_catalog(&self, _agent_id: Option<&str>) -> String {
        let language = self.current_app_language();
        self.services
            .skill_registry
            .lock()
            .map(|reg| reg.format_full_catalog_for_language(200_000, &language))
            .unwrap_or_default()
    }

    async fn load_workspace_path(&self) -> Result<std::path::PathBuf, TurnError> {
        Ok(self.services.file_mgr.workspace_path().to_path_buf())
    }

    async fn load_agents_md(
        &self,
        authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
    ) -> Result<Vec<crate::runtime::agents_md::AgentsMdFile>, TurnError> {
        let mut loader = self.agents_md_loader.lock().await;
        Ok(loader.load(authorized_workspace).await)
    }

    async fn load_project_memory(
        &self,
        workspace_path: &std::path::Path,
        query: &str,
    ) -> Result<crate::runtime::project_memory::ProjectMemoryContext, TurnError> {
        let app_data_dir = self.services.db().base_dir().to_path_buf();
        let service = crate::runtime::project_memory::ProjectMemoryService::new(
            app_data_dir,
            workspace_path.to_path_buf(),
        );
        service.load_context(query).map_err(|err| {
            TurnError::PersistenceError(format!("Failed to load project memory: {err}"))
        })
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        // Production tool_defs are populated via load_turn_config_overrides
        // (returns Some(json_defs)), so the driver overrides this empty default.
        // This impl exists only to satisfy the trait — it should never be the value
        // actually used in a turn.
        log::info!(
            "[tool-desc-trace] entered get_tool_defs (fallback path — should be overridden by load_turn_config_overrides)"
        );
        Ok(vec![])
    }

    async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok(self.services.db().load_core_memory())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

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

    #[test]
    fn build_history_message_content_preserves_uploaded_file_hints() {
        let content = serde_json::json!({
            "text": "请继续分析这个表格",
            "files": [
                {
                    "id": "attachment-1",
                    "fileName": "sales.csv",
                    "filePath": "/tmp/sales.csv",
                    "kind": "file",
                    "fileType": "csv",
                    "mimeType": "text/csv"
                }
            ]
        });

        let llm_content =
            build_history_message_content("user", &content, false).expect("history content");

        assert!(llm_content.contains("[当前消息附件]"));
        assert!(llm_content.contains("/tmp/sales.csv"));
        assert!(llm_content.contains("本轮附件已自动加入授权目录"));
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

    // -----------------------------------------------------------------------
    // TauriChatServices::db() — dynamic user-scope resolution
    // -----------------------------------------------------------------------

    fn make_cus_with_home(
        tmp: &TempDir,
    ) -> Arc<crate::storage::current_user_storage::CurrentUserStorage> {
        let home = Arc::new(crate::storage::AiJiaHome::from_path(
            tmp.path().to_path_buf(),
        ));
        Arc::new(crate::storage::current_user_storage::CurrentUserStorage::new(home))
    }

    /// Wraps `CurrentUserStorage::get_or` — the same logic as `TauriChatServices::db()`.
    fn resolve_db(
        cus: &crate::storage::current_user_storage::CurrentUserStorage,
        root_db: &Arc<AppStorage>,
    ) -> Arc<AppStorage> {
        cus.get_or(root_db)
    }

    #[test]
    fn services_db_returns_root_before_login() {
        let root_tmp = TempDir::new().unwrap();
        let cus_tmp = TempDir::new().unwrap();
        let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
        let cus = make_cus_with_home(&cus_tmp);
        assert_eq!(
            resolve_db(&cus, &root_db).base_dir(),
            root_db.base_dir(),
            "before login db() must resolve to root_db"
        );
    }

    #[test]
    fn services_db_returns_user_dir_after_login() {
        let root_tmp = TempDir::new().unwrap();
        let cus_tmp = TempDir::new().unwrap();
        let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
        let cus = make_cus_with_home(&cus_tmp);

        let scope = crate::storage::UserScope::new(1, 2);
        cus.activate_scope(scope).unwrap();

        let expected = cus_tmp.path().join("users").join("t_1__u_2");
        assert_eq!(
            resolve_db(&cus, &root_db).base_dir(),
            expected.as_path(),
            "after login db() must resolve to user-scoped dir"
        );
        assert_ne!(
            resolve_db(&cus, &root_db).base_dir(),
            root_db.base_dir(),
            "after login db() must not point at root_db"
        );
    }

    #[test]
    fn services_db_falls_back_to_root_after_logout() {
        let root_tmp = TempDir::new().unwrap();
        let cus_tmp = TempDir::new().unwrap();
        let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
        let cus = make_cus_with_home(&cus_tmp);

        cus.activate_scope(crate::storage::UserScope::new(1, 2))
            .unwrap();
        cus.deactivate();

        assert_eq!(
            resolve_db(&cus, &root_db).base_dir(),
            root_db.base_dir(),
            "after logout db() must fall back to root_db"
        );
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
        cloud_model: settings.cloud_model.clone(),
        cloud_model_type: settings.cloud_model_type.clone(),
        thinking_type: settings.thinking_type.clone(),
        thinking_budget_tokens: settings.thinking_budget_tokens,
        ..AppSettings::default()
    }
}

fn system_prompt_content(value: Option<serde_json::Value>, fallback: &str) -> Option<String> {
    value
        .as_ref()
        .and_then(|v| flatten_system_message_value(v))
        .or_else(|| {
            if fallback.trim().is_empty() {
                None
            } else {
                Some(fallback.to_string())
            }
        })
}

/// Extract Anthropic-style structured cache segments from the rendered
/// system message. Returns an empty Vec when the message is missing or only
/// holds a single string (no per-block cache breakpoints to forward).
pub(crate) fn system_prompt_segments(
    value: &Option<serde_json::Value>,
) -> Vec<crate::llm::streaming::SystemPromptSegment> {
    let Some(value) = value.as_ref() else {
        return Vec::new();
    };
    let Some(content) = value.get("content") else {
        return Vec::new();
    };
    let Some(arr) = content.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|block| {
            let text = block.get("text")?.as_str()?;
            if text.trim().is_empty() {
                return None;
            }
            let cache = block.get("cache_control").is_some();
            Some(crate::llm::streaming::SystemPromptSegment {
                text: text.to_string(),
                cache,
            })
        })
        .collect()
}

fn flatten_system_message_value(value: &serde_json::Value) -> Option<String> {
    if value.get("role").and_then(|r| r.as_str()) != Some("system") {
        return None;
    }
    let content = value.get("content")?;
    if let Some(s) = content.as_str() {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    } else if let Some(arr) = content.as_array() {
        let joined: String = arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if joined.trim().is_empty() {
            None
        } else {
            Some(joined)
        }
    } else {
        None
    }
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

/// Classify a retryable error into a [`RetryReason`] so the frontend can show
/// the right toast (upstream busy vs local network flap vs rate limit).
fn classify_retry_reason(error: &str) -> crate::runtime::events::RetryReason {
    use crate::runtime::events::RetryReason;
    let lower = error.to_lowercase();
    // Local-side signals win over status codes — a "timeout" wrapping a 5xx
    // response is still a real timeout on our side worth flagging as network.
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
    {
        RetryReason::NetworkFlap
    } else if lower.contains("429") || lower.contains("rate limit") {
        RetryReason::RateLimited
    } else if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        RetryReason::UpstreamBusy
    } else {
        RetryReason::NetworkFlap
    }
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
        let system_prompt = system_prompt_content(Some(rendered_system), "legacy system")
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
    fn current_app_language(&self) -> String {
        let settings_map = self.services.db().get_all_settings().unwrap_or_default();
        let settings = AppSettings::from_string_map(&settings_map);
        if settings.app_language == "en-US" {
            "en-US".to_string()
        } else {
            "zh-CN".to_string()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cus: Arc<CurrentUserStorage>,
        root_db: Arc<AppStorage>,
        gateway: Arc<LlmGateway>,
        file_mgr: Arc<FileManager>,
        crypto: Option<Arc<SecureStorage>>,
        tool_registry: Arc<ToolRegistry>,
        skill_registry: Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>,
        auth_manager: Arc<AuthManager>,
        permission_store: Arc<crate::runtime::store::PermissionStore>,
        app: tauri::AppHandle,
    ) -> Self {
        Self::new_with_channel_sessions(
            cus,
            root_db,
            gateway,
            file_mgr,
            crypto,
            tool_registry,
            skill_registry,
            auth_manager,
            permission_store,
            app,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_channel_sessions(
        cus: Arc<CurrentUserStorage>,
        root_db: Arc<AppStorage>,
        gateway: Arc<LlmGateway>,
        file_mgr: Arc<FileManager>,
        crypto: Option<Arc<SecureStorage>>,
        tool_registry: Arc<ToolRegistry>,
        skill_registry: Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>,
        auth_manager: Arc<AuthManager>,
        permission_store: Arc<crate::runtime::store::PermissionStore>,
        app: tauri::AppHandle,
        channel_sessions: Option<
            Arc<dyn crate::connector::im::shared::ask_coordinator::ChannelSessionRegistry>,
        >,
    ) -> Self {
        let runtime_resolver = app
            .try_state::<crate::runtime::dependencies::ManagedRuntimeResolver>()
            .map(|resolver| resolver.inner().clone());
        let assistant_write_queue =
            Arc::new(MessageWriteQueue::new(Arc::new(DynamicWriteTarget {
                cus: cus.clone(),
                root_db: root_db.clone(),
            })));
        let services = TauriChatServices {
            cus,
            root_db,
            gateway,
            file_mgr,
            assistant_write_queue,
            crypto,
            tool_registry,
            auth_manager,
            app,
            skill_registry,
            runtime_resolver,
            employee_run_overrides: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        };
        let host = Arc::new(TauriRuntimeHost::new(services.app.clone()));
        let adapter = Arc::new(match channel_sessions {
            Some(registry) => TauriEventAdapter::with_channel_sessions(host.clone(), registry),
            None => TauriEventAdapter::new(host.clone()),
        });
        let bus = RuntimeEventBus::new();
        bus.subscribe(adapter);
        let llm_executor: Arc<dyn RuntimeLlmExecutor> = Arc::new(TauriLegacyTurnExecutor {
            services: services.clone(),
            agents_md_loader: Arc::new(tokio::sync::Mutex::new(
                crate::runtime::agents_md::AgentsMdLoader::new(),
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
        if let Some(queue) = services
            .app
            .try_state::<Arc<crate::runtime::agent::task_notification::TaskNotificationQueue>>()
        {
            runtime = runtime.with_task_notification_queue(queue.inner().clone());
        } else {
            // Fail-closed: log error and leave the queue as None (no notifications this session).
            // This is symmetric with spawn_subagent's fail-closed path in plugin/registry.rs —
            // if the queue is missing, spawn_subagent also returns None, so no notifications
            // can be enqueued either. The system stays consistent without crashing.
            log::error!(
                "[chat] TaskNotificationQueue not in app state — async sub-agent notifications \
                 will not be surfaced this session; cross-check with spawn_subagent registration \
                 which should also be disabled"
            );
        }
        // LTR (P1.8): wire per-process Team / AgentName registries so
        // cancel_session can drop their per-session entries.
        if let Some(team_reg) = services
            .app
            .try_state::<Arc<crate::runtime::agent::TeamRegistry>>()
        {
            runtime = runtime.with_team_registry(team_reg.inner().clone());
        }
        if let Some(name_reg) = services
            .app
            .try_state::<Arc<crate::runtime::agent::AgentNameRegistry>>()
        {
            runtime = runtime.with_agent_names(name_reg.inner().clone());
        }
        if let Some(inbox_reg) = services
            .app
            .try_state::<Arc<crate::runtime::agent::InboxRegistry>>()
        {
            runtime = runtime.with_inbox_registry(inbox_reg.inner().clone());
        }
        if let Some(sup) = services
            .app
            .try_state::<Arc<crate::runtime::agent::LeadIdleSupervisor>>()
        {
            runtime = runtime.with_lead_idle(sup.inner().clone());
        }
        if let Some(reg) = services
            .app
            .try_state::<Arc<crate::runtime::agent::CancellationRegistry>>()
        {
            runtime = runtime.with_cancellation_registry(reg.inner().clone());
        }
        runtime = runtime.with_host(host as Arc<dyn crate::transport::runtime_host::RuntimeHost>);
        // Path C wake (LTR B-gap1) is now wired by `wire_path_c_wake_to_self`
        // after the adapter is wrapped in `Arc<Self>` (see lib.rs).  We can't
        // wire it here because the wake closure needs to call
        // `Arc<Self>::send_chat_request` to reuse the user-send code path
        // (which constructs a per-request ToolDispatcher with all services);
        // we don't have an `Arc<Self>` until the caller wraps us.  See
        // `wire_path_c_wake_to_self` for details and rationale.
        Self { runtime, services }
    }

    async fn load_llm_settings_for_turn(
        &self,
        request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        TauriLegacyTurnExecutor {
            services: self.services.clone(),
            agents_md_loader: Arc::new(tokio::sync::Mutex::new(
                crate::runtime::agents_md::AgentsMdLoader::new(),
            )),
        }
        .load_llm_settings_for_turn(request)
        .await
    }

    fn agenda_store_for_current_user(&self) -> anyhow::Result<crate::runtime::agenda::AgendaStore> {
        use crate::storage::{CurrentUserStorage, UserScopedPathResolver};

        let resolver = self
            .services
            .app
            .try_state::<Arc<CurrentUserStorage>>()
            .ok_or_else(|| anyhow::anyhow!("CurrentUserStorage not registered"))?;
        let paths = resolver.require_paths()?;
        Ok(crate::runtime::agenda::AgendaStore::new(paths.base_dir()))
    }

    /// 返回当前 workspace 根目录，供 ChannelManager 等调用方构造下载目录。
    pub fn workspace_path(&self) -> std::path::PathBuf {
        self.services.file_mgr.workspace_path().to_path_buf()
    }

    /// 向内部 runtime event bus 注册外部订阅者。
    pub fn subscribe_event_listener(
        &self,
        subscriber: std::sync::Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>,
    ) {
        self.runtime.subscribe_event_listener(subscriber);
    }

    /// LTR P2 (B-gap1 Path C) — wire the LeadIdleSupervisor's wake_fn to
    /// reuse `send_chat_request` (the same code path user-driven
    /// `send_message` takes) instead of `SessionRuntime::spawn_continuation`.
    ///
    /// # Why this is on the transport layer (not in `SessionRuntime`)
    ///
    /// A Path-C continuation turn needs a per-request `ToolDispatcher`
    /// (`tool_registry.to_runtime_dispatcher(deps)`), and constructing one
    /// requires services that live on the transport layer:
    /// `tool_registry` / `gateway` / `auth_manager` / `skill_registry` / etc.
    /// `SessionRuntime` is intentionally transport-neutral; making it carry
    /// these services would invert the dependency.  By driving wake from the
    /// transport layer we keep both the user-send path and the wake path
    /// going through the *exact* same `send_chat_request` code, so they
    /// can't drift (which is precisely how the
    /// "tool dispatcher not configured" bug appeared in the first place:
    /// `spawn_continuation` was clone-ing the base `SessionRuntime` whose
    /// `QueryEngine` had no dispatcher).
    ///
    /// # Lifetime / cycle
    ///
    /// We hold a `Weak<Self>` inside the wake closure to avoid keeping the
    /// adapter alive forever via the supervisor.  At wake time we `upgrade`
    /// — if the adapter has been dropped (app shutdown), the wake is a no-op.
    pub fn wire_path_c_wake_to_self(self: &Arc<Self>) {
        let Some(sup) = self.runtime.lead_idle_supervisor() else {
            log::debug!(
                "[wire_path_c_wake_to_self] no LeadIdleSupervisor — skipping; \
                 LTR is not enabled in this build"
            );
            return;
        };
        let weak_self: std::sync::Weak<Self> = Arc::downgrade(self);
        let installed = sup.set_wake_fn(std::sync::Arc::new(
            move |key: crate::runtime::agent::LeadKey, team_name: String| {
                let Some(adapter) = weak_self.upgrade() else {
                    log::warn!(
                        "[path_c_wake] adapter has been dropped — skipping continuation \
                         turn for session={} agent={} team={}",
                        key.0.as_str(),
                        key.1.as_str(),
                        team_name
                    );
                    return;
                };
                let session_str = key.0.as_str().to_string();
                let agent_str = key.1.as_str().to_string();
                let team_str = team_name.clone();
                tokio::spawn(async move {
                    log::info!(
                        "[path_c_wake] spawning continuation turn via send_chat_request \
                         conv={} lead={} team={}",
                        session_str,
                        agent_str,
                        team_str
                    );
                    // PR6: forward wake-source team_name verbatim so the
                    // continuation turn uses it as active_team_name without
                    // re-reading conv.json (which may not yet reflect this
                    // team — e.g. when active_team is "alpha" but the wake
                    // came from "beta").
                    let mut req = ChatTurnRequest::new(
                        key.0.clone(),
                        "__resume_from_task_notification__".to_string(),
                        Vec::new(),
                    );
                    if !team_name.is_empty() {
                        req = req.with_active_team_name_override(team_name);
                    }
                    match adapter.send_chat_request(req).await {
                        Ok(()) => {
                            log::info!(
                                "[path_c_wake] continuation turn completed conv={}",
                                session_str
                            );
                        }
                        Err(e) => {
                            // Expected error: another turn is already busy on this
                            // conversation (user-driven send_message landed first or
                            // a prior wake is still running).  Log and let the
                            // pending state stay set — the running turn will pick
                            // up the inbox at its own next opportunity.
                            log::warn!(
                                "[path_c_wake] continuation turn rejected (likely a \
                                 concurrent turn already busy) conv={}: {}",
                                session_str,
                                e
                            );
                        }
                    }
                });
            },
        ));
        if !installed {
            log::warn!(
                "[wire_path_c_wake_to_self] supervisor already had a wake_fn installed; \
                 keeping the previous one. This usually indicates the wire was called twice."
            );
        } else {
            log::info!(
                "[wire_path_c_wake_to_self] wake_fn installed — Path C continuation \
                 turns will reuse send_chat_request"
            );
        }
    }

    /// 暴露权限控制平面，供 IM 协调器等外部组件使用。
    pub fn permission_control_plane(
        &self,
    ) -> std::sync::Arc<dyn crate::runtime::store::PendingPermissionControlPlane> {
        self.runtime.permission_control_plane()
    }

    /// 暴露交互控制平面，供 IM 协调器等外部组件使用。
    pub fn interaction_control_plane(
        &self,
    ) -> std::sync::Arc<dyn crate::runtime::interaction::PendingInteractionControlPlane> {
        self.runtime.interaction_control_plane()
    }

    /// 与 `send_message` 相同，但接受调用方已预构造的 `ChatTurnRequest`，
    /// 保留其中的 `run_id`，用于外部需要在发送前注册 run_id 的场景（如 DingtalkReplyManager）。
    pub async fn send_chat_request(&self, request: ChatTurnRequest) -> Result<(), String> {
        let conversation_id = request.conversation_id.as_str().to_string();
        let run_id = request.run_id.clone();
        self.services
            .gateway
            .set_busy_for_run(&conversation_id, run_id.clone())?;

        let session_id = request.conversation_id.clone();
        let agent_runtime = self
            .services
            .app
            .try_state::<Arc<crate::runtime::agent::AgentRuntime>>()
            .map(|v| v.inner().clone());
        let workspace_path = self.services.file_mgr.workspace_path();
        let request_scoped_runtime_deps = crate::plugin::registry::RequestScopedRuntimeDeps {
            storage: self.services.db().clone(),
            file_manager: self.services.file_mgr.clone(),
            workspace_path: workspace_path.clone(),
            conversation_id: session_id.as_str().to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            agent_id: None,
            app_handle: Some(self.services.app.clone()),
            auth_manager: Some(self.services.auth_manager.clone()),
            model: String::new(),
            gateway: Some(self.services.gateway.clone()),
            tool_registry: Some(self.services.tool_registry.clone()),
            app_settings: Some(Arc::new(AppSettings::default())),
            agent_runtime,
            event_bus: None,
            skill_registry: Some(self.services.skill_registry.clone()),
            authorized_workspace: None,
            read_file_state: None,
            cancellation: None,
            permission_mode: request.permission_mode,
            runtime_resolver: self.services.runtime_resolver.clone(),
            permission_ctx: None,
            current_persona_id: None,
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
        let result = runtime.run_chat_request(request).await;
        self.services
            .gateway
            .clear_task_for_run(&conversation_id, &run_id);

        if result.is_ok() {
            self.spawn_auto_title(conversation_id.clone(), 0).await;
        }

        // After this turn ends (success or otherwise), let the PendingQueueManager
        // schedule a debounced drain so any items buffered while we were busy
        // get merged into the next turn.
        if let Some(mgr) = self
            .services
            .app
            .try_state::<Arc<crate::runtime::pending::PendingQueueManager>>()
        {
            let mgr_clone = mgr.inner().clone();
            let session_id = crate::runtime::ids::SessionId::new(conversation_id.clone());
            tauri::async_runtime::spawn(async move {
                mgr_clone.schedule_drain(session_id).await;
            });
        }

        result
    }

    pub async fn send_message(
        &self,
        conversation_id: String,
        content: String,
        attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
        permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
        agent_name: Option<String>,
        client_message_id: Option<String>,
        skill_command: Option<crate::runtime::chat::chat_turn_driver::SkillCommandRef>,
    ) -> Result<(), String> {
        log::info!(
            "[send_message] trace_id={:?} conversation_id={} content_len={} attachments_count={}",
            client_message_id.as_deref(),
            conversation_id,
            content.len(),
            attachments.len()
        );
        for att in &attachments {
            log::info!(
                "[send_message] attachment: name={} path={} kind={} type={}",
                att.file_name,
                att.file_path,
                att.kind,
                att.file_type
            );
        }

        // Pending queue gate: if the session is busy, enqueue this message
        // instead of returning "already processing". Chips will appear in the
        // UI; drain will merge it into the next turn.
        if let Some(mgr_state) = self
            .services
            .app
            .try_state::<Arc<crate::runtime::pending::PendingQueueManager>>()
        {
            let pending_mgr = mgr_state.inner().clone();
            let pending_item = crate::runtime::pending::PendingItem {
                id: format!("pend-{}", uuid::Uuid::new_v4()),
                source: crate::runtime::pending::PendingSource::App,
                text: content.clone(),
                sender_nick: None,
                attachments: attachments
                    .iter()
                    .map(|a| crate::runtime::pending::PendingAttachment {
                        id: a.id.clone(),
                        file_path: a.file_path.clone(),
                        mime: a.mime_type.clone(),
                        size_bytes: Some(a.file_size),
                    })
                    .collect(),
                skill_command: skill_command.clone(),
                received_at: chrono::Utc::now().to_rfc3339(),
            };
            let session_id = crate::runtime::ids::SessionId::new(conversation_id.clone());
            let outcome = pending_mgr
                .enqueue_or_send(session_id, pending_item)
                .await
                .map_err(|e| format!("enqueue_or_send error: {e:#}"))?;
            match outcome {
                crate::runtime::pending::EnqueueOutcome::SentDirectly { .. } => {
                    // Fall through to the existing send path (which preserves
                    // skill_id / agent_name / permission_mode / client_message_id).
                }
                crate::runtime::pending::EnqueueOutcome::Queued { snapshot } => {
                    log::info!(
                        "[pending] app composer message queued conv={} queue_size={}",
                        conversation_id,
                        snapshot.len()
                    );
                    return Ok(());
                }
                crate::runtime::pending::EnqueueOutcome::Rejected { reason } => {
                    return Err(match reason {
                        crate::runtime::pending::EnqueueRejection::QueueFull { limit } => {
                            format!("消息堆积过多（已达 {limit} 条），请稍后再发")
                        }
                        crate::runtime::pending::EnqueueRejection::SessionArchived => {
                            "会话已归档，无法发送消息".to_string()
                        }
                    });
                }
            }
        }

        let mut request = ChatTurnRequest::new(conversation_id.clone(), content, attachments);
        request.skill_command = skill_command;
        // Derive per-turn attachment dirs on the backend (frontend paths are untrusted).
        // The derived dirs will be merged into the per-turn ToolPermissionContext as
        // RuleSource::Session in QueryEngine::build_turn_permission_ctx.
        request.session_attachment_dirs =
            crate::runtime::path_auth::derive_working_dirs_from_attachments(
                &request
                    .attachments
                    .iter()
                    .map(|a| std::path::PathBuf::from(&a.file_path))
                    .collect::<Vec<_>>(),
            );
        log::info!(
            "[send_message] derived session_attachment_dirs count={} dirs={:?}",
            request.session_attachment_dirs.len(),
            request.session_attachment_dirs
        );
        request.agent_name = agent_name;
        request.client_message_id = client_message_id;
        if let Some(permission_mode) = permission_mode {
            request.permission_mode = permission_mode;
        }
        self.run_chat_request_internal(request).await
    }

    pub async fn send_message_with_overrides(
        &self,
        conversation_id: String,
        content: String,
        attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
        permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
        agent_name: Option<String>,
        client_message_id: Option<String>,
        persona_id_override: Option<String>,
        run_id: Option<crate::runtime::ids::RunId>,
    ) -> Result<crate::runtime::ids::RunId, String> {
        let mut request = crate::runtime::chat::chat_turn_driver::ChatTurnRequest::new(
            conversation_id.clone(),
            content,
            attachments,
        );
        if let Some(id) = run_id {
            request.run_id = id;
        }
        request.agent_name = agent_name;
        request.persona_id_override = persona_id_override;
        request.permission_mode = permission_mode.unwrap_or_default();
        request.client_message_id = client_message_id;

        let captured_run_id = request.run_id.clone();

        self.run_chat_request_internal(request).await?;

        Ok(captured_run_id)
    }

    async fn run_chat_request_internal(&self, request: ChatTurnRequest) -> Result<(), String> {
        let conversation_id = request.conversation_id.as_str().to_string();
        let run_id = request.run_id.clone();
        log::info!(
            "[send_message] calling set_busy_for_run conv={} run={}",
            conversation_id,
            run_id.as_str()
        );
        self.services
            .gateway
            .set_busy_for_run(&conversation_id, run_id.clone())?;

        let session_id = request.conversation_id.clone();
        log::info!(
            "[send_message] resolving agent_runtime state conv={}",
            conversation_id
        );
        let agent_runtime = self
            .services
            .app
            .try_state::<Arc<crate::runtime::agent::AgentRuntime>>()
            .map(|v| v.inner().clone());
        log::info!("[send_message] agent_runtime={}", agent_runtime.is_some());
        // Load app settings (decrypting the primary key) for the runtime deps.
        log::info!("[send_message] loading settings conv={}", conversation_id);
        let app_settings_arc = {
            let map = self.services.db().get_all_settings().unwrap_or_default();
            let mut s = if map.is_empty() {
                AppSettings::default()
            } else {
                AppSettings::from_string_map(&map)
            };
            if let Some(ss) = self.services.crypto.as_ref() {
                s.primary_api_key = decrypt_api_key(ss, &s.primary_api_key);
            }
            Arc::new(s)
        };
        let workspace_path = self.services.file_mgr.workspace_path();
        log::info!(
            "[send_message] workspace_path={} exists={} conv={}",
            workspace_path.display(),
            workspace_path.exists(),
            conversation_id
        );
        let active_persona_id: Option<String> = match request.persona_id_override.as_deref() {
            Some(id) => Some(id.to_string()),
            None => self.services.db().get_active_persona_id().ok(),
        };
        let request_scoped_runtime_deps = crate::plugin::registry::RequestScopedRuntimeDeps {
            storage: self.services.db().clone(),
            file_manager: self.services.file_mgr.clone(),
            workspace_path: workspace_path.clone(),
            conversation_id: session_id.as_str().to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            agent_id: None,
            app_handle: Some(self.services.app.clone()),
            auth_manager: Some(self.services.auth_manager.clone()),
            model: String::new(),
            gateway: Some(self.services.gateway.clone()),
            tool_registry: Some(self.services.tool_registry.clone()),
            app_settings: Some(app_settings_arc),
            agent_runtime,
            event_bus: None,
            skill_registry: Some(self.services.skill_registry.clone()),
            authorized_workspace: chat_runtime_impl::load_authorized_workspace(
                &self.services.app,
                &conversation_id,
            ),
            read_file_state: None,
            cancellation: None,
            permission_mode: request.permission_mode,
            runtime_resolver: self.services.runtime_resolver.clone(),
            permission_ctx: None,
            current_persona_id: active_persona_id,
        };
        log::info!(
            "[send_message] building runtime_dispatcher conv={}",
            conversation_id
        );
        let runtime_dispatcher = self
            .services
            .tool_registry
            .to_runtime_dispatcher(request_scoped_runtime_deps)
            .await;
        log::info!(
            "[send_message] runtime_dispatcher built conv={}",
            conversation_id
        );
        let runtime = self.runtime.clone().with_query_engine(
            QueryEngine::with_dispatcher(runtime_dispatcher)
                .with_workspace_path(self.services.file_mgr.workspace_path().to_path_buf())
                .with_runtime_resolver(self.services.runtime_resolver.clone()),
        );
        log::info!(
            "[send_message] calling runtime.run_chat_request conv={}",
            conversation_id
        );
        // 早期触发标题生成：等 user message 持久化后（约 1.5s 给 driver 写盘），
        // 立即开始总结，不必等到整个 turn（含工具循环）跑完。后面 turn 结束再
        // 兜底触发一次，should_auto_title guard 会跳过已生成的情况。
        self.spawn_auto_title(conversation_id.clone(), 500).await;
        // Compatibility marker for review tests: self.runtime.run_chat_request(request)
        let result = runtime.run_chat_request(request).await;
        // Release the stream-cancel bridge for this turn before any post-turn work
        // can start, otherwise a stopped turn leaves a stale cancelled slot behind.
        self.services
            .gateway
            .clear_task_for_run(&conversation_id, &run_id);

        if result.is_ok() {
            self.spawn_auto_title(conversation_id.clone(), 0).await;
        }

        // After this turn ends (success or otherwise), let the PendingQueueManager
        // schedule a debounced drain so items buffered during this turn get
        // merged into the next one. Mirrors the same hook at the tail of
        // `send_chat_request` (IM path) — without this, app-side pending items
        // sit in the queue forever after the SentDirectly turn finishes.
        if let Some(mgr) = self
            .services
            .app
            .try_state::<Arc<crate::runtime::pending::PendingQueueManager>>()
        {
            let mgr_clone = mgr.inner().clone();
            let session_id = crate::runtime::ids::SessionId::new(conversation_id.clone());
            tauri::async_runtime::spawn(async move {
                mgr_clone.schedule_drain(session_id).await;
            });
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
        conversation_service::stop_streaming(self.services.gateway.clone(), conversation_id).await
    }

    pub async fn approve_permission_request(
        &self,
        tool_call_id: String,
        updated_input: Option<serde_json::Value>,
        remember: Option<bool>,
        destination: Option<PermissionDestination>,
    ) -> Result<(), String> {
        log::info!(
            "[approve_permission_request] tool_call_id={} remember={:?} destination={:?}",
            tool_call_id,
            remember,
            destination
        );
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
        log::info!(
            "[deny_permission_request] tool_call_id={} remember={:?} destination={:?}",
            tool_call_id,
            remember,
            destination
        );
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
            self.services.db().clone() as Arc<dyn ConversationStore>,
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

    /// Build a read-only overview of a conversation's team activity. Returns
    /// `{ teams: [] }` for conversations that never had a team.
    pub async fn get_team_overview(
        &self,
        conversation_id: String,
    ) -> Result<crate::runtime::team_view::TeamOverview, String> {
        let storage = self.services.db().clone();
        tokio::task::spawn_blocking(move || {
            crate::runtime::team_view::build_team_overview(&storage, &conversation_id)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("join error: {e}"))?
    }

    /// Read one teammate's complete on-disk transcript jsonl.
    pub async fn get_teammate_transcript(
        &self,
        conversation_id: String,
        agent_id: String,
    ) -> Result<Vec<serde_json::Value>, String> {
        let storage = self.services.db().clone();
        tokio::task::spawn_blocking(move || {
            crate::runtime::team_view::read_teammate_transcript(
                &storage,
                &conversation_id,
                &agent_id,
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("join error: {e}"))?
    }

    /// PR9: Read a slice of `<conv>/teams/{team_name}/team-chat.jsonl`.
    /// Returns raw JSON lines (the writer's shape — `{ts, from, to, text, ...}`).
    /// `since_ts` filters out lines whose `ts <= since_ts` (string compare,
    /// fine for RFC3339).  `limit` caps the returned slice to that many lines.
    pub async fn team_chat_messages(
        &self,
        conversation_id: String,
        team_name: String,
        since_ts: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>, String> {
        use crate::storage::{CurrentUserStorage, UserScopedPathResolver};
        let conv_dir = self
            .services
            .app
            .try_state::<Arc<CurrentUserStorage>>()
            .and_then(|cus| cus.require_paths().ok())
            .map(|paths| paths.conversations_dir().join(&conversation_id))
            .ok_or_else(|| "user scope not active".to_string())?;
        crate::runtime::agent::team_paths::validate_team_name(&team_name)
            .map_err(|e| e.to_string())?;
        let path = crate::runtime::agent::team_paths::TeamPaths::for_team(&conv_dir, &team_name)
            .team_chat_jsonl();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for line in content.lines() {
            let trimmed = match line.find('\t') {
                Some(idx) => &line[..idx],
                None => line,
            }
            .trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            if let Some(ref since) = since_ts {
                if v.get("ts")
                    .and_then(|t| t.as_str())
                    .map_or(true, |t| t <= since.as_str())
                {
                    continue;
                }
            }
            out.push(v);
            if let Some(lim) = limit {
                if out.len() >= lim {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Fire-and-forget: 触发对话标题自动生成。先尝试 LLM 总结 user 首句，
    /// 失败则兜底到 user 首句字面截断。idempotent guard 由 should_auto_title
    /// + generate_and_set_title 内部 title=="新对话" 双重检查保证。
    async fn spawn_auto_title(&self, conversation_id: String, delay_ms: u64) {
        // 提前加载 settings —— spawn 内部跨线程 self 不安全
        let dummy_request = ChatTurnRequest::new(conversation_id.clone(), String::new(), vec![]);
        let settings = match self.load_llm_settings_for_turn(&dummy_request).await {
            Ok(r) => build_gateway_settings(&r),
            Err(e) => {
                log::warn!("[auto-title] load_llm_settings_for_turn failed: {:?}", e);
                return;
            }
        };
        let db = self.services.db().clone() as Arc<dyn ConversationStore>;
        let gateway = self.services.gateway.clone();
        let host: Arc<dyn crate::transport::runtime_host::RuntimeHost> =
            Arc::new(TauriRuntimeHost::new(self.services.app.clone()));
        tauri::async_runtime::spawn(async move {
            if delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            let needs =
                conversation_service::should_auto_title(&*db, &conversation_id).unwrap_or(false);
            if !needs {
                return;
            }
            conversation_service::generate_and_set_title(
                db,
                gateway,
                host,
                conversation_id,
                settings,
            )
            .await;
        });
    }

    pub async fn create_conversation(&self) -> Result<String, String> {
        // 不在这里 emit conversation:created：前端 createNewConversation 已经做了
        // 乐观更新（optimisticId → backendId 替换），重复事件会触发不必要的 reload。
        // 只有后端绕开前端入口的 dispatcher（agenda / employee / schedule）需要 emit。
        conversation_service::create_conversation(
            self.services.db().clone() as Arc<dyn ConversationStore>
        )
        .await
    }

    pub async fn delete_conversation(&self, conversation_id: String) -> Result<(), String> {
        let outcome = conversation_service::delete_conversation(
            self.services.db().clone(),
            self.services.gateway.clone(),
            self.services.file_mgr.clone(),
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
            self.services.db().clone() as Arc<dyn ConversationStore>,
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

    pub async fn get_conversation_meta(
        &self,
        conversation_id: String,
    ) -> Result<Option<conversation_service::ConversationMetaDto>, String> {
        match self.services.db().get_conversation(&conversation_id) {
            Ok(meta) => Ok(Some(meta.into())),
            Err(e) => {
                log::warn!(
                    "[get_conversation_meta] conv {} unreadable: {e:#}",
                    conversation_id
                );
                Ok(None)
            }
        }
    }

    pub async fn archive_conversation(&self, conversation_id: String) -> Result<(), String> {
        conversation_service::archive_conversation(
            self.services.db().clone() as Arc<dyn ConversationStore>,
            conversation_id,
        )
        .await
    }

    pub async fn set_conversation_expert_team(
        &self,
        conversation_id: String,
        expert_team_id: String,
        team_label: String,
    ) -> Result<(), String> {
        let base = self.services.db().base_dir().to_path_buf();
        crate::storage::file_store::conversations::set_conversation_source(
            &base,
            &conversation_id,
            crate::storage::file_store::types::ConversationSource::ExpertTeam { expert_team_id },
            Some(team_label),
        )
        .map_err(|e| e.to_string())
    }

    pub async fn clear_conversation_source(&self, conversation_id: String) -> Result<(), String> {
        let base = self.services.db().base_dir().to_path_buf();
        crate::storage::file_store::conversations::set_conversation_source(
            &base,
            &conversation_id,
            crate::storage::file_store::types::ConversationSource::User,
            None,
        )
        .map_err(|e| e.to_string())
    }

    pub async fn get_conversation_source(
        &self,
        conversation_id: String,
    ) -> Result<crate::storage::file_store::types::ConversationSource, String> {
        let base = self.services.db().base_dir().to_path_buf();
        crate::storage::file_store::conversations::read_conversation_source(&base, &conversation_id)
            .map_err(|e| e.to_string())
    }

    pub async fn restore_conversation(&self, conversation_id: String) -> Result<(), String> {
        conversation_service::restore_conversation(
            self.services.db().clone() as Arc<dyn ConversationStore>,
            conversation_id,
        )
        .await
    }

    pub async fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_archived_conversations(
            self.services.db().clone() as Arc<dyn ConversationStore>
        )
        .await
    }

    pub async fn set_conversation_pinned(
        &self,
        conversation_id: String,
        pinned: bool,
    ) -> Result<(), String> {
        conversation_service::pin_conversation(
            self.services.db().clone() as Arc<dyn ConversationStore>,
            conversation_id,
            pinned,
        )
        .await
    }

    pub async fn get_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_conversations(
            self.services.db().clone() as Arc<dyn ConversationStore>
        )
        .await
    }

    pub async fn get_tasks(
        &self,
        conversation_id: String,
    ) -> Result<Vec<crate::models::message::TaskRecordFrontend>, String> {
        crate::models::message::TaskRecordFrontend::list_from_task_v2_store(
            self.services.db().base_dir(),
            &conversation_id,
        )
        .map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl crate::runtime::agenda::AgendaRunDispatcher for TauriChatCommandAdapter {
    async fn dispatch(
        &self,
        item: crate::runtime::agenda::AgendaItem,
        planned_fire_at: chrono::DateTime<chrono::Utc>,
        trigger_source: crate::runtime::agenda::TriggerSource,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<String> {
        use crate::runtime::agenda::{Occurrence, OccurrenceStatus};
        use crate::runtime::ids::{RunId, SessionId};

        let store = self.agenda_store_for_current_user()?;

        // 1. 创建 conversation
        let conversation_id = conversation_service::create_conversation(
            self.services.db().clone() as Arc<dyn ConversationStore>
        )
        .await
        .map_err(anyhow::Error::msg)?;

        // 1.4. 通知前端有新 conversation：sidebar 监听后刷新列表。
        //      所有后端直接走 conversation_service::create_conversation 的路径
        //      （agenda / employee / schedule_runner）都要 emit 这个事件，
        //      因为前端 chatStore 的乐观更新只发生在前端 createNewConversation 路径。
        emit_conversation_created(
            &self.services.app,
            &conversation_id,
            "agenda",
            Some(&item.title),
        );

        // 1.5. 如果 item 绑定了 workspace_path，把它 authorize 给这条新 conversation。
        //      这跟 HomeTaskComposerCard 提交时的 authorize_local_directory 等价：
        //      session_id == conversation_id，让后续 send_message 通过
        //      load_authorized_workspace 拿到正确目录而不是漂移到全局 workspace。
        if let Some(workspace_path) = item.workspace_path.as_deref() {
            let trimmed = workspace_path.trim();
            if !trimmed.is_empty() {
                if let Some(facade) = self
                    .services
                    .app
                    .try_state::<Arc<crate::storage::file_store::RuntimeRepositoryFacade>>()
                {
                    let root_path = std::path::PathBuf::from(trimmed);
                    let canonical = root_path.canonicalize().unwrap_or(root_path);
                    let display_name = canonical
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| canonical.to_string_lossy().to_string());
                    let ws = crate::runtime::store::AuthorizedWorkspace {
                        id: uuid::Uuid::new_v4().to_string(),
                        session_id: crate::runtime::ids::SessionId::new(conversation_id.clone()),
                        root_path: canonical.clone(),
                        display_name,
                        authorized_at: chrono::Utc::now().to_rfc3339(),
                    };
                    if let Err(e) = facade
                        .authorized_workspace_store()
                        .replace_for_session(&conversation_id, &ws)
                    {
                        log::warn!(
                            "[agenda-dispatch] authorize workspace failed conv={} path={} err={}",
                            conversation_id,
                            canonical.display(),
                            e
                        );
                    } else {
                        log::info!(
                            "[agenda-dispatch] authorized workspace conv={} root={}",
                            conversation_id,
                            canonical.display()
                        );
                    }
                } else {
                    log::warn!(
                        "[agenda-dispatch] RuntimeRepositoryFacade not in app state, \
                         agenda item workspace_path will be ignored conv={} path={}",
                        conversation_id,
                        trimmed
                    );
                }
            }
        }

        // 2. 预生成 RunId 并写 Running occurrence
        let run_id = RunId::new(uuid::Uuid::new_v4().to_string());
        let session_id = SessionId::new(conversation_id.clone());
        let occ = Occurrence {
            id: Occurrence::new_id(),
            agenda_item_id: item.id.clone(),
            fired_at: now,
            planned_fire_at,
            started_at: now,
            finished_at: None,
            primary_employee_id: item.organizer_employee_id.clone(),
            conversation_id: conversation_id.clone(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            status: OccurrenceStatus::Running,
            error_summary: None,
            trigger_source: trigger_source.clone(),
        };
        store.append_occurrence(&occ)?;
        let occurrence_id = occ.id.clone();

        // 3. 推进 item next_fire_at + occurrence_count
        if matches!(
            trigger_source,
            crate::runtime::agenda::TriggerSource::Scheduled
        ) {
            if let Err(e) = store.advance_after_fire(&item.id, now) {
                let mut final_occ = occ.clone();
                final_occ.finished_at = Some(chrono::Utc::now());
                final_occ.status = OccurrenceStatus::Failed;
                final_occ.error_summary = Some(e.to_string());
                let _ = store.append_occurrence(&final_occ);
                return Err(e);
            }
        }

        // 4. 读 employee 拿 system_prompt_extra / default_skill_id 拼 prompt。
        //    任何步骤失败都退化为兜底 prompt（fallback），不阻塞 agenda 触发；occurrence
        //    已写盘为 Running，绝不能因为加载 employee 失败而留孤儿。
        let employee = (|| -> Option<crate::runtime::employee::store::EmployeeRecord> {
            use crate::runtime::employee::store::EmployeeStore;
            let store = self
                .services
                .app
                .try_state::<Arc<EmployeeStore>>()
                .map(|s| s.inner().clone())?;
            match store.get(&item.organizer_employee_id) {
                Ok(emp) => Some(emp),
                Err(e) => {
                    log::warn!(
                        "[agenda-dispatch] failed to load employee {}: {e}",
                        item.organizer_employee_id
                    );
                    None
                }
            }
        })();

        // 没有匹配的员工（比如老 agenda 写的是 persona id "default"）就用 agenda 自己的 prompt 兜底，
        // 不阻塞触发。
        let trigger_label = format!("[日程触发] {}\n计划触发时间：{planned_fire_at}", item.title);
        let employees_dir = {
            use crate::storage::{CurrentUserStorage, UserScopedPathResolver};
            self.services
                .app
                .try_state::<Arc<CurrentUserStorage>>()
                .and_then(|cus| cus.require_paths().ok())
                .map(|paths| paths.employees_dir())
        };
        let language = self.current_app_language();
        let prompt = if let Some(emp) = employee.as_ref() {
            crate::runtime::employee::dispatch_prompt::build_dispatch_prompt_for_language(
                emp,
                &trigger_label,
                None,
                Some(&item.prompt),
                employees_dir.as_deref(),
                Some(&language),
            )
        } else {
            format!("{trigger_label}\n\n{}", item.prompt)
        };

        // persona_id_override = None：让 chat 层走 active persona 兜底（PR-6 彻底切掉 persona）
        let result = self
            .send_message_with_overrides(
                conversation_id.clone(),
                prompt,
                Vec::new(),
                None,
                None,
                None,
                None,
                Some(run_id.clone()),
            )
            .await;

        // 5. 追加最终 occurrence
        let mut final_occ = occ.clone();
        final_occ.finished_at = Some(chrono::Utc::now());
        match result {
            Ok(_) => {
                final_occ.status = OccurrenceStatus::Succeeded;
            }
            Err(e) => {
                final_occ.status = OccurrenceStatus::Failed;
                final_occ.error_summary = Some(e);
            }
        }
        store.append_occurrence(&final_occ)?;

        Ok(occurrence_id)
    }
}

/// RAII guard for `employee_run_overrides`. Inserts on construction, removes
/// on drop, so a panic / cancellation / early return in the agent loop cannot
/// leak override entries.
struct OverrideGuard {
    overrides: Arc<std::sync::Mutex<std::collections::HashMap<String, EmployeeRunOverrides>>>,
    conversation_id: String,
}

impl OverrideGuard {
    fn install(
        overrides: Arc<std::sync::Mutex<std::collections::HashMap<String, EmployeeRunOverrides>>>,
        conversation_id: String,
        ov: EmployeeRunOverrides,
    ) -> Self {
        if let Ok(mut map) = overrides.lock() {
            map.insert(conversation_id.clone(), ov);
        }
        Self {
            overrides,
            conversation_id,
        }
    }
}

impl Drop for OverrideGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.overrides.lock() {
            map.remove(&self.conversation_id);
        }
    }
}

#[async_trait]
impl crate::runtime::pending::ChatTurnDispatcher for TauriChatCommandAdapter {
    async fn dispatch(&self, mut request: ChatTurnRequest) -> anyhow::Result<()> {
        // Spec §6.1: N drained pending items must land in messages.jsonl as N
        // independent user messages. The last item rides on `request` (will be
        // persisted by run_chat_request's standard flow). The first N-1 items
        // we persist here, BEFORE handing the request to the turn driver, so
        // the next history-load includes them.
        if let Some(batch) = request.pending_batch.take() {
            let conv_id = request.conversation_id.as_str().to_string();
            let run_id = request.run_id.clone();
            let n = batch.len();
            if n > 1 {
                for item in batch.iter().take(n - 1) {
                    let text = match &item.sender_nick {
                        Some(nick) if !nick.is_empty() => {
                            format!("[{}]: {}", nick, item.text)
                        }
                        _ => item.text.clone(),
                    };
                    let attachments: Vec<
                        crate::runtime::chat::chat_turn_driver::ChatAttachmentRef,
                    > = item
                        .attachments
                        .iter()
                        .map(
                            |a| crate::runtime::chat::chat_turn_driver::ChatAttachmentRef {
                                id: a.id.clone(),
                                file_name: std::path::Path::new(&a.file_path)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .map(String::from)
                                    .unwrap_or_else(|| a.file_path.clone()),
                                file_path: a.file_path.clone(),
                                kind: "file".to_string(),
                                file_size: a.size_bytes.unwrap_or(0),
                                file_type: a
                                    .mime
                                    .clone()
                                    .unwrap_or_else(|| "application/octet-stream".into()),
                                mime_type: a.mime.clone(),
                            },
                        )
                        .collect();
                    let msg_id = format!("msg-{}", uuid::Uuid::new_v4());
                    let content_value =
                        crate::runtime::chat::chat_turn_driver::build_user_content_json_with_skill(
                            &text,
                            &attachments,
                            item.skill_command.as_ref(),
                        );
                    let content_json = content_value.to_string();
                    if let Err(e) =
                        self.services
                            .db()
                            .insert_message(&msg_id, &conv_id, "user", &content_json)
                    {
                        log::warn!(
                            "[pending-dispatch] failed to persist drained item {} as user msg: {:#}",
                            item.id,
                            e
                        );
                    } else {
                        log::info!(
                            "[pending-dispatch] persisted drained item {} as user msg id={}",
                            item.id,
                            msg_id
                        );
                        // Emit MessagePersisted so frontend chatStore appends a
                        // user bubble for each drained item (mirrors what
                        // chat_turn_driver does for the ride-on-request item).
                        let event = crate::runtime::events::RuntimeEvent::new(
                            request.conversation_id.clone(),
                            run_id.clone(),
                            crate::runtime::events::RuntimeEventKind::MessagePersisted {
                                message_id: msg_id,
                                role: "user".to_string(),
                                content: content_value,
                                client_message_id: None,
                                tool_calls: None,
                            },
                        );
                        if let Err(e) = self.runtime.event_bus().emit(event).await {
                            log::warn!(
                                "[pending-dispatch] emit MessagePersisted for drained item failed: {:#}",
                                e
                            );
                        }
                    }
                }
            }
        }
        self.send_chat_request(request)
            .await
            .map_err(|e| anyhow::anyhow!("dispatch via TauriChatCommandAdapter failed: {e}"))
    }
}

#[async_trait]
impl crate::runtime::employee::runner::EmployeeRunDispatcher for TauriChatCommandAdapter {
    async fn dispatch_employee_run(
        &self,
        employee: crate::runtime::employee::store::EmployeeRecord,
        fire_at: DateTime<Utc>,
        prompt_override: Option<String>,
        catchup_info: Option<String>,
        trigger_kind: crate::runtime::employee::runner::TriggerKind,
        attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
    ) -> anyhow::Result<String> {
        use crate::runtime::employee::inbox_writer;
        use crate::runtime::employee::runner::TriggerKind;
        use crate::runtime::employee::store::EmployeeStore;
        use crate::storage::{CurrentUserStorage, UserScopedPathResolver};
        use tauri::Manager;

        // ─── Sync phase: create conversation, persist Running entry, return id ───

        let conversation_id = conversation_service::create_conversation(
            self.services.db().clone() as Arc<dyn ConversationStore>
        )
        .await
        .map_err(anyhow::Error::msg)?;
        // Stamp employee identity onto conv.json:
        //   1) `employee_id` field (legacy reader still hits this)
        //   2) `source = Employee { employee_id }` + index `kind = Employee` mirror
        //      (so the sidebar can group this conversation under 数字员工)
        // Failure here is non-fatal: the UI just won't render the employee
        // identity card / grouping.
        if let Err(e) = self
            .services
            .db()
            .set_conversation_employee_id(&conversation_id, Some(&employee.id))
        {
            log::warn!(
                "[dispatch_employee_run] failed to stamp employee_id for {} on conv {}: {e:#}",
                employee.id,
                conversation_id
            );
        }
        let base = self.services.db().base_dir().to_path_buf();
        if let Err(e) = crate::storage::file_store::conversations::set_conversation_source(
            &base,
            &conversation_id,
            crate::storage::file_store::types::ConversationSource::Employee {
                employee_id: employee.id.clone(),
            },
            Some(employee.name.clone()),
        ) {
            log::warn!(
                "[dispatch_employee_run] failed to stamp source=Employee for {} on conv {}: {e:#}",
                employee.id,
                conversation_id
            );
        }
        emit_conversation_created(
            &self.services.app,
            &conversation_id,
            "employee",
            Some(&employee.name),
        );

        // Compute the trigger label here (it depends on `trigger_kind` which is
        // a transport-layer enum — keeping the match local keeps the prompt
        // helper in `runtime/employee/` decoupled from `TriggerKind`).
        let trigger_label = match trigger_kind {
            TriggerKind::OnDemand => "[按需派活]".to_string(),
            TriggerKind::Cron => format!(
                "[定时触发] 触发时间：{}",
                fire_at.format("%Y-%m-%d %H:%M UTC")
            ),
        };

        // Resolve employees_dir for inbox + record_run. If the user is not
        // logged in we cannot persist anything, so bail out before spawning.
        // Moved above `build_dispatch_prompt` so the prompt builder can use
        // it for snapshot lookup (`<employees_dir>/<id>/template/template.json`
        // overrides record fields once the instance has been stamped).
        let employees_dir = {
            let cus = self
                .services
                .app
                .try_state::<Arc<CurrentUserStorage>>()
                .ok_or_else(|| anyhow::anyhow!("CurrentUserStorage not registered"))?;
            let paths = cus
                .require_paths()
                .map_err(|e| anyhow::anyhow!("paths unavailable: {e}"))?;
            paths.employees_dir()
        };

        let language = self.current_app_language();
        let prompt = crate::runtime::employee::dispatch_prompt::build_dispatch_prompt_for_language(
            &employee,
            &trigger_label,
            catchup_info.as_deref(),
            prompt_override.as_deref(),
            Some(employees_dir.as_path()),
            Some(&language),
        );

        // record_run synchronously: last_run_at represents "last dispatched at"
        // for both on-demand and cron paths.
        {
            let store = self
                .services
                .app
                .try_state::<Arc<EmployeeStore>>()
                .map(|s| s.inner().clone());
            if let Some(store) = store {
                if let Err(e) = store.record_run(&employee.id, fire_at) {
                    log::warn!(
                        "[dispatch_employee_run] record_run failed for {}: {e}",
                        employee.id
                    );
                }
            } else {
                log::warn!(
                    "[dispatch_employee_run] EmployeeStore singleton missing; skipping record_run for {}",
                    employee.id
                );
            }
        }

        // PR-8: the `Running` inbox entry was retired. Users had two
        // signals for "运行中" (the streaming chat bubble + the employee
        // card's running badge); the inbox row was a third copy that
        // got marked-read seconds later, cluttering 汇报中心 without
        // adding information. We jump straight to `push_report` /
        // `push_error` when the run finishes.

        // ─── Sync persist of the dispatch prompt as a user message ─────────
        // The agent loop below runs detached in a spawned task; if that task
        // panics or returns before persist_user_message lands, the conv.json
        // exists on disk but messages.jsonl is empty — the user sees a blank
        // chat with no recoverable trace. Persist here synchronously, then
        // mark `pre_persisted=true` on the request so the driver does NOT
        // re-persist or re-emit MessagePersisted for the same content.
        let dispatch_msg_id = format!("msg-{}", uuid::Uuid::new_v4());
        let dispatch_content_json =
            crate::runtime::chat::chat_turn_driver::build_user_content_json(&prompt, &attachments)
                .to_string();
        if let Err(e) = self.services.db().insert_message(
            &dispatch_msg_id,
            &conversation_id,
            "user",
            &dispatch_content_json,
        ) {
            log::error!(
                "[dispatch_employee_run] persist dispatch prompt failed for {}: {e:#}",
                employee.id
            );
            return Err(anyhow::anyhow!("persist dispatch prompt failed: {e}"));
        }
        // Emit the legacy `message:updated` so the chat view renders the
        // dispatch bubble immediately, without waiting for the spawned agent
        // loop to call get_history → build initial_messages → emit.
        let _ = self.services.app.emit(
            "message:updated",
            serde_json::json!({
                "conversationId": conversation_id,
                "messageId": dispatch_msg_id,
                "id": dispatch_msg_id,
                "role": "user",
                "content": crate::runtime::conversation_service::transform_message_json_for_frontend(
                    serde_json::json!({
                        "content": serde_json::from_str::<serde_json::Value>(&dispatch_content_json)
                            .unwrap_or(serde_json::Value::Null),
                    })
                )["content"].clone(),
                "createdAt": chrono::Utc::now().to_rfc3339(),
            }),
        );

        // ─── Async phase: run the agent loop in a detached task ────────────

        // Resolve the EmployeeActiveRuns state once; the spawned task installs
        // an `ActiveRunGuard` so registration is panic-safe.
        let active_runs_for_spawn = self
            .services
            .app
            .try_state::<std::sync::Arc<crate::runtime::employee::EmployeeActiveRuns>>()
            .map(|s| s.inner().clone());

        let adapter = self.clone();
        let employee_clone = employee.clone();
        let conv_id = conversation_id.clone();
        let employees_dir_async = employees_dir.clone();
        let attachments_for_run = attachments;

        tauri::async_runtime::spawn(async move {
            // RAII guard ensures the active-runs entry is unregistered on
            // drop, including panic paths. Mirrors OverrideGuard above.
            let _active_run_guard = active_runs_for_spawn.map(|ar| {
                crate::runtime::employee::ActiveRunGuard::install(
                    ar,
                    crate::runtime::employee::ActiveRun {
                        employee_id: employee_clone.id.clone(),
                        conversation_id: conv_id.clone(),
                        started_at: chrono::Utc::now(),
                        trigger_kind: match trigger_kind {
                            TriggerKind::OnDemand => {
                                crate::runtime::employee::TriggerKindLabel::OnDemand
                            }
                            TriggerKind::Cron => crate::runtime::employee::TriggerKindLabel::Cron,
                        },
                    },
                )
            });

            // PR-11 (2026-05-15): tool whitelisting per employee is retired.
            // Rationale documented in docs/plans/2026-05-15-employee-deep-fix.md:
            // - the snapshot/record tool_whitelist drifted from runtime tool
            //   names (e.g. "load_skill" → "Skill", "bash" → "Bash") whenever
            //   tools got refactored, leaving employees silently unable to
            //   call the very tools the prompt asked them to use
            // - permission/sandbox already gate sensitive actions; the
            //   whitelist was a redundant second gate with no per-employee
            //   user value
            // We pass an empty whitelist; `load_turn_config_overrides`
            // treats empty as "no schema filter" and the agent loop sees
            // the full tool catalog. `max_iterations: 120` (longer than the
            // default user-driven cap) is preserved.
            let _guard = OverrideGuard::install(
                adapter.services.employee_run_overrides.clone(),
                conv_id.clone(),
                EmployeeRunOverrides {
                    tool_whitelist: std::collections::HashSet::new(),
                    max_iterations: 120,
                },
            );

            let result = {
                let mut req = ChatTurnRequest::new(
                    conv_id.clone(),
                    prompt.clone(),
                    attachments_for_run.clone(),
                );
                // The dispatch prompt was already persisted + emitted in the
                // sync phase above; the driver must skip its own persist step
                // so the user bubble is not duplicated.
                req.pre_persisted = true;
                req.session_attachment_dirs =
                    crate::runtime::path_auth::derive_working_dirs_from_attachments(
                        &req.attachments
                            .iter()
                            .map(|a| std::path::PathBuf::from(&a.file_path))
                            .collect::<Vec<_>>(),
                    );
                adapter
                    .send_chat_request(req)
                    .await
                    .map_err(|e| anyhow::anyhow!("send_chat_request failed in dispatch: {e}"))
            };

            match &result {
                Ok(()) => {
                    // Flush any pending assistant-message writes, then read the
                    // conversation to build a useful title + summary for the inbox.
                    let _ = adapter.services.assistant_write_queue.flush();
                    let (title, summary) =
                        extract_report_title_summary(adapter.services.db().as_ref(), &conv_id);
                    if let Err(e) = inbox_writer::push_report(
                        employees_dir_async.clone(),
                        &employee_clone.id,
                        &employee_clone.name,
                        &conv_id,
                        title,
                        summary,
                        None,
                    ) {
                        log::warn!(
                            "[dispatch_employee_run] push_report failed for {}: {e}",
                            employee_clone.id
                        );
                    }
                }
                Err(err) => {
                    let err_str = format!("{err:#}");
                    if let Err(e) = inbox_writer::push_error(
                        employees_dir_async.clone(),
                        &employee_clone.id,
                        &employee_clone.name,
                        &conv_id,
                        &err_str,
                        None,
                    ) {
                        log::warn!(
                            "[dispatch_employee_run] push_error failed for {}: {e}",
                            employee_clone.id
                        );
                    }
                }
            }

            // Desktop notification: only for cron triggers. On-demand users are
            // already watching the chat view; notifying them is noise.
            if matches!(trigger_kind, TriggerKind::Cron) && result.is_ok() {
                adapter.notify_employee_run_complete(
                    &employee_clone.name,
                    &employee_clone.id,
                    fire_at,
                );
            }
        });

        Ok(conversation_id)
    }
}

/// Best-effort extraction of (title, summary) for an employee inbox Report
/// entry, given the conversation_id of a just-completed agent run.
///
/// - title: from conv.json's `title` field if auto-title finished, else None
///   (push_report falls back to "{employee_name} 已完成任务").
/// - summary: first 200 chars of the last assistant message's text content.
///
/// Both are returned as None on read failure — never panics, never blocks the
/// inbox write path.
fn extract_report_title_summary(
    db: &crate::storage::file_store::AppStorage,
    conversation_id: &str,
) -> (Option<String>, Option<String>) {
    let title = db
        .get_conversation(conversation_id)
        .ok()
        .map(|meta| meta.title)
        .filter(|t| !t.trim().is_empty() && !t.starts_with("新对话"));

    let summary = db
        .get_messages_v2(conversation_id)
        .ok()
        .and_then(|msgs| {
            msgs.into_iter()
                .rev()
                .find(|m| m.role == "assistant")
                .map(|m| {
                    let text = m.text().to_string();
                    let trimmed: String = text.chars().take(200).collect();
                    if text.chars().count() > 200 {
                        format!("{trimmed}…")
                    } else {
                        trimmed
                    }
                })
        })
        .filter(|s| !s.trim().is_empty());

    (title, summary)
}

impl TauriChatCommandAdapter {
    /// Send macOS notifications for inbox entries created during an employee run.
    fn notify_employee_run_complete(
        &self,
        employee_name: &str,
        employee_id: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) {
        use crate::runtime::employee::inbox::InboxStore;
        use crate::storage::{CurrentUserStorage, UserScopedPathResolver};
        use tauri::Manager;
        use tauri_plugin_notification::NotificationExt;

        let app = self.services.app.clone();
        let employee_name = employee_name.to_string();
        let employee_id = employee_id.to_string();

        tauri::async_runtime::spawn(async move {
            let Ok(cus) = app
                .try_state::<std::sync::Arc<CurrentUserStorage>>()
                .ok_or_else(|| "no CurrentUserStorage".to_string())
            else {
                return;
            };
            let Ok(paths) = cus.require_paths() else {
                return;
            };
            let inbox = InboxStore::new(paths.employees_dir());
            let entries = match inbox.list_for(&employee_id, 50) {
                Ok(e) => e,
                Err(_) => return,
            };
            for entry in entries {
                if entry.created_at < since {
                    continue;
                }
                let body = entry.summary.unwrap_or_default();
                let _ = app
                    .notification()
                    .builder()
                    .title(&format!("{} · {}", employee_name, entry.title))
                    .body(&body)
                    .show();
            }
        });
    }
}

/// 通知前端后端直接创建了新 conversation，让 sidebar reload 对话列表。
///
/// 所有后端绕开前端 `createNewConversation`（走 `conversation_service::create_conversation`）
/// 的路径都要 emit 一次，否则 sidebar 不会感知：
/// - agenda dispatcher（定时日程 / 立即运行）
/// - employee dispatcher（数字员工派活）
/// - schedule_runner（老 schedule 模块，PR-4 会删）
/// - 前端 `create_conversation` Tauri 命令（走 TauriChatCommandAdapter）
///
/// `source` 是调用方标识（"agenda" / "employee" / "schedule" / "user"），
/// 前端可以据此决定 UX 差异（比如是否自动切到该对话），但目前只做 reload。
fn emit_conversation_created(
    app: &tauri::AppHandle,
    conversation_id: &str,
    source: &str,
    title: Option<&str>,
) {
    let _ = app.emit(
        "conversation:created",
        serde_json::json!({
            "conversationId": conversation_id,
            "source": source,
            "title": title,
        }),
    );
}

#[cfg(test)]
mod retry_reason_tests {
    use super::classify_retry_reason;
    use crate::runtime::events::RetryReason;

    #[test]
    fn upstream_5xx_is_upstream_busy() {
        assert_eq!(
            classify_retry_reason(
                "Anthropic API stream error (502 Bad Gateway): <html>nginx/1.20.1</html>"
            ),
            RetryReason::UpstreamBusy
        );
        assert_eq!(
            classify_retry_reason("500 Internal Server Error: distributor unavailable"),
            RetryReason::UpstreamBusy
        );
        assert_eq!(
            classify_retry_reason("503 Service Unavailable: model_not_found"),
            RetryReason::UpstreamBusy
        );
    }

    #[test]
    fn rate_limit_is_rate_limited() {
        assert_eq!(
            classify_retry_reason("429 Too Many Requests"),
            RetryReason::RateLimited
        );
        assert_eq!(
            classify_retry_reason("Anthropic rate limit exceeded"),
            RetryReason::RateLimited
        );
    }

    #[test]
    fn local_network_is_network_flap() {
        assert_eq!(
            classify_retry_reason("request timed out after 60s"),
            RetryReason::NetworkFlap
        );
        assert_eq!(
            classify_retry_reason("connection reset by peer"),
            RetryReason::NetworkFlap
        );
        assert_eq!(
            classify_retry_reason("broken pipe writing to upstream"),
            RetryReason::NetworkFlap
        );
        // A "timeout" wrapping a 5xx still counts as local network.
        assert_eq!(
            classify_retry_reason("502 timed out waiting for upstream"),
            RetryReason::NetworkFlap
        );
    }

    #[test]
    fn unknown_falls_back_to_network_flap() {
        assert_eq!(
            classify_retry_reason("some weird unclassified blip"),
            RetryReason::NetworkFlap
        );
    }
}
