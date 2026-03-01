use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use futures::StreamExt;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;
use crate::llm::gateway::LlmGateway;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent};
use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::tool_executor::{self, ToolContext};
use crate::llm::orchestrator::{self, AnalysisAction, StepConfig};
use crate::llm::prompts;
use crate::llm::prompt_guard;
use crate::storage::crypto::SecureStorage;
use crate::models::settings::AppSettings;

/// Maximum agent loop iterations for daily consultation mode.
const MAX_TOOL_ITERATIONS: usize = 10;

/// Maximum wall-clock time for the entire agent loop (15 minutes for multi-step analysis).
const AGENT_TIMEOUT_SECS: u64 = 900;

/// Maximum time to wait for a single chunk from the LLM stream (90 seconds).
/// If no data arrives within this window, the stream is considered stalled.
const CHUNK_TIMEOUT_SECS: u64 = 90;

/// Compress tool result text in message history to save context tokens.
/// Strips verbose headers from execute_python results while keeping the actual output.
fn compress_tool_result(text: &str) -> String {
    // Only compress messages that look like execute_python tool results
    if !text.contains("[Purpose:") || !text.contains("Exit code:") {
        return text.to_string();
    }

    let mut output = String::new();
    let mut in_stdout = false;
    let mut in_stderr = false;
    let mut in_generated = false;

    for line in text.lines() {
        // Skip verbose headers
        if line.starts_with("[Purpose:") || line.starts_with("Exit code:") || line.starts_with("Execution time:") {
            continue;
        }
        if line == "--- stdout ---" {
            in_stdout = true;
            in_stderr = false;
            in_generated = false;
            continue;
        }
        if line == "--- stderr ---" {
            in_stderr = true;
            in_stdout = false;
            in_generated = false;
            // Only include stderr if there was an error
            continue;
        }
        if line == "--- generated_files ---" {
            in_generated = true;
            in_stdout = false;
            in_stderr = false;
            continue;
        }

        // Keep stdout content (the actual analysis output)
        if in_stdout || in_generated {
            output.push_str(line);
            output.push('\n');
        }
        // Keep stderr only for actual errors (skip warnings)
        if in_stderr && !line.trim().is_empty() && !line.contains("FutureWarning") && !line.contains("DeprecationWarning") {
            output.push_str(line);
            output.push('\n');
        }
    }

    if output.trim().is_empty() {
        // If no stdout/stderr sections found, return original (might be a different tool)
        text.to_string()
    } else {
        output
    }
}

/// Guard that clears the gateway's active task for a specific conversation when dropped.
/// Ensures the agent state is cleaned up on success, error, or panic.
struct AgentGuard {
    gateway: Arc<LlmGateway>,
    db: Arc<AppStorage>,
    app: AppHandle,
    conversation_id: String,
    cleared: bool,
}

impl AgentGuard {
    fn new(gateway: Arc<LlmGateway>, db: Arc<AppStorage>, app: AppHandle, conversation_id: String) -> Self {
        Self { gateway, db, app, conversation_id, cleared: false }
    }

    /// Explicitly clear the active task and emit cleanup events.
    /// Always emits both `streaming:done` (so frontend clears streaming UI)
    /// and `agent:idle` (so frontend clears busy state).
    async fn clear(&mut self) {
        if !self.cleared {
            self.cleared = true;
            self.gateway.clear_task(&self.conversation_id).await;
            self.db.remove_active_task(&self.conversation_id).ok();
            // streaming:done MUST fire so frontend clears isStreaming state.
            // finish_agent() also emits this, but if the agent panicked before
            // reaching finish_agent(), this is the only safety net.
            if let Err(e) = self.app.emit("streaming:done", serde_json::json!({
                "conversationId": self.conversation_id,
                "messageId": "",
            })) {
                log::warn!("[AgentGuard] Failed to emit streaming:done for {}: {}", self.conversation_id, e);
            }
            if let Err(e) = self.app.emit("agent:idle", serde_json::json!({
                "conversationId": self.conversation_id
            })) {
                log::warn!("[AgentGuard] Failed to emit agent:idle for {}: {}", self.conversation_id, e);
            }
            log::info!("[AgentGuard] Cleared active task for conversation {} and emitted streaming:done + agent:idle", self.conversation_id);
        }
    }
}

impl Drop for AgentGuard {
    fn drop(&mut self) {
        if !self.cleared {
            // Synchronous DB cleanup — works even if tokio runtime is shutting down
            self.db.remove_active_task(&self.conversation_id).ok();

            // Async cleanup for gateway + event emission via spawn
            // (may not execute if runtime is shutting down, but we have crash recovery
            // in lib.rs to handle that case on next startup)
            let gateway = self.gateway.clone();
            let app = self.app.clone();
            let conversation_id = self.conversation_id.clone();
            tokio::spawn(async move {
                gateway.clear_task(&conversation_id).await;
                let _ = app.emit("streaming:done", serde_json::json!({
                    "conversationId": conversation_id,
                    "messageId": "",
                }));
                let _ = app.emit("agent:idle", serde_json::json!({
                    "conversationId": conversation_id
                }));
                log::info!("[AgentGuard] Drop fallback: cleared active task for conversation {} and emitted streaming:done + agent:idle", conversation_id);
            });
        }
    }
}

/// Check which conversations have active agent tasks.
///
/// Returns a list of conversation IDs that are currently being processed.
#[tauri::command]
pub async fn is_agent_busy(
    gateway: State<'_, Arc<LlmGateway>>,
) -> Result<Vec<String>, String> {
    Ok(gateway.get_busy_conversations().await)
}

/// Send a user message and trigger the LLM agent loop.
///
/// The function detects whether the conversation is in analysis mode
/// (structured 5-step workflow) or daily consultation mode, and
/// dispatches accordingly with the appropriate system prompt, tool
/// filter, and iteration limit.
#[tauri::command]
pub async fn send_message(
    db: State<'_, Arc<AppStorage>>,
    gateway: State<'_, Arc<LlmGateway>>,
    file_mgr: State<'_, Arc<FileManager>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
    app: AppHandle,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
) -> Result<(), String> {
    log::info!("=== send_message START === conversation_id={}, content_len={}, file_ids={:?}",
        conversation_id, content.len(), file_ids);

    // Guard: reject if THIS conversation is already busy
    if gateway.is_conversation_busy(&conversation_id).await {
        log::warn!("send_message rejected: conversation {} is already processing", conversation_id);
        return Err("This conversation is already processing.".to_string());
    }

    // Guard: reject if max concurrent agents reached
    let busy_count = gateway.get_busy_conversations().await.len();
    if busy_count >= crate::llm::gateway::MAX_CONCURRENT_AGENTS {
        log::warn!("send_message rejected: max concurrent agents reached ({}/{})",
            busy_count, crate::llm::gateway::MAX_CONCURRENT_AGENTS);
        return Err(format!(
            "Maximum concurrent conversations reached ({}). Please wait.",
            crate::llm::gateway::MAX_CONCURRENT_AGENTS
        ));
    }

    // 1. Look up attached file metadata (single batch query)
    let file_attachments = if file_ids.is_empty() {
        Vec::new()
    } else {
        db.get_uploaded_files_by_ids(&file_ids).map_err(|e| e.to_string())?
    };

    // 2. Save user message to DB (including file references)
    let msg_id = uuid::Uuid::new_v4().to_string();
    let content_json = if file_attachments.is_empty() {
        serde_json::json!({ "text": content }).to_string()
    } else {
        let files_meta: Vec<serde_json::Value> = file_attachments.iter().map(|f| {
            serde_json::json!({
                "id": f["id"],
                "fileName": f["originalName"],
                "fileSize": f["fileSize"],
                "fileType": f["fileType"],
                "status": "uploaded",
            })
        }).collect();
        serde_json::json!({ "text": content, "files": files_meta }).to_string()
    };

    db.insert_message(&msg_id, &conversation_id, "user", &content_json)
        .map_err(|e| e.to_string())?;

    // NOTE: We do NOT emit "message:updated" for the user message here.
    // The frontend already adds an optimistic user message to the store
    // before calling this IPC command. Emitting here would cause duplicates.

    // 3. Build LLM content with file references
    let llm_content = if file_attachments.is_empty() {
        content.clone()
    } else {
        let file_refs: Vec<String> = file_attachments.iter().map(|f| {
            let name = f["originalName"].as_str().unwrap_or("unknown");
            let ftype = f["fileType"].as_str().unwrap_or("unknown");
            let fid = f["id"].as_str().unwrap_or("");
            format!("- {} (file_id: \"{}\", 类型: {})", name, fid, ftype)
        }).collect();
        format!(
            "{}\n\n[已上传文件]\n{}\n\n提示：先调用 analyze_file(file_id) 获取文件的 filePath（绝对路径），然后在 execute_python 中使用该 filePath 读取文件。",
            content, file_refs.join("\n")
        )
    };

    // 4. Load settings
    let settings_map = db.get_all_settings().map_err(|e| e.to_string())?;
    let mut settings: AppSettings = if settings_map.is_empty() {
        log::info!("No settings in DB, using defaults");
        AppSettings::default()
    } else {
        log::info!("Settings map has {} keys: {:?}", settings_map.len(),
            settings_map.keys().collect::<Vec<_>>());
        AppSettings::from_string_map(&settings_map)
    };

    log::info!("Settings loaded: primary_model={}, masking={}, auto_routing={}",
        settings.primary_model, settings.data_masking_level, settings.auto_model_routing);

    // Log raw key info (before decryption) for diagnostics
    let raw_pk_len = settings.primary_api_key.len();
    let raw_pk_has_colon = settings.primary_api_key.contains(':');
    log::info!("Raw primary_api_key: len={}, contains_colon={}, first_10='{}'",
        raw_pk_len, raw_pk_has_colon,
        settings.primary_api_key.chars().take(10).collect::<String>());

    // Decrypt API keys if SecureStorage is available
    if let Some(ss) = crypto.as_ref() {
        log::info!("SecureStorage available, decrypting API keys...");
        settings.primary_api_key = decrypt_key(ss, &settings.primary_api_key);
        settings.tavily_api_key = decrypt_key(ss, &settings.tavily_api_key);
    } else {
        log::warn!("SecureStorage NOT available, using raw key values");
    }

    log::info!("After decryption: primary_api_key len={}, first_10='{}'",
        settings.primary_api_key.len(),
        settings.primary_api_key.chars().take(10).collect::<String>());

    // Fall back to built-in default key ONLY for DeepSeek provider
    // (the built-in key is a DeepSeek key, using it for other providers would cause 401)
    if settings.primary_api_key.is_empty() && settings.primary_model == "deepseek-v3" {
        log::info!("Primary API key empty for DeepSeek, falling back to built-in default key");
        let defaults = AppSettings::default();
        settings.primary_api_key = defaults.primary_api_key.clone();
    }

    log::info!("Final primary_api_key: len={}, first_10='{}'",
        settings.primary_api_key.len(),
        settings.primary_api_key.chars().take(10).collect::<String>());

    // Check if API key is configured
    if settings.primary_api_key.is_empty() {
        let provider_name = match settings.primary_model.as_str() {
            "deepseek-v3" => "DeepSeek",
            "qwen-plus" => "通义千问",
            "openai" => "OpenAI",
            "claude" => "Claude",
            "volcano" => "火山引擎",
            _ => &settings.primary_model,
        };
        let error_msg = format!("请先在设置中配置 {} 的 API Key", provider_name);
        app.emit("streaming:error", serde_json::json!({
            "conversationId": conversation_id,
            "error": error_msg,
        }))
            .map_err(|e| e.to_string())?;
        return Err(error_msg);
    }

    // 5. Build initial message history from DB (sliding window: last N messages)
    //    Uses SQL LIMIT to avoid loading the full history for long conversations.
    const MAX_HISTORY_MESSAGES: u32 = 30;
    let db_messages = db.get_recent_messages(&conversation_id, MAX_HISTORY_MESSAGES)
        .map_err(|e| e.to_string())?;
    let mut chat_messages: Vec<ChatMessage> = db_messages.iter().filter_map(|m| {
        let role = m.get("role")?.as_str()?;
        let content_val = m.get("content")?;
        let text = content_val.get("text")?.as_str()?.to_string();
        // Compress tool results in history to save context tokens
        let text = compress_tool_result(&text);
        Some(ChatMessage::text(role, text))
    }).collect();

    // Replace the last user message content with llm_content that includes file references
    if !file_attachments.is_empty() {
        if let Some(last) = chat_messages.last_mut() {
            if last.role == "user" {
                last.content = llm_content;
            }
        }
    }

    log::info!("Chat messages built: count={}, roles={:?}",
        chat_messages.len(),
        chat_messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>());

    // 6. Determine masking level
    // Always use Strict masking — PII protection is non-negotiable.
    // The setting is kept for forward compatibility but defaults to strict.
    let masking_level = MaskingLevel::Strict;

    // 7. Derive workspace path for tool execution
    let workspace_path = db.get_setting("workspacePath")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| file_mgr.workspace_path().to_path_buf());

    // 8. Determine conversation mode and step configuration
    //
    // The conversation.mode column is the single source of truth:
    //   "daily"      → normal chat (may detect analysis trigger)
    //   "confirming" → Step 0 done, waiting for user to confirm direction
    //   "analyzing"  → Steps 1–5 in progress
    let has_files = !file_attachments.is_empty();
    let conversation_mode = db.get_conversation_mode(&conversation_id)
        .unwrap_or_else(|_| "daily".to_string());

    log::info!("Conversation {} mode='{}', has_files={}", conversation_id, conversation_mode, has_files);

    let step_config: Option<StepConfig> = match conversation_mode.as_str() {
        "daily" => {
            // Check if user wants to start a structured analysis
            if orchestrator::detect_analysis_mode(&chat_messages, has_files) {
                log::info!("Analysis mode detected for conversation {}, transitioning to 'confirming'",
                    conversation_id);

                // Transition: daily → confirming (run Step 0: direction confirmation)
                if let Err(e) = db.set_conversation_mode(&conversation_id, "confirming") {
                    log::error!("Failed to set mode to 'confirming': {}", e);
                }

                let config = orchestrator::build_step_config(0);
                // Initialize step 0 as in_progress in the analysis_states table
                if let Err(e) = orchestrator::advance_step(
                    &db.inner().clone(), &conversation_id, 0, "in_progress",
                ) {
                    log::error!("Failed to mark step 0 as in_progress: {}", e);
                }

                Some(config)
            } else {
                None // daily chat, no analysis
            }
        }

        "confirming" | "analyzing" => {
            // Route through the orchestrator's mode-based state machine
            let action = orchestrator::next_action(
                &conversation_mode,
                &db.inner().clone(),
                &conversation_id,
                &content,
            );
            log::info!("Orchestrator action for conversation {}: {:?}",
                conversation_id, std::mem::discriminant(&action));

            match action {
                AnalysisAction::DailyChat => {
                    // Shouldn't normally happen in confirming/analyzing mode,
                    // but handle gracefully (e.g. mode column out of sync)
                    log::warn!("Orchestrator returned DailyChat while mode='{}', falling through",
                        conversation_mode);
                    None
                }

                AnalysisAction::StartAnalysis(config) => {
                    // confirming → analyzing: Step 0 done, user confirmed, start Step 1
                    log::info!("Starting analysis step {} for conversation {}",
                        config.step, conversation_id);

                    // Save user's response as analysis direction note.
                    // In "confirming" mode, any response after Step 0 is treated as
                    // confirmation + optional direction (e.g. "重点看技术部门").
                    let direction = content.trim();
                    if !direction.is_empty() {
                        let note_key = format!("note:{}:analysis_direction", conversation_id);
                        match db.set_memory(&note_key, direction, Some("step_0_direction")) {
                            Ok(_) => log::info!("Saved analysis direction: '{}'", direction),
                            Err(e) => log::warn!("Failed to save analysis direction: {}", e),
                        }
                    }

                    // Transition: confirming → analyzing
                    if let Err(e) = db.set_conversation_mode(&conversation_id, "analyzing") {
                        log::error!("Failed to set mode to 'analyzing': {}", e);
                    }

                    // Mark step 0 completed, new step as in_progress
                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, 0, "completed",
                    ) {
                        log::error!("Failed to mark step 0 as completed: {}", e);
                    }
                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, config.step, "in_progress",
                    ) {
                        log::error!("Failed to mark step {} as in_progress: {}", config.step, e);
                    }

                    // Sub-agent context reset: discard Step 0's tool call history,
                    // keep only the original user message + a synthetic summary.
                    let original_user_msg = chat_messages.iter()
                        .find(|m| m.role == "user")
                        .cloned();
                    let step_summary = format!(
                        "分析方向已确认。关键信息已保存到分析记录中（见系统提示词中的[前序分析记录]部分）。\n\
                        请开始执行第 {} 步。\n\
                        如需读取数据文件，请使用系统提示词中[本次会话的文件]部分提供的文件信息。",
                        config.step
                    );
                    chat_messages = Vec::new();
                    if let Some(user_msg) = original_user_msg {
                        chat_messages.push(user_msg);
                    }
                    chat_messages.push(ChatMessage::text("assistant", &step_summary));

                    Some(config)
                }

                AnalysisAction::AdvanceStep(config) => {
                    // User confirmed current step → advance to next step
                    log::info!("Advancing to step {} for conversation {}",
                        config.step, conversation_id);

                    let prev_step = config.step - 1;

                    // Mark previous step completed, new step in_progress
                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, prev_step, "completed",
                    ) {
                        log::error!("Failed to mark step {} as completed: {}", prev_step, e);
                    }
                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, config.step, "in_progress",
                    ) {
                        log::error!("Failed to mark step {} as in_progress: {}", config.step, e);
                    }

                    // Sub-agent context reset
                    let original_user_msg = chat_messages.iter()
                        .find(|m| m.role == "user")
                        .cloned();
                    let step_summary = format!(
                        "前 {} 步分析已完成。关键结论已保存到分析记录中（见系统提示词中的[前序分析记录]部分）。\n\
                        请基于这些记录继续执行第 {} 步。\n\
                        如需读取数据文件，请使用系统提示词中[本次会话的文件]部分提供的文件信息。",
                        prev_step, config.step
                    );
                    chat_messages = Vec::new();
                    if let Some(user_msg) = original_user_msg {
                        chat_messages.push(user_msg);
                    }
                    chat_messages.push(ChatMessage::text("assistant", &step_summary));

                    Some(config)
                }

                AnalysisAction::RerunStep(config) => {
                    // User gave feedback → re-run current step with feedback in messages
                    log::info!("Re-running step {} with user feedback for conversation {}",
                        config.step, conversation_id);

                    // Reset step status to in_progress
                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, config.step, "in_progress",
                    ) {
                        log::error!("Failed to mark step {} as in_progress: {}", config.step, e);
                    }

                    // Keep current messages (user's feedback is the last message)
                    Some(config)
                }

                AnalysisAction::ResumeStep(config) => {
                    // Crash recovery → resume a paused step
                    log::info!("Resuming paused step {} for conversation {}",
                        config.step, conversation_id);

                    // Mark step as in_progress (was "paused" from crash recovery)
                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, config.step, "in_progress",
                    ) {
                        log::error!("Failed to mark step {} as in_progress: {}", config.step, e);
                    }

                    Some(config)
                }

                AnalysisAction::FinishAnalysis => {
                    // All steps done, user confirmed → exit analysis mode
                    log::info!("Analysis complete for conversation {}", conversation_id);

                    // Transition: analyzing → daily
                    if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                        log::error!("Failed to set mode to 'daily': {}", e);
                    }
                    if let Err(e) = db.finalize_analysis(&conversation_id, "completed") {
                        log::error!("Failed to finalize analysis: {}", e);
                    }

                    // Fall through to daily mode for follow-up questions
                    None
                }

                AnalysisAction::AbortAnalysis => {
                    // User explicitly aborted analysis (e.g. "算了", "取消", "cancel")
                    log::info!("User aborted analysis for conversation {}", conversation_id);

                    // Transition: confirming/analyzing → daily
                    if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                        log::error!("Failed to set mode to 'daily': {}", e);
                    }
                    if let Err(e) = db.finalize_analysis(&conversation_id, "aborted") {
                        log::error!("Failed to finalize analysis as aborted: {}", e);
                    }

                    // Fall through to daily mode — agent will respond to confirm abort
                    None
                }
            }
        }

        // Unknown mode — treat as daily
        _ => {
            log::warn!("Unknown conversation mode '{}', treating as daily", conversation_mode);
            None
        }
    };

    // Clone everything needed for the background task
    let assistant_id = uuid::Uuid::new_v4().to_string();
    let assistant_id_clone = assistant_id.clone();
    let assistant_id_for_timeout = assistant_id.clone();
    let conversation_id_clone = conversation_id.clone();
    let db_clone = db.inner().clone();
    let gateway_clone = gateway.inner().clone();
    let file_mgr_clone = file_mgr.inner().clone();
    let app_clone = app.clone();

    // 9. Spawn the agent loop in a background task with guard and timeout
    log::info!("=== Spawning agent_loop === assistant_id={}, analysis_step={:?}",
        assistant_id, step_config.as_ref().map(|c| c.step));

    // Mark gateway as busy BEFORE spawning so concurrent calls are blocked immediately
    gateway.set_busy(&conversation_id).await.map_err(|e| e)?;

    // Record in DB for crash recovery. If this fails, rollback the gateway busy state.
    if let Err(e) = db.insert_active_task(&conversation_id) {
        log::error!("Failed to insert active task, rolling back gateway busy state: {}", e);
        gateway.clear_task(&conversation_id).await;
        return Err(e.to_string());
    }

    tokio::spawn(async move {
        let mut guard = AgentGuard::new(
            gateway_clone.clone(),
            db_clone.clone(),
            app_clone.clone(),
            conversation_id_clone.clone(),
        );

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(AGENT_TIMEOUT_SECS),
            agent_loop(
                db_clone,
                gateway_clone,
                file_mgr_clone,
                app_clone.clone(),
                settings,
                chat_messages,
                masking_level,
                workspace_path,
                assistant_id_clone,
                conversation_id_clone.clone(),
                step_config,
            ),
        ).await;

        match result {
            Ok(()) => {
                log::info!("[AgentGuard] agent_loop completed normally for conversation {}", conversation_id_clone);
            }
            Err(_elapsed) => {
                log::error!("[AgentGuard] agent_loop TIMED OUT after {}s for conversation {}",
                    AGENT_TIMEOUT_SECS, conversation_id_clone);
                // Cancel any active streaming for this conversation
                guard.gateway.cancel_conversation(&conversation_id_clone).await.ok();
                let _ = app_clone.emit(
                    "streaming:error",
                    serde_json::json!({
                        "conversationId": conversation_id_clone,
                        "error": format!("Agent timed out after {} minutes. Please try again.", AGENT_TIMEOUT_SECS / 60),
                    }),
                );
                // Must emit streaming:done so frontend clears streaming state
                let _ = app_clone.emit(
                    "streaming:done",
                    serde_json::json!({
                        "conversationId": conversation_id_clone,
                        "messageId": assistant_id_for_timeout,
                    }),
                );
            }
        }

        // Always clear the guard
        guard.clear().await;
    });

    Ok(())
}

/// The core agent loop — stream, execute tools, re-stream until done.
///
/// When `step_config` is `Some`, the loop operates in analysis mode:
/// - Uses the step's system prompt
/// - Filters tools to only the step's relevant tools
/// - Respects the step's max_iterations limit
/// - Auto-advances to the next step when the current step completes
///
/// When `step_config` is `None`, operates in daily consultation mode:
/// - Uses the daily system prompt
/// - All tools available
/// - Standard MAX_TOOL_ITERATIONS limit
async fn agent_loop(
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    app: AppHandle,
    settings: AppSettings,
    initial_messages: Vec<ChatMessage>,
    masking_level: MaskingLevel,
    workspace_path: std::path::PathBuf,
    assistant_id: String,
    conversation_id: String,
    step_config: Option<StepConfig>,
) {
    let tavily_api_key = if settings.tavily_api_key.is_empty() {
        None
    } else {
        Some(settings.tavily_api_key.clone())
    };

    // Build file context: query ALL uploaded files for this conversation
    // so the LLM always knows what's available (even after sliding window truncation)
    let file_context = match db.get_uploaded_files_for_conversation(&conversation_id) {
        Ok(files) if !files.is_empty() => {
            let refs: Vec<String> = files.iter().map(|f| {
                let name = f["originalName"].as_str().unwrap_or("unknown");
                let fid = f["id"].as_str().unwrap_or("");
                let ftype = f["fileType"].as_str().unwrap_or("unknown");
                format!("- {} (file_id: \"{}\", 类型: {})", name, fid, ftype)
            }).collect();
            format!(
                "\n\n[本次会话的文件]\n{}\n说明：先调用 analyze_file(file_id) 获取文件信息，返回中的 filePath 字段是文件绝对路径。execute_python 中使用该 filePath 读取文件。",
                refs.join("\n")
            )
        }
        _ => String::new(),
    };

    let current_step_config = step_config;
    let mut messages = initial_messages;
    let current_assistant_id = assistant_id;

    // In daily mode, filter out tool call/result messages from analysis history
    // to maximize useful context within the token budget. Analysis steps produce
    // many tool call + tool result messages that are not useful for daily chat.
    if current_step_config.is_none() {
        messages.retain(|m| {
            // Keep user and system messages
            if m.role == "user" || m.role == "system" {
                return true;
            }
            // Keep assistant messages that have text content (not pure tool-call wrappers)
            if m.role == "assistant" {
                return !m.content.is_empty()
                    && m.tool_calls.as_ref().map_or(true, |tc| tc.is_empty());
            }
            // Filter out tool result messages
            if m.tool_call_id.is_some() {
                return false;
            }
            true
        });
    }

    // Determine system prompt, tool filter, and token budget based on mode
    let (system_prompt, tool_defs_override, max_iterations, token_budget, chunk_timeout_secs) = match &current_step_config {
        Some(config) => {
            log::info!("Agent loop in ANALYSIS mode: step={}, tools={}, max_iter={}",
                config.step,
                config.tool_defs.len(),
                config.max_iterations);
            (
                config.system_prompt.clone(),
                Some(config.tool_defs.clone()),
                config.max_iterations,
                8192u32, // analysis steps need more output room for structured data
                180u64,  // analysis mode: tools take time, LLM thinks longer
            )
        }
        None => {
            log::info!("Agent loop in DAILY CONSULTATION mode");
            (
                prompts::get_system_prompt(None),
                None, // use all tools
                MAX_TOOL_ITERATIONS,
                4096u32, // daily consultation: standard budget
                CHUNK_TIMEOUT_SECS, // daily mode: 90s
            )
        }
    };

    // Append file context to system prompt so LLM always has file info
    let system_prompt = format!("{}{}", system_prompt, file_context);

    // Inject accumulated analysis notes from previous steps into the system prompt.
    // This ensures the LLM retains key findings even when message history is truncated.
    let analysis_notes_context = {
        let notes_prefix = format!("note:{}:", conversation_id);
        match db.get_memories_by_prefix(&notes_prefix) {
            Ok(notes) if !notes.is_empty() => {
                let mut ctx = String::from("\n\n[前序分析记录（save_analysis_note 保存的关键结论）]\n");
                for (key, value) in &notes {
                    // Extract the note name from "note:{conv_id}:{name}"
                    let note_name = key.strip_prefix(&notes_prefix).unwrap_or(key);
                    ctx.push_str(&format!("### {}\n{}\n\n", note_name, value));
                }
                ctx.push_str("说明：以上是之前步骤保存的分析结论，当前步骤应基于这些结论继续分析。\n");
                ctx
            }
            _ => String::new(),
        }
    };
    let system_prompt = format!("{}{}", system_prompt, analysis_notes_context);

    let mut full_content = String::new();
    let mut combined_mask_ctx: Option<MaskingContext> = None;
    let mut generated_file_ids: Vec<String> = Vec::new();
    let mut iteration_count = 0usize;

    for iteration in 0..max_iterations {
        iteration_count = iteration + 1;
        log::info!(
            "=== [AGENT] iteration={}/{} conversation={} messages={} ===",
            iteration, max_iterations, conversation_id, messages.len()
        );
        // Log each message role + content length for debugging
        for (i, m) in messages.iter().enumerate() {
            let has_tc = m.tool_calls.as_ref().map_or(0, |v| v.len());
            let tc_id = m.tool_call_id.as_deref().unwrap_or("-");
            log::debug!(
                "[AGENT] msg[{}] role={} len={} tool_call_id={} tool_calls={}",
                i, m.role, m.content.len(), tc_id, has_tc
            );
        }

        // Stream from LLM with system prompt and tool filter
        let stream_start = std::time::Instant::now();
        log::info!("[AGENT] Calling gateway.stream_message() model={} system_prompt_len={}",
            settings.primary_model, system_prompt.len());
        let stream_result = gateway
            .stream_message(
                &settings,
                messages.clone(),
                masking_level.clone(),
                Some(&system_prompt),
                tool_defs_override.clone(),
                token_budget,
                Some(&conversation_id),
            )
            .await;

        let (_task_id, mut stream, mask_ctx, mut cancel_rx) = match stream_result {
            Ok(r) => {
                log::info!("gateway.stream_message() OK, task_id={}", r.0);
                r
            }
            Err(e) => {
                log::error!("gateway.stream_message() FAILED: {}", e);
                let _ = app.emit(
                    "streaming:error",
                    serde_json::json!({
                        "conversationId": conversation_id,
                        "error": e.to_string(),
                    }),
                );
                // Must emit streaming:done so frontend clears streaming state
                let _ = app.emit(
                    "streaming:done",
                    serde_json::json!({
                        "conversationId": conversation_id,
                        "messageId": current_assistant_id,
                    }),
                );
                return;
            }
        };

        // Merge this iteration's masking context into the combined context.
        // Each iteration may discover new PII in tool results; merging ensures
        // the final unmask() covers all mappings from all iterations.
        match combined_mask_ctx.as_mut() {
            Some(existing) => existing.merge(mask_ctx),
            None => { combined_mask_ctx = Some(mask_ctx); }
        }

        // Collect this iteration's content and tool calls
        let mut iter_content = String::new();
        let mut tool_calls = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut delta_count: u32 = 0;
        let mut stream_cancelled = false;

        loop {
            let chunk_timeout = tokio::time::sleep(
                std::time::Duration::from_secs(chunk_timeout_secs),
            );
            tokio::select! {
                // Cancel signal — fires immediately even if HTTP is stuck
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        log::info!("[AGENT] Cancel signal received for conversation {}", conversation_id);
                        stream_cancelled = true;
                        break;
                    }
                }
                // No data for chunk_timeout_secs — treat as stalled
                _ = chunk_timeout => {
                    log::error!("[AGENT] Chunk timeout ({}s) for conversation {}", chunk_timeout_secs, conversation_id);
                    let _ = app.emit("streaming:error", serde_json::json!({
                        "conversationId": conversation_id,
                        "error": "响应超时，请重试。",
                    }));
                    // Must emit streaming:done so frontend clears streaming state
                    let _ = app.emit("streaming:done", serde_json::json!({
                        "conversationId": conversation_id,
                        "messageId": current_assistant_id,
                    }));
                    return;
                }
                // Normal stream event
                event = stream.next() => {
                    match event {
                        Some(StreamEvent::ContentDelta { delta }) => {
                            let clean = strip_thinking_markers(&delta);
                            if !clean.is_empty() {
                                delta_count += 1;
                                iter_content.push_str(&clean);
                                full_content.push_str(&clean);

                                // Leak detection: check periodically to avoid per-delta overhead
                                if delta_count % 5 == 0 || full_content.len() > 200 {
                                    if let prompt_guard::LeakCheckResult::Leaked { matched_count, .. } =
                                        prompt_guard::check_for_leak(&full_content)
                                    {
                                        log::warn!(
                                            "[AGENT] Prompt leak detected mid-stream for {} ({} fingerprints)",
                                            conversation_id, matched_count
                                        );
                                        full_content.clear();
                                        full_content.push_str(prompt_guard::LEAK_REFUSAL);
                                        iter_content.clear();
                                        break;
                                    }
                                }

                                let _ = app.emit(
                                    "streaming:delta",
                                    serde_json::json!({
                                        "conversationId": conversation_id,
                                        "delta": clean,
                                    }),
                                );
                            }
                        }
                        Some(StreamEvent::ThinkingDelta { delta }) => {
                            let _ = app.emit(
                                "streaming:delta",
                                serde_json::json!({
                                    "conversationId": conversation_id,
                                    "delta": delta,
                                }),
                            );
                        }
                        Some(StreamEvent::ToolCallStart { tool_call }) => {
                            log::info!(
                                "[AGENT] Tool call received: name='{}' id='{}' args={}",
                                tool_call.name,
                                tool_call.id,
                                serde_json::to_string(&tool_call.arguments)
                                    .unwrap_or_else(|_| "??".into())
                            );
                            tool_calls.push(tool_call);
                        }
                        Some(StreamEvent::Done {
                            stop_reason: reason,
                            usage,
                            ..
                        }) => {
                            stop_reason = reason;
                            let stream_elapsed = stream_start.elapsed();
                            log::info!(
                                "[AGENT] Stream done: stop_reason={:?}, usage=({} in / {} out), deltas={}, content_len={}, tool_calls={}, elapsed={:?}",
                                stop_reason, usage.input_tokens, usage.output_tokens,
                                delta_count, iter_content.len(), tool_calls.len(), stream_elapsed
                            );
                            break;
                        }
                        Some(StreamEvent::Error { error }) => {
                            log::error!("[AGENT] Stream error: {}", error);
                            let _ = app.emit(
                                "streaming:error",
                                serde_json::json!({
                                    "conversationId": conversation_id,
                                    "error": error,
                                }),
                            );
                            // Must emit streaming:done so frontend clears streaming state
                            let _ = app.emit(
                                "streaming:done",
                                serde_json::json!({
                                    "conversationId": conversation_id,
                                    "messageId": current_assistant_id,
                                }),
                            );
                            return;
                        }
                        None => {
                            log::info!("[AGENT] Stream ended (None) for conversation {}", conversation_id);
                            break;
                        }
                    }
                }
            }
        }

        // If cancelled, break out of the iteration loop
        if stream_cancelled {
            break;
        }

        // If no tool calls or stop reason is EndTurn, finish this step
        if tool_calls.is_empty() || stop_reason != StopReason::ToolUse {
            log::info!(
                "[AGENT] Finishing step: stop_reason={:?}, tool_calls={}, total_content_len={}",
                stop_reason, tool_calls.len(), full_content.len()
            );
            break; // exit inner loop, check step transition below
        }

        // --- Tool execution phase ---
        messages.push(ChatMessage::assistant_with_tool_calls(
            iter_content,
            tool_calls.clone(),
        ));

        let tool_ctx = ToolContext {
            db: db.clone(),
            file_manager: file_mgr.clone(),
            workspace_path: workspace_path.clone(),
            conversation_id: conversation_id.clone(),
            tavily_api_key: tavily_api_key.clone(),
            app_handle: Some(app.clone()),
        };

        for tc in &tool_calls {
            log::info!(
                "[AGENT] Executing tool '{}' (id={}) with args: {}",
                tc.name, tc.id,
                serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "??".into())
            );

            let _ = app.emit(
                "tool:executing",
                serde_json::json!({
                    "conversationId": conversation_id,
                    "toolName": tc.name,
                    "toolId": tc.id,
                    "purpose": tc.arguments.get("purpose").and_then(|v| v.as_str()),
                }),
            );

            let tool_start = std::time::Instant::now();
            let result = tool_executor::execute_tool(&tool_ctx, tc).await;
            let tool_elapsed = tool_start.elapsed();

            log::info!(
                "[AGENT] Tool '{}' result: is_error={}, content_len={}, elapsed={:?}, preview='{}'",
                tc.name, result.is_error, result.content.len(), tool_elapsed,
                truncate_for_ui(&result.content, 300),
            );

            let _ = app.emit(
                "tool:completed",
                serde_json::json!({
                    "conversationId": conversation_id,
                    "toolName": tc.name,
                    "toolId": tc.id,
                    "success": !result.is_error,
                    "summary": truncate_for_ui(&result.content, 200),
                }),
            );

            // Collect fileId from tool results (generate_report, export_data, generate_chart, execute_python)
            if !result.is_error {
                // Try parsing as JSON first (generate_report, export_data, generate_chart return JSON)
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result.content) {
                    if let Some(file_id) = parsed.get("fileId").and_then(|v| v.as_str()) {
                        generated_file_ids.push(file_id.to_string());
                    }
                }
                // Also check for "fileId:" pattern in text output (execute_python auto-registered files).
                // Flexible parsing: handles "fileId: xxx", "fileId:xxx", extra whitespace, etc.
                for line in result.content.lines() {
                    if let Some(pos) = line.find("fileId:") {
                        let after = &line[pos + 7..].trim_start();
                        let id: String = after.chars()
                            .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
                            .collect();
                        // Validate UUID-like format (at least 32 hex chars + 4 hyphens = 36 chars)
                        if id.len() >= 36 && !generated_file_ids.contains(&id) {
                            generated_file_ids.push(id);
                        }
                    }
                }
            }

            let masked_result = match combined_mask_ctx.as_mut() {
                Some(ctx) => ctx.mask_text(&result.content),
                None => result.content.clone(),
            };
            messages.push(ChatMessage::tool_result(
                &tc.id,
                &tc.name,
                masked_result,
            ));
        }
    }

    // --- Step completion: save assistant message and check auto-advance ---

    // If we hit max_iterations without naturally finishing, append a notice
    if iteration_count >= max_iterations {
        log::warn!(
            "[AGENT] Hit max_iterations ({}) for conversation {} step {:?}",
            max_iterations, conversation_id, current_step_config.as_ref().map(|c| c.step)
        );
        if current_step_config.is_some() {
            let notice = format!(
                "\n\n---\n⚠️ 本步分析较为复杂，已达处理上限（{} 次迭代）。以上是当前阶段的分析结果。\n\
                如需补充分析，请回复具体要求；如结果已满足需要，请确认继续下一步。",
                max_iterations
            );
            full_content.push_str(&notice);
        }
    }

    // Save current step's assistant message
    finish_agent(
        &db,
        &app,
        &current_assistant_id,
        &conversation_id,
        &full_content,
        combined_mask_ctx.as_ref(),
        &generated_file_ids,
        &workspace_path,
    );

    // --- Step completion: mark as completed and wait for user confirmation ---
    //
    // All analysis steps require user confirmation before advancing.
    // The next user message will trigger send_message() → next_action() →
    // AdvanceStep/RerunStep/FinishAnalysis based on whether the user confirms
    // or gives feedback.
    if let Some(ref config) = current_step_config {
        let completed_step = config.step;

        // Validate that the step saved its analysis note (Steps 1-5 should all save notes)
        if completed_step >= 1 && completed_step <= 5 {
            let expected_key = format!("note:{}:step{}_summary", conversation_id, completed_step);
            let has_note = db.get_memory(&expected_key)
                .ok()
                .flatten()
                .map(|v| !v.is_empty())
                .unwrap_or(false);

            if !has_note {
                log::warn!(
                    "[AGENT] Step {} completed without saving analysis note '{}' — data may be lost in next step",
                    completed_step, expected_key
                );
                let _ = app.emit("streaming:error", serde_json::json!({
                    "conversationId": conversation_id,
                    "error": format!("⚠️ 第 {} 步完成但未保存分析记录（save_analysis_note），后续步骤可能丢失关键数据。建议在确认前要求 AI 重新保存。", completed_step),
                    "severity": "warning",
                }));
            }
        }

        log::info!(
            "[AGENT] Step {} finished for conversation {}, marking as completed",
            completed_step, conversation_id
        );

        // Mark this step as completed in DB so next_action() can route correctly.
        if let Err(e) = orchestrator::advance_step(
            &db, &conversation_id, completed_step, "completed",
        ) {
            log::error!("Failed to mark step {} as completed: {}", completed_step, e);
        }

        if completed_step >= 5 {
            log::info!("[AGENT] Final step (5) completed for conversation {}, awaiting user confirmation to finish", conversation_id);
        }
    }
}

/// Finalize the agent response — save to DB and emit events.
///
/// When `full_content` is empty (LLM produced only tool calls without
/// text output), we skip saving an empty message and skip
/// `message:updated`, but still emit `streaming:done` so the frontend
/// clears its streaming state. This prevents empty assistant bubbles
/// from appearing in the chat.
fn finish_agent(
    db: &Arc<AppStorage>,
    app: &AppHandle,
    assistant_id: &str,
    conversation_id: &str,
    full_content: &str,
    mask_ctx: Option<&MaskingContext>,
    generated_file_ids: &[String],
    workspace_path: &std::path::Path,
) {
    // Unmask PII placeholders before saving to DB
    let unmasked_content = match mask_ctx {
        Some(ctx) => ctx.unmask(full_content),
        None => full_content.to_string(),
    };

    // Leak detection at persistence boundary
    let (unmasked_content, was_leaked) = prompt_guard::filter_leaked_content(&unmasked_content);
    if was_leaked {
        log::warn!(
            "[finish_agent] Prompt leak caught at persistence for conv={}",
            conversation_id
        );
    }

    let unmasked_trimmed = unmasked_content.trim();

    // Only save and emit a message when there is actual content.
    // During analysis mode the LLM may produce tool-call-only iterations
    // (no visible text); saving those would create empty assistant bubbles.
    if !unmasked_trimmed.is_empty() {
        // Check if conversation still exists (might have been deleted while agent was running)
        if db.get_conversation_mode(conversation_id).is_err() {
            log::warn!("Conversation {} was deleted during agent run, skipping message save", conversation_id);
            return;
        }

        // Build content JSON, including generated files if any
        let content_value = if !generated_file_ids.is_empty() {
            match db.get_generated_files_by_ids(generated_file_ids) {
                Ok(file_records) if !file_records.is_empty() => {
                    let gen_files: Vec<serde_json::Value> = file_records.iter().map(|f| {
                        let stored_path = f["storedPath"].as_str().unwrap_or("");
                        let full_path = workspace_path.join(stored_path);
                        serde_json::json!({
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
                        })
                    }).collect();
                    log::info!(
                        "[AGENT] Attaching {} generated files to message {}",
                        gen_files.len(), assistant_id
                    );
                    serde_json::json!({
                        "text": unmasked_content,
                        "generatedFiles": gen_files,
                    })
                }
                Ok(_) => serde_json::json!({ "text": unmasked_content }),
                Err(e) => {
                    log::error!("Failed to query generated files: {:#}", e);
                    serde_json::json!({ "text": unmasked_content })
                }
            }
        } else {
            serde_json::json!({ "text": unmasked_content })
        };

        // Save assistant message to DB (with unmasked content)
        let content_json = content_value.to_string();
        log::info!(
            "[finish_agent] Saving message id={} conv={} content_len={}",
            assistant_id, conversation_id, content_json.len()
        );
        match db.insert_message(assistant_id, conversation_id, "assistant", &content_json) {
            Ok(_) => {
                log::info!("[finish_agent] Message saved, emitting message:updated for {}", assistant_id);
                // Emit full message ONLY after DB write succeeds.
                // This prevents the UI from showing a message that would disappear on refresh.
                if let Err(e) = app.emit(
                    "message:updated",
                    serde_json::json!({
                        "id": assistant_id,
                        "conversationId": conversation_id,
                        "role": "assistant",
                        "content": content_value,
                    }),
                ) {
                    log::warn!("Failed to emit message:updated for {}: {}", assistant_id, e);
                }
            }
            Err(e) => {
                log::error!("Failed to save assistant message to DB: {:#}", e);
                // Do NOT emit message:updated — the message didn't persist
            }
        }
    } else {
        log::info!(
            "[AGENT] Skipping empty assistant message for conversation {}, assistant_id={}",
            conversation_id, assistant_id
        );
    }

    // NOTE: streaming:done is NOT emitted here. It is emitted exclusively by
    // AgentGuard::clear() when the entire agent_loop ends (all steps done or
    // confirmation checkpoint reached). This prevents a premature streaming:done
    // during auto-advance between analysis steps, which would cause the frontend
    // to set isStreaming=false while the next step's stream is about to begin.

    // Auto-generate conversation title from first assistant response
    if !unmasked_trimmed.is_empty() {
        if let Ok(msgs) = db.get_messages(conversation_id) {
            let assistant_count = msgs.iter()
                .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
                .count();
            if assistant_count <= 1 {
                let title: String = unmasked_trimmed.chars().take(30).collect();
                let title = title.split('\n').next().unwrap_or(&title).trim().to_string();
                let title = if title.len() < unmasked_content.len() {
                    format!("{}...", title)
                } else {
                    title
                };
                if let Err(e) = db.update_conversation_title(conversation_id, &title) {
                    log::warn!("Failed to auto-update conversation title: {}", e);
                }
                // Notify frontend of the title change
                let _ = app.emit(
                    "conversation:title-updated",
                    serde_json::json!({
                        "conversationId": conversation_id,
                        "title": title,
                    }),
                );
            }
        }
    }
}

/// Truncate text for UI display purposes.
fn truncate_for_ui(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let mut truncated = text.chars().take(max_len).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

/// Stop the streaming response for a specific conversation.
#[tauri::command]
pub async fn stop_streaming(
    gateway: State<'_, Arc<LlmGateway>>,
    conversation_id: String,
) -> Result<(), String> {
    gateway.cancel_conversation(&conversation_id).await.map_err(|e| e.to_string())
}

/// Get messages for a conversation.
/// Returns messages as JSON array (the frontend Message[] type).
#[tauri::command]
pub async fn get_messages(
    db: State<'_, Arc<AppStorage>>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_messages(&conversation_id).map_err(|e| e.to_string())
}

/// Create a new conversation.
#[tauri::command]
pub async fn create_conversation(
    db: State<'_, Arc<AppStorage>>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.create_conversation(&id, "New Conversation")
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// Delete a conversation and clean up associated files on disk.
///
/// 1. Queries all `stored_path` values from `uploaded_files` and
///    `generated_files` for this conversation.
/// 2. Deletes those physical files from the workspace.
/// 3. Deletes the conversation from DB (CASCADE removes related rows).
#[tauri::command]
pub async fn delete_conversation(
    db: State<'_, Arc<AppStorage>>,
    gateway: State<'_, Arc<LlmGateway>>,
    file_mgr: State<'_, Arc<FileManager>>,
    app: AppHandle,
    conversation_id: String,
) -> Result<(), String> {
    // Guard: if an agent is running on this conversation, cancel it first
    if gateway.is_conversation_busy(&conversation_id).await {
        log::info!("delete_conversation: cancelling active agent for conversation {}", conversation_id);
        gateway.cancel_conversation(&conversation_id).await.ok();
        // Give the agent loop a moment to exit gracefully
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Force-clear the gateway state in case the loop didn't exit
        gateway.clear_task(&conversation_id).await;
        db.remove_active_task(&conversation_id).ok();
        let _ = app.emit("streaming:done", serde_json::json!({
            "conversationId": conversation_id,
            "messageId": "",
        }));
        let _ = app.emit("agent:idle", serde_json::json!({
            "conversationId": conversation_id,
        }));
    }

    // 1. Collect physical file paths before CASCADE delete removes DB rows
    let file_paths = db.get_file_paths_for_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;

    // 2. Delete physical files (best-effort — don't fail if a file is already gone)
    let mut deleted = 0usize;
    let mut failed = 0usize;
    for path in &file_paths {
        let full_path = file_mgr.full_path(path);
        match std::fs::remove_file(&full_path) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Already gone — fine
            }
            Err(e) => {
                log::warn!("Failed to delete file {:?}: {}", full_path, e);
                failed += 1;
            }
        }
    }
    if !file_paths.is_empty() {
        log::info!(
            "Conversation {} file cleanup: {} deleted, {} failed, {} already gone",
            conversation_id, deleted, failed, file_paths.len() - deleted - failed
        );
    }

    // 3. Delete conversation (CASCADE removes uploaded_files, generated_files, messages, analysis_states)
    db.delete_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

/// Get all conversations.
#[tauri::command]
pub async fn get_conversations(
    db: State<'_, Arc<AppStorage>>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_conversations().map_err(|e| e.to_string())
}

/// Attempt to decrypt an API key. Falls back to returning the raw value
/// if it's not in encrypted format or decryption fails.
/// If decryption fails for an encrypted value (contains ':'), return empty
/// string so the caller can fall back to defaults.
fn decrypt_key(ss: &SecureStorage, value: &str) -> String {
    if value.is_empty() || !value.contains(':') {
        log::info!("decrypt_key: value has no colon (len={}), returning as-is", value.len());
        return value.to_string();
    }
    match ss.decrypt(value) {
        Ok(plaintext) => {
            log::info!("decrypt_key: decryption OK, plaintext len={}", plaintext.len());
            plaintext
        }
        Err(e) => {
            log::warn!("Failed to decrypt API key (err={}), returning empty to trigger default fallback", e);
            String::new()
        }
    }
}

/// Strip DeepSeek internal thinking markers from content deltas.
///
/// DeepSeek models sometimes leak `<｜end▁of▁thinking｜>` and similar
/// markers into the regular content stream. These should not be shown
/// to users.
fn strip_thinking_markers(text: &str) -> String {
    text.replace("<｜end▁of▁thinking｜>", "")
        .replace("<｜begin▁of▁thinking｜>", "")
        .replace("<|end▁of▁thinking|>", "")
        .replace("<|begin▁of▁thinking|>", "")
}
