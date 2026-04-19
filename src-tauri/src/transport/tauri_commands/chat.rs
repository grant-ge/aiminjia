// Legacy Tauri command layer: constructs PluginContext to bootstrap tool execution.
// Suppress the deprecation lint here; this is the entry-point bridge between
// Tauri commands and the legacy PluginContext-based tool chain.
// Migrate to CapabilityContext when the command layer is refactored.
#![allow(deprecated)]
use std::sync::Arc;

use async_trait::async_trait;
use tauri::{Emitter, Manager};

use crate::auth::AuthManager;
use crate::llm::gateway::LlmGateway;
use crate::llm::prompt_guard;
use crate::llm::prompts;
use crate::models::message::SubAgentTranscriptEntryFrontend;
use crate::models::settings::AppSettings;
use crate::plugin::skill_trait::ToolFilter;
use crate::plugin::ToolRegistry;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::conversation_service;
use crate::runtime::ids::{SessionId, ToolCallId};
use crate::runtime::store::PendingPermissionResolution;
use crate::runtime::store::conversation_store::ConversationStore;
use crate::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, SessionRuntime,
};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::{
    LlmStepInput, LlmStepResult, ResolvedLlmSettings, RuntimeLlmExecutor, TurnConfig,
    TurnError, TurnIterationState,
};
use crate::transport::tauri_event_adapter::TauriEventAdapter;
use crate::transport::tauri_runtime_host::TauriRuntimeHost;
use crate::storage::crypto::SecureStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

mod chat_runtime_impl;

pub(crate) use chat_runtime_impl::build_visible_tool_defs;

/// Maximum number of stream-level retries within the agent loop.
/// When a stream error or gateway error is retryable (5xx, timeout, connection),
/// the current iteration is retried instead of aborting the entire agent loop.
const MAX_STREAM_RETRIES: u32 = 2;

/// Delay before retrying a failed stream (seconds).
const STREAM_RETRY_DELAY_SECS: u64 = 2;

fn resolve_request_is_analysis(db: &AppStorage, conversation_id: &str) -> bool {
    matches!(
        db.get_conversation_mode(conversation_id).ok().as_deref(),
        Some("confirming" | "analyzing")
    )
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

#[derive(Clone)]
struct TauriChatServices {
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    crypto: Option<Arc<SecureStorage>>,
    tool_registry: Arc<ToolRegistry>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    auth_manager: Arc<AuthManager>,
    app: tauri::AppHandle,
}

struct TauriLegacyTurnExecutor {
    services: TauriChatServices,
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
        use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
        use crate::runtime::ids::{RunId, SessionId};
        use futures::StreamExt;

        let session_id = SessionId::from(input.conversation_id);
        let run_id = RunId::from(input.run_id);

        let settings = build_gateway_settings(input.llm_settings);

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

    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        let settings_map = self.services.db.get_all_settings().unwrap_or_default();
        let mut settings = if settings_map.is_empty() {
            AppSettings::default()
        } else {
            AppSettings::from_string_map(&settings_map)
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
            use_cloud: settings.use_cloud,
            cloud_model: settings.cloud_model,
            cloud_model_type: settings.cloud_model_type,
        })
    }

    async fn run_precompute(
        &self,
        _config: &TurnConfig,
        _state: &mut TurnIterationState,
    ) -> Result<Option<String>, TurnError> {
        // TODO(S4-T12/future): full precompute requires StepConfig.precompute, which is not yet
        // carried by TurnConfig. TurnConfig only has is_analysis:bool, not the full StepConfig.
        // When S4-T13 wires the driver loop, TurnConfig should be extended with:
        //   pub step_config: Option<StepConfig>,
        // at which point this body can be extracted from chat_runtime_impl.rs Block 6 (L1795-L2034).
        // For daily mode (is_analysis == false) Ok(None) is always correct.
        // For analysis mode without the step_config, falling back to no-precompute is safe
        // (the agent will do the computation itself in the tool loop).
        Ok(None)
    }

    async fn persist_user_message(
        &self,
        conversation_id: &str,
        content: &str,
        file_ids: &[String],
    ) -> Result<String, TurnError> {
        let msg_id = format!("msg-{}", uuid::Uuid::new_v4());

        // Build the same content_json structure as legacy_send_message_impl
        let content_json = if file_ids.is_empty() {
            serde_json::json!({ "text": content }).to_string()
        } else {
            let files_meta: Vec<serde_json::Value> = file_ids
                .iter()
                .map(|id| serde_json::json!({ "id": id }))
                .collect();
            serde_json::json!({
                "text": content,
                "files": files_meta,
            })
            .to_string()
        };

        if let Err(e) = self
            .services
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

    async fn persist_assistant_message(
        &self,
        conversation_id: &str,
        content: &str,
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
        if self.services.db.get_conversation_mode(conversation_id).is_err() {
            log::warn!(
                "[persist_assistant_message] Conversation {} deleted during agent run, skipping save",
                conversation_id
            );
            return Ok(message_id);
        }

        let workspace_path = self.services.file_mgr.workspace_path();

        // --- Build content JSON, attaching generated files when present ---
        let content_value = if !generated_file_ids.is_empty() {
            match self.services.db.get_generated_files_by_ids(generated_file_ids) {
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
                    serde_json::json!({
                        "text": filtered_content,
                        "generatedFiles": gen_files,
                    })
                }
                Ok(_) => serde_json::json!({ "text": filtered_content }),
                Err(e) => {
                    log::error!(
                        "[persist_assistant_message] Failed to query generated files: {:#}",
                        e
                    );
                    serde_json::json!({ "text": filtered_content })
                }
            }
        } else {
            serde_json::json!({ "text": filtered_content })
        };

        // --- Persist to AppStorage ---
        let content_json = content_value.to_string();
        log::info!(
            "[persist_assistant_message] Saving id={} conv={} content_len={}",
            message_id,
            conversation_id,
            content_json.len()
        );
        match self.services.db.insert_message(
            &message_id,
            conversation_id,
            "assistant",
            &content_json,
        ) {
            Ok(_) => {
                log::info!(
                    "[persist_assistant_message] Message saved successfully id={}",
                    message_id
                );
            }
            Err(e) => {
                log::error!(
                    "[persist_assistant_message] Failed to save message to DB: {:#}",
                    e
                );
                return Err(TurnError::PersistenceError(format!(
                    "Failed to save assistant message: {}",
                    e
                )));
            }
        }

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
        // TODO(S4-T12/future): full finalize_step requires StepConfig.step (u32), which is not yet
        // carried by TurnConfig. TurnConfig only has is_analysis:bool.
        // When S4-T13 wires the driver loop, TurnConfig should be extended with:
        //   pub step_config: Option<StepConfig>,
        // at which point this body can be extracted from chat_runtime_impl.rs Block 34 (L3300-L3372):
        //   - auto_capture_step_context(&db, &conversation_id, step_num, &messages)
        //   - validate analysis notes (warn if missing)
        //   - orchestrator::advance_step(&db, &conversation_id, step_num, "completed")
        // For daily mode (is_analysis == false) the no-op is always correct.
        if !config.is_analysis {
            return Ok(());
        }
        // analysis mode: log that we skipped finalize (until step_config is available)
        log::debug!(
            "[finalize_step] Analysis mode but step_config not in TurnConfig — skipping \
             auto_capture and advance_step for conv={}. full_content_len={}",
            config.conversation_id,
            state.full_content.len()
        );
        Ok(())
    }

    /// 构建 Turn 级的 system prompt。
    /// 精确移植 agent_loop Block 4 的逻辑：
    ///   - 从 DB 读取 active persona
    ///   - 从 auth_manager 获取 product_name（租户品牌名）
    ///   - 根据 is_analysis 路由 PromptMode::Daily / PromptMode::Analysis
    async fn build_system_prompt(
        &self,
        _conversation_id: &str,
        is_analysis: bool,
    ) -> Result<String, TurnError> {
        let persona = self.services.db.get_active_persona().ok();

        let product_name: Option<String> = self
            .services
            .auth_manager
            .get_auth_info()
            .await
            .tenant
            .and_then(|t| t.product_name.filter(|n| !n.is_empty()));

        let mode = if is_analysis {
            prompts::PromptMode::Analysis
        } else {
            prompts::PromptMode::Daily
        };

        let parts = prompts::build_system_prompt_parts(
            mode,
            persona.as_ref(),
            product_name.as_deref(),
        );
        let prompt = if parts.dynamic_section.is_empty() {
            parts.static_section
        } else {
            format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
        };

        log::info!(
            "[build_system_prompt] mode={:?} len={} persona={} product_name={}",
            mode,
            prompt.len(),
            persona.as_ref().map(|p| p.identity.as_str()).unwrap_or("(none)"),
            product_name.as_deref().unwrap_or("(none)"),
        );

        Ok(prompt)
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
        let authorized_workspace = chat_runtime_impl::load_authorized_workspace(
            &self.services.app,
            conversation_id,
        );
        Ok(chat_runtime_impl::build_llm_content(
            content,
            &file_attachments,
            authorized_workspace.is_some(),
        ))
    }

    async fn get_tool_defs(
        &self,
        is_analysis: bool,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;

        let filter = if is_analysis {
            ToolFilter::All
        } else {
            ToolFilter::Only(
                DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect()
            )
        };

        let tool_definitions: Vec<crate::llm::streaming::ToolDefinition> = self.services.tool_registry
            .get_schemas_filtered(&filter)
            .await;

        // ToolDefinition implements Serialize
        let json_defs: Vec<serde_json::Value> = tool_definitions
            .into_iter()
            .filter_map(|td| {
                serde_json::to_value(&td)
                    .map_err(|e| log::warn!("[get_tool_defs] Failed to serialize tool '{}': {}", td.name, e))
                    .ok()
            })
            .collect();

        log::info!(
            "[get_tool_defs] is_analysis={} returned {} tool definitions",
            is_analysis,
            json_defs.len(),
        );

        Ok(json_defs)
    }

    async fn load_history(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        const HISTORY_LIMIT: u32 = 50;

        let raw_messages = self.services.db
            .get_recent_messages(conversation_id, HISTORY_LIMIT)
            .map_err(|e| TurnError::PersistenceError(format!(
                "Failed to load conversation history: {}", e
            )))?;
        let has_authorized_workspace = chat_runtime_impl::load_authorized_workspace(
            &self.services.app,
            conversation_id,
        )
        .is_some();

        // 转换格式：content 可能是 {"text": "...", "files": [...]} 或直接字符串
        let chat_messages: Vec<serde_json::Value> = raw_messages
            .into_iter()
            .filter_map(|msg| {
                let role = msg["role"].as_str()?.to_string();
                let content = build_history_message_content(
                    &role,
                    msg.get("content")?,
                    has_authorized_workspace,
                )?;
                if content.trim().is_empty() {
                    return None;
                }
                Some(serde_json::json!({
                    "role": role,
                    "content": content,
                }))
            })
            .collect();

        log::info!(
            "[load_history] conv={} loaded {} messages (limit={})",
            conversation_id,
            chat_messages.len(),
            HISTORY_LIMIT,
        );

        Ok(chat_messages)
    }

    async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
        use crate::runtime::chat::context_builder::build_env_info;

        let workspace_path = self.services.file_mgr.workspace_path().to_path_buf();

        let authorized = chat_runtime_impl::load_authorized_workspace(
            &self.services.app,
            conversation_id,
        );
        let authorized_tuple = authorized.as_ref().map(|aw| {
            (
                aw.root_path.to_string_lossy().into_owned(),
                aw.display_name.clone(),
            )
        });
        let authorized_ref = authorized_tuple
            .as_ref()
            .map(|(p, n)| (p.as_str(), n.as_str()));

        let env_info = build_env_info(&workspace_path, authorized_ref).await;

        log::info!(
            "[get_env_info] conv={} workspace={} authorized={} env_info_len={}",
            conversation_id,
            workspace_path.display(),
            authorized.is_some(),
            env_info.len()
        );

        Ok(env_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_storage() -> (AppStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = AppStorage::new(dir.path()).unwrap();
        (storage, dir)
    }

    #[test]
    fn resolve_request_is_analysis_follows_conversation_mode() {
        let (storage, _dir) = test_storage();
        storage.create_conversation("c1", "Conv").unwrap();

        assert!(
            !resolve_request_is_analysis(&storage, "c1"),
            "daily conversations must stay in daily mode"
        );

        storage.set_conversation_mode("c1", "confirming").unwrap();
        assert!(
            resolve_request_is_analysis(&storage, "c1"),
            "confirming conversations must use analysis mode"
        );

        storage.set_conversation_mode("c1", "analyzing").unwrap();
        assert!(
            resolve_request_is_analysis(&storage, "c1"),
            "analyzing conversations must use analysis mode"
        );

        storage.set_conversation_mode("c1", "daily").unwrap();
        assert!(
            !resolve_request_is_analysis(&storage, "c1"),
            "daily conversations must not use analysis mode"
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
}

// ---------------------------------------------------------------------------
// Private helpers for run_llm_step
// ---------------------------------------------------------------------------

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
        ..AppSettings::default()
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
            session_mgr,
            auth_manager,
            app,
        };
        let host = Arc::new(TauriRuntimeHost::new(services.app.clone()));
        let adapter = Arc::new(TauriEventAdapter::new(host));
        let bus = RuntimeEventBus::new();
        bus.subscribe(adapter);
        let llm_executor: Arc<dyn RuntimeLlmExecutor> = Arc::new(TauriLegacyTurnExecutor {
            services: services.clone(),
        });
        let mut runtime = SessionRuntime::with_llm_executor(
            QueryEngine::new().with_workspace_path(services.file_mgr.workspace_path().to_path_buf()),
            bus,
            llm_executor,
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
        let is_analysis = resolve_request_is_analysis(&self.services.db, &conversation_id);
        let mut request = ChatTurnRequest::new(conversation_id, content, file_ids);
        request.is_analysis = is_analysis;
        self.runtime
            .run_chat_request(request)
            .await
    }

    pub fn is_agent_busy(&self) -> Vec<String> {
        self.services.gateway.get_busy_conversations()
    }

    pub async fn stop_streaming(&self, conversation_id: String) -> Result<(), String> {
        let session_id = SessionId::new(conversation_id.clone());
        self.runtime
            .cancel_session(&session_id, crate::runtime::cancellation::CancellationReason::Interrupt);
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
    ) -> Result<(), String> {
        self.runtime
            .resolve_permission_request(
                &ToolCallId::new(tool_call_id),
                PendingPermissionResolution::Allow { updated_input },
            )
            .map_err(|e| e.to_string())
    }

    pub async fn deny_permission_request(
        &self,
        tool_call_id: String,
        message: Option<String>,
    ) -> Result<(), String> {
        self.runtime
            .resolve_permission_request(
                &ToolCallId::new(tool_call_id),
                PendingPermissionResolution::Deny {
                    message: message.unwrap_or_else(|| "Permission request denied by user.".to_string()),
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
                    message: message.unwrap_or_else(|| "Permission request cancelled by user.".to_string()),
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

    pub async fn get_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_conversations(
            self.services.db.clone() as Arc<dyn ConversationStore>,
        )
        .await
    }
}
