use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use futures::StreamExt;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;
use crate::llm::gateway::LlmGateway;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent};
use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::orchestrator::{self, StepConfig, StepStatus};
use crate::llm::prompts;
use crate::llm::prompt_guard;
use crate::plugin::{PluginContext, ToolRegistry, SkillRegistry};
use crate::plugin::skill_trait::{SkillState, StepAction};
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

/// Find the largest byte index <= `max_bytes` that falls on a UTF-8 char boundary.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Auto-capture step context before message history wipe.
///
/// Extracts the last assistant message(s) and key tool result snippets (execute_python
/// stdout) from the current message history, then saves them as a structured note.
/// This ensures each step's output is preserved even if the LLM forgets to call
/// `save_analysis_note`.
///
/// Max output: 4000 chars to prevent context bloat.
fn auto_capture_step_context(
    db: &AppStorage,
    conversation_id: &str,
    step_num: u32,
    messages: &[ChatMessage],
) {
    const MAX_CONTEXT_CHARS: usize = 6000;

    let mut context_parts: Vec<String> = Vec::new();

    // 1. Extract assistant messages (the step's analysis conclusions)
    // Capture up to 2 substantive assistant messages (the last one often contains
    // the step summary while the second-to-last contains intermediate findings).
    let mut assistant_count = 0;
    for msg in messages.iter().rev() {
        if msg.role == "assistant" && !msg.content.trim().is_empty() {
            if msg.content.len() > 20 {
                context_parts.push(format!("[分析结论]\n{}", msg.content));
                assistant_count += 1;
                if assistant_count >= 2 {
                    break;
                }
            }
        }
    }

    // 2. Extract key tool result outputs (execute_python stdout, truncated)
    let mut tool_outputs: Vec<String> = Vec::new();
    for msg in messages.iter() {
        if msg.role == "tool" && msg.name.as_deref() == Some("execute_python") {
            let compressed = compress_tool_result(&msg.content);
            let trimmed = compressed.trim();
            if !trimmed.is_empty() && trimmed.len() > 10 {
                // Keep first 1500 chars of each tool output (increased from 800)
                let snippet = if trimmed.len() > 1500 {
                    let end = truncate_at_char_boundary(trimmed, 1500);
                    format!("{}...(truncated)", &trimmed[..end])
                } else {
                    trimmed.to_string()
                };
                tool_outputs.push(snippet);
            }
        }
    }

    if !tool_outputs.is_empty() {
        // Keep up to 5 most recent tool outputs (increased from 3)
        let recent_outputs: Vec<&String> = tool_outputs.iter().rev().take(5).collect();
        let mut tool_section = String::from("[关键数据输出]\n");
        for output in recent_outputs.into_iter().rev() {
            tool_section.push_str(output);
            tool_section.push_str("\n---\n");
        }
        context_parts.push(tool_section);
    }

    if context_parts.is_empty() {
        log::warn!("[auto_capture] No content to capture for step {} in conversation {}", step_num, conversation_id);
        return;
    }

    // Combine and truncate to MAX_CONTEXT_CHARS
    let mut combined = context_parts.join("\n\n");
    if combined.len() > MAX_CONTEXT_CHARS {
        let end = truncate_at_char_boundary(&combined, MAX_CONTEXT_CHARS);
        combined.truncate(end);
        combined.push_str("\n...(auto-truncated)");
    }

    let note_key = format!("note:{}:step{}_auto_context", conversation_id, step_num);
    match db.set_memory(&note_key, &combined, Some("auto_capture")) {
        Ok(_) => log::info!("[auto_capture] Saved step {} auto_context ({} chars) for conversation {}",
            step_num, combined.len(), conversation_id),
        Err(e) => log::warn!("[auto_capture] Failed to save step {} auto_context: {}", step_num, e),
    }
}

/// Detect if a user message is a general daily question unrelated to the
/// current analysis workflow.
///
/// Returns `true` when the message looks like a general HR question or
/// casual chat, not a step confirmation, abort, or analysis feedback.
/// Used during `confirming`/`analyzing` mode to allow daily chat without
/// leaving the analysis workflow.
fn is_daily_question(text: &str) -> bool {
    let trimmed = text.trim();

    // Short messages (≤20 chars) are likely confirmations or abort — don't intercept
    if trimmed.chars().count() <= 20 {
        return false;
    }

    // If the message matches confirmation or abort keywords, it's not a daily question
    if crate::plugin::skill_trait::is_confirm_keyword(trimmed)
        || crate::plugin::skill_trait::is_abort_keyword(trimmed)
    {
        return false;
    }

    let lower = trimmed.to_lowercase();

    // Patterns that indicate a general question (not analysis feedback)
    let question_patterns = [
        "请问", "什么是", "怎么", "如何", "能不能",
        "帮我查", "帮忙查", "有没有",
        "是什么意思", "是多少", "怎么算", "怎么计算",
        "政策", "规定", "法规", "标准",
        "社保", "公积金", "个税", "年假", "病假", "产假",
        "劳动法", "劳动合同", "试用期", "离职", "辞退",
        "what is", "how to", "how do", "can you",
        "please explain", "tell me about",
    ];

    // Messages containing question patterns are likely daily questions
    if question_patterns.iter().any(|p| lower.contains(p)) {
        // But exclude if they also contain analysis-specific feedback terms
        let feedback_patterns = [
            "这一步", "上一步", "当前步骤", "分析结果", "重新分析",
            "调整", "修改", "补充", "岗位族", "职级", "公平性",
        ];
        if feedback_patterns.iter().any(|p| lower.contains(p)) {
            return false; // Analysis feedback, not a daily question
        }
        return true;
    }

    false
}

/// Clear all analysis-related notes for a conversation.
///
/// Called when analysis finishes (Finish) or is aborted (Abort) to prevent
/// stale notes from polluting a future re-analysis in the same conversation.
/// Cleans up: step checkpoints, auto_context, summaries, analysis_direction,
/// and the active_skill marker.
///
/// Also stores a completion timestamp to enable cooldown (P4).
fn clear_analysis_notes(db: &AppStorage, conversation_id: &str) {
    let prefix = format!("note:{}:", conversation_id);
    match db.delete_memories_by_prefix(&prefix) {
        Ok(count) => {
            if count > 0 {
                log::info!(
                    "[cleanup] Cleared {} analysis notes for conversation {}",
                    count, conversation_id
                );
            }
        }
        Err(e) => log::warn!(
            "[cleanup] Failed to clear analysis notes for {}: {}",
            conversation_id, e
        ),
    }

    // Store completion timestamp for cooldown detection
    let cooldown_key = format!("note:{}:analysis_completed_at", conversation_id);
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = db.set_memory(&cooldown_key, &now, Some("system")) {
        log::warn!("[cleanup] Failed to store analysis cooldown timestamp: {}", e);
    }
}

/// Build a [`StepConfig`] from an active Skill and its current state.
///
/// Replaces `orchestrator::build_step_config()` — configuration now comes
/// from the Skill plugin rather than hardcoded step tables.
async fn build_config_from_skill(
    skill: &dyn crate::plugin::Skill,
    state: &SkillState,
    tool_registry: &ToolRegistry,
) -> StepConfig {
    let step_num = state.current_step.as_deref()
        .and_then(|s| s.strip_prefix("step"))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);

    let tool_filter = skill.tool_filter(state);
    let tool_defs = tool_registry.get_schemas_filtered(&tool_filter).await;

    // Build step display names from the workflow definition
    let step_display_names = skill.workflow()
        .map(|wf| {
            wf.steps.iter().enumerate().map(|(i, s)| {
                (i as u32, s.display_name.clone())
            }).collect()
        })
        .unwrap_or_default();

    StepConfig {
        step: step_num,
        system_prompt: skill.system_prompt(state),
        tool_defs,
        max_iterations: skill.max_iterations(state),
        requires_confirmation: true,
        token_budget: skill.token_budget(state),
        step_display_names,
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
            self.gateway.clear_task(&self.conversation_id);
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
            // Synchronous cleanup — gateway.clear_task() is now sync (std::sync::Mutex)
            self.db.remove_active_task(&self.conversation_id).ok();
            self.gateway.clear_task(&self.conversation_id);

            // Event emission is also sync (Tauri emit is sync)
            let _ = self.app.emit("streaming:done", serde_json::json!({
                "conversationId": self.conversation_id,
                "messageId": "",
            }));
            let _ = self.app.emit("agent:idle", serde_json::json!({
                "conversationId": self.conversation_id
            }));
            log::info!("[AgentGuard] Drop fallback: cleared active task for conversation {} and emitted streaming:done + agent:idle", self.conversation_id);
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
    Ok(gateway.get_busy_conversations())
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
    tool_registry: State<'_, Arc<ToolRegistry>>,
    skill_registry: State<'_, Arc<SkillRegistry>>,
    app: AppHandle,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
) -> Result<(), String> {
    log::info!("=== send_message START === conversation_id={}, content_len={}, file_ids={:?}",
        conversation_id, content.len(), file_ids);

    // Guard: reject if THIS conversation is already busy
    if gateway.is_conversation_busy(&conversation_id) {
        log::warn!("send_message rejected: conversation {} is already processing", conversation_id);
        return Err("This conversation is already processing.".to_string());
    }

    // Guard: reject if max concurrent agents reached
    let busy_count = gateway.get_busy_conversations().len();
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

    // Log key metadata only — NEVER log key characters (security)
    let raw_pk_len = settings.primary_api_key.len();
    let raw_pk_has_colon = settings.primary_api_key.contains(':');
    log::info!("Raw primary_api_key: len={}, contains_colon={}", raw_pk_len, raw_pk_has_colon);

    // Decrypt API keys if SecureStorage is available
    if let Some(ss) = crypto.as_ref() {
        log::info!("SecureStorage available, decrypting API keys...");
        settings.primary_api_key = decrypt_key(ss, &settings.primary_api_key);
        settings.tavily_api_key = decrypt_key(ss, &settings.tavily_api_key);
        settings.bocha_api_key = decrypt_key(ss, &settings.bocha_api_key);
    } else {
        log::warn!("SecureStorage NOT available, using raw key values");
    }

    log::info!("After decryption: primary_api_key len={}", settings.primary_api_key.len());

    // Fall back to built-in default key ONLY for DeepSeek provider
    // (the built-in key is a DeepSeek key, using it for other providers would cause 401)
    if settings.primary_api_key.is_empty() && settings.primary_model == "deepseek-v3" {
        log::info!("Primary API key empty for DeepSeek, falling back to built-in default key");
        let defaults = AppSettings::default();
        settings.primary_api_key = defaults.primary_api_key.clone();
    }

    log::info!("Final primary_api_key: len={}", settings.primary_api_key.len());

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

    // 8. Determine conversation mode and use the Skill system for routing
    //
    // The conversation.mode column tracks workflow state:
    //   "daily"      → normal chat (may detect Skill activation)
    //   "confirming" → workflow step 0 done, waiting for user to confirm direction
    //   "analyzing"  → workflow steps 1–5 in progress
    //
    // The SkillRegistry replaces the hardcoded orchestrator:
    //   - detect_activation() replaces detect_analysis_mode()
    //   - Skill.on_step_complete() replaces next_action()
    //   - Skill methods provide prompt, tool filter, iterations, budget
    let has_files = !file_attachments.is_empty();
    let conversation_mode = db.get_conversation_mode(&conversation_id)
        .unwrap_or_else(|_| "daily".to_string());

    log::info!("Conversation {} mode='{}', has_files={}", conversation_id, conversation_mode, has_files);

    let step_config: Option<StepConfig> = match conversation_mode.as_str() {
        "daily" => {
            // First, check if there's a pending skill activation from the previous message.
            // This is set when detect_activation fires but we want user confirmation first.
            let pending_key = format!("note:{}:pending_skill", conversation_id);
            let pending_skill_id = db.get_memory(&pending_key).ok().flatten();

            if let Some(ref skill_id) = pending_skill_id {
                // Clear the pending marker regardless of outcome
                if let Err(e) = db.delete_memories_by_prefix(&pending_key) {
                    log::warn!("Failed to clear pending_skill: {}", e);
                }

                // Check if user confirmed the activation
                if crate::plugin::skill_trait::is_confirm_keyword(&content)
                    || crate::plugin::skill_trait::is_abort_keyword(&content)
                {
                    if crate::plugin::skill_trait::is_abort_keyword(&content) {
                        log::info!("User rejected pending skill activation for conversation {}", conversation_id);
                        None // process as daily
                    } else {
                        // User confirmed → proceed with activation
                        log::info!("User confirmed pending skill '{}' for conversation {}", skill_id, conversation_id);
                        let skill = skill_registry.get(skill_id).await;
                        match skill {
                            Some(skill) if skill.workflow().is_some() => {
                                let workflow = skill.workflow().unwrap();
                                let initial_step = &workflow.initial_step;
                                let step_num = initial_step.strip_prefix("step")
                                    .and_then(|n| n.parse::<u32>().ok())
                                    .unwrap_or(0);

                                if let Err(e) = db.set_conversation_mode(&conversation_id, "confirming") {
                                    log::error!("Failed to set mode to 'confirming': {}", e);
                                }
                                let skill_key = format!("note:{}:active_skill", conversation_id);
                                if let Err(e) = db.set_memory(&skill_key, skill_id, Some("system")) {
                                    log::error!("Failed to store active skill ID: {}", e);
                                }
                                if let Err(e) = orchestrator::advance_step(
                                    &db.inner().clone(), &conversation_id, step_num, "in_progress",
                                ) {
                                    log::error!("Failed to mark step {} as in_progress: {}", step_num, e);
                                }

                                let mut state = SkillState::new(skill_id);
                                state.current_step = Some(initial_step.clone());
                                Some(build_config_from_skill(&*skill, &state, &tool_registry).await)
                            }
                            _ => {
                                log::warn!("Pending skill '{}' not found or has no workflow", skill_id);
                                None
                            }
                        }
                    }
                } else {
                    // User sent a non-confirmation message → treat as daily, ignore pending
                    log::info!("Pending skill '{}' expired (user sent non-confirmation), processing as daily", skill_id);
                    None
                }
            } else {
            // No pending skill — check if a Skill with a workflow should activate.
            // P4: Skip detection during cooldown period after recent analysis.
            let in_cooldown = {
                let cooldown_key = format!("note:{}:analysis_completed_at", conversation_id);
                db.get_memory(&cooldown_key).ok().flatten().map_or(false, |ts| {
                    chrono::DateTime::parse_from_rfc3339(&ts).map_or(false, |completed_at| {
                        let elapsed = chrono::Utc::now().signed_duration_since(completed_at);
                        elapsed.num_seconds() < 60 // 60-second cooldown
                    })
                })
            };

            let detected = if in_cooldown {
                log::info!(
                    "Skipping skill detection for conversation {} (analysis cooldown active)",
                    conversation_id
                );
                None
            } else {
                skill_registry.detect_activation(
                    &content, has_files, skill_registry.default_skill_id()
                ).await
            };

            if let Some(skill_id) = detected {
                let skill = skill_registry.get(&skill_id).await;
                match skill {
                    Some(skill) if skill.workflow().is_some() => {
                        log::info!(
                            "Skill '{}' detected for conversation {}, asking user for confirmation",
                            skill_id, conversation_id
                        );

                        // Store as pending — don't activate yet
                        if let Err(e) = db.set_memory(&pending_key, &skill_id, Some("system")) {
                            log::error!("Failed to store pending skill ID: {}", e);
                        }

                        // Save a confirmation message to the chat
                        let skill_display = skill.display_name();
                        let confirm_msg = format!(
                            "检测到您可能想要进行「{}」。如果确认开始，请回复「确认」或「开始」；如果不需要，直接继续提问即可。",
                            skill_display
                        );
                        let confirm_id = uuid::Uuid::new_v4().to_string();
                        let content_json = serde_json::json!({"text": confirm_msg}).to_string();

                        if let Err(e) = db.insert_message(&confirm_id, &conversation_id, "assistant", &content_json) {
                            log::error!("Failed to save confirmation message: {}", e);
                        }
                        if let Err(e) = app.emit("message:updated", serde_json::json!({
                            "id": confirm_id,
                            "conversationId": conversation_id,
                            "role": "assistant",
                            "content": {"text": confirm_msg},
                        })) {
                            log::warn!("Failed to emit confirmation message: {}", e);
                        }

                        // Return Ok early — skip agent_loop for this turn
                        return Ok(());
                    }
                    _ => {
                        log::debug!("No workflow skill activated, staying in daily mode");
                        None
                    }
                }
            } else {
                None // daily chat, no analysis
            }
            }
        }

        "confirming" | "analyzing" => {
            // P2: Allow daily chat during analysis — detect general questions
            // that are unrelated to the current analysis workflow.
            // If the message looks like a daily question (not confirmation, not abort,
            // and matches daily-question patterns), handle it as daily chat without
            // changing the conversation mode.
            if is_daily_question(&content) {
                log::info!(
                    "Detected daily question during '{}' mode for conversation {}, routing to daily chat",
                    conversation_mode, conversation_id
                );
                None // step_config = None → daily mode agent_loop
            } else {
            // Active workflow: look up the stored skill ID for this conversation.
            let skill_key = format!("note:{}:active_skill", conversation_id);
            let active_skill_id = match db.get_memory(&skill_key).ok().flatten() {
                Some(id) => id,
                None => {
                    // No skill ID found — memory lost (crash, corruption).
                    // Reset to daily mode rather than assuming comp-analysis.
                    log::warn!(
                        "No active_skill stored for conversation {} in mode '{}', resetting to daily",
                        conversation_id, conversation_mode
                    );
                    if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                        log::error!("Failed to reset mode to 'daily': {}", e);
                    }
                    // Fall through to daily mode (step_config = None)
                    String::new()
                }
            };

            // If skill ID was lost, skip workflow lookup and run in daily mode
            if active_skill_id.is_empty() {
                None
            } else {
            match skill_registry.get(&active_skill_id).await {
                None => {
                    log::error!(
                        "Active skill '{}' not found for conversation {}, falling back to daily mode",
                        active_skill_id, conversation_id
                    );
                    if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                        log::error!("Failed to reset mode to 'daily': {}", e);
                    }
                    None
                }
                Some(skill) => {

            let db_step_state = orchestrator::get_step_state(&db, &conversation_id);
            let step_num = db_step_state.as_ref().map(|s| s.step).unwrap_or(0);

            let mut skill_state = SkillState::new(skill.id());
            skill_state.current_step = Some(format!("step{}", step_num));

            // Route based on step status
            match db_step_state.as_ref().map(|s| &s.status) {
                Some(&StepStatus::Paused) => {
                    // Crash recovery: resume paused step
                    log::info!("Resuming paused step {} for conversation {}",
                        step_num, conversation_id);

                    if let Err(e) = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, step_num, "in_progress",
                    ) {
                        log::error!("Failed to mark step {} as in_progress: {}", step_num, e);
                    }

                    Some(build_config_from_skill(&*skill, &skill_state, &tool_registry).await)
                }

                Some(&StepStatus::InProgress) => {
                    // Edge case: user sent message while step is still running.
                    let action = skill.on_step_complete(&mut skill_state, &content);

                    if matches!(action, StepAction::Abort) {
                        log::info!("User aborted analysis (in_progress) for conversation {}",
                            conversation_id);
                        if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                            log::error!("Failed to set mode to 'daily': {}", e);
                        }
                        if let Err(e) = db.finalize_analysis(&conversation_id, "aborted") {
                            log::error!("Failed to finalize analysis as aborted: {}", e);
                        }
                        clear_analysis_notes(&db, &conversation_id);
                        None
                    } else {
                        log::warn!(
                            "User sent message while step {} is in_progress; re-running",
                            step_num
                        );
                        if let Err(e) = orchestrator::advance_step(
                            &db.inner().clone(), &conversation_id, step_num, "in_progress",
                        ) {
                            log::error!("Failed to mark step {} as in_progress: {}", step_num, e);
                        }
                        Some(build_config_from_skill(&*skill, &skill_state, &tool_registry).await)
                    }
                }

                Some(&StepStatus::Completed) | None => {
                    let action = skill.on_step_complete(&mut skill_state, &content);
                    log::info!("Skill action for conversation {} step {}: {:?}",
                        conversation_id, step_num, std::mem::discriminant(&action));

                    match action {
                        StepAction::Abort => {
                            log::info!("User aborted analysis for conversation {}",
                                conversation_id);
                            if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                                log::error!("Failed to set mode to 'daily': {}", e);
                            }
                            if let Err(e) = db.finalize_analysis(&conversation_id, "aborted") {
                                log::error!("Failed to finalize analysis as aborted: {}", e);
                            }
                            clear_analysis_notes(&db, &conversation_id);
                            None
                        }

                        StepAction::Finish => {
                            log::info!("Analysis complete for conversation {}",
                                conversation_id);
                            if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                                log::error!("Failed to set mode to 'daily': {}", e);
                            }
                            if let Err(e) = db.finalize_analysis(&conversation_id, "completed") {
                                log::error!("Failed to finalize analysis: {}", e);
                            }
                            clear_analysis_notes(&db, &conversation_id);
                            None
                        }

                        StepAction::AdvanceToStep(next_step_id) => {
                            let next_step_num = next_step_id.strip_prefix("step")
                                .and_then(|n| n.parse::<u32>().ok());

                            if next_step_num.is_none() {
                                log::warn!(
                                    "Invalid step ID '{}' from skill '{}', expected 'stepN' format; falling back to step {}",
                                    next_step_id, active_skill_id, step_num + 1
                                );
                            }
                            let next_step_num = next_step_num.unwrap_or(step_num + 1);

                            log::info!("Advancing to step {} for conversation {}",
                                next_step_num, conversation_id);

                            // Save direction note for step 0 → step 1 transition
                            if step_num == 0 {
                                let direction = content.trim();
                                if !direction.is_empty() {
                                    let note_key = format!("note:{}:analysis_direction", conversation_id);
                                    match db.set_memory(&note_key, direction, Some("step_0_direction")) {
                                        Ok(_) => log::info!("Saved analysis direction: '{}'", direction),
                                        Err(e) => log::warn!("Failed to save analysis direction: {}", e),
                                    }
                                }
                            }

                            // Transition mode: confirming → analyzing (for step 0 → step 1)
                            if conversation_mode == "confirming" {
                                if let Err(e) = db.set_conversation_mode(&conversation_id, "analyzing") {
                                    log::error!("Failed to set mode to 'analyzing': {}", e);
                                }
                            }

                            // Mark current step completed, new step in_progress
                            if let Err(e) = orchestrator::advance_step(
                                &db.inner().clone(), &conversation_id, step_num, "completed",
                            ) {
                                log::error!("Failed to mark step {} as completed: {}", step_num, e);
                            }
                            if let Err(e) = orchestrator::advance_step(
                                &db.inner().clone(), &conversation_id, next_step_num, "in_progress",
                            ) {
                                log::error!("Failed to mark step {} as in_progress: {}", next_step_num, e);
                            }

                            // Emit step-reset so the frontend:
                            // 1. Resets the streaming content for the new step
                            // 2. Updates the watchdog activity timer (prevents 30s timeout
                            //    during checkpoint extraction which can take up to 30s)
                            let _ = app.emit("streaming:step-reset", serde_json::json!({
                                "conversationId": conversation_id,
                                "step": next_step_num,
                            }));

                            // --- Checkpoint extraction (Layer 1) ---
                            // Make a dedicated non-streaming LLM call to extract structured
                            // step conclusions BEFORE wiping message history. Falls back to
                            // auto_capture on any failure. Runs for ALL steps (including step 0
                            // which carries critical file info and analysis direction).
                            {
                                let (base_ep, step_ep) = skill.extract_prompt(&format!("step{}", step_num));
                                let extract_prompt = if base_ep.is_empty() && step_ep.is_empty() {
                                    String::new()
                                } else {
                                    format!("{}\n\n{}", base_ep, step_ep)
                                };
                                let cp_result = crate::llm::checkpoint::checkpoint_extract(
                                    &gateway, &settings, &conversation_id, step_num, &chat_messages, &db, &extract_prompt,
                                ).await;
                                if cp_result.is_some() {
                                    log::info!("[step_advance] Checkpoint extraction succeeded for step {}", step_num);
                                } else {
                                    log::warn!("[step_advance] Checkpoint extraction failed for step {}, auto_capture will serve as fallback", step_num);
                                }

                                // --- Auto-capture (Layer 3) --- always runs as fallback
                                auto_capture_step_context(&db, &conversation_id, step_num, &chat_messages);
                            }

                            // Sub-agent context reset
                            let original_user_msg = chat_messages.iter()
                                .find(|m| m.role == "user")
                                .cloned();
                            let step_summary = if step_num == 0 {
                                format!(
                                    "分析方向已确认。关键信息已保存到分析记录中。\n\n\
                                    请查看系统提示词中 [前序分析记录] 部分获取之前的完整数据。\n\
                                    所有数据分析必须使用 execute_python 基于实际数据执行，禁止凭空推断。\n\
                                    如需读取数据文件，请使用系统提示词中[本次会话的文件]部分提供的文件信息。\n\n\
                                    请开始执行第 {} 步。",
                                    next_step_num
                                )
                            } else {
                                format!(
                                    "前 {} 步分析已完成。关键结论和数据已保存到分析记录中。\n\n\
                                    请查看系统提示词中 [前序分析记录] 部分获取之前步骤的完整数据（包括字段映射、岗位归一化结果、职级框架等）。\n\
                                    所有数据分析必须使用 execute_python 基于实际数据执行，禁止凭空推断。\n\
                                    如需读取数据文件，请使用系统提示词中[本次会话的文件]部分提供的文件信息。\n\n\
                                    请开始执行第 {} 步。",
                                    step_num, next_step_num
                                )
                            };
                            chat_messages = Vec::new();
                            if let Some(user_msg) = original_user_msg {
                                chat_messages.push(user_msg);
                            }
                            chat_messages.push(ChatMessage::text("assistant", &step_summary));

                            // Build config for the new step
                            let mut new_state = SkillState::new(skill.id());
                            new_state.current_step = Some(next_step_id);

                            Some(build_config_from_skill(&*skill, &new_state, &tool_registry).await)
                        }

                        StepAction::WaitForUser => {
                            log::info!("Re-running step {} with user feedback for conversation {}",
                                step_num, conversation_id);

                            if let Err(e) = orchestrator::advance_step(
                                &db.inner().clone(), &conversation_id, step_num, "in_progress",
                            ) {
                                log::error!("Failed to mark step {} as in_progress: {}", step_num, e);
                            }

                            Some(build_config_from_skill(&*skill, &skill_state, &tool_registry).await)
                        }
                    }
                }
            }

                } // Some(skill)
            } // match skill_registry.get
            } // else (active_skill_id not empty)
            } // else (not daily question)
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
    let tool_registry_clone = tool_registry.inner().clone();
    let app_clone = app.clone();

    // 9. Spawn the agent loop in a background task with guard and timeout
    log::info!("=== Spawning agent_loop === assistant_id={}, analysis_step={:?}",
        assistant_id, step_config.as_ref().map(|c| c.step));

    // Mark gateway as busy BEFORE spawning so concurrent calls are blocked immediately
    gateway.set_busy(&conversation_id).map_err(|e| e)?;

    // Record in DB for crash recovery. If this fails, rollback the gateway busy state.
    if let Err(e) = db.insert_active_task(&conversation_id) {
        log::error!("Failed to insert active task, rolling back gateway busy state: {}", e);
        gateway.clear_task(&conversation_id);
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
                tool_registry_clone,
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
                guard.gateway.cancel_conversation(&conversation_id_clone).ok();
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
    tool_registry: Arc<ToolRegistry>,
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
    let bocha_api_key = if settings.bocha_api_key.is_empty() {
        None
    } else {
        Some(settings.bocha_api_key.clone())
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
    // In daily mode, get all tool schemas from the registry.
    let all_tool_defs = tool_registry.get_all_schemas().await;
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
                config.token_budget,
                180u64,  // analysis mode: tools take time, LLM thinks longer
            )
        }
        None => {
            log::info!("Agent loop in DAILY CONSULTATION mode");
            (
                prompts::get_system_prompt(None),
                Some(all_tool_defs), // use all registered tools from registry
                MAX_TOOL_ITERATIONS,
                4096u32, // daily consultation: standard budget
                CHUNK_TIMEOUT_SECS, // daily mode: 90s
            )
        }
    };

    // Append file context to system prompt so LLM always has file info
    let system_prompt = format!("{}{}", system_prompt, file_context);

    // Inject accumulated analysis notes from previous steps into the system prompt.
    // Three-layer priority: checkpoint (structured) > summary (LLM-curated) > auto_context (fallback).
    // Checkpoints use field-level decay: summary/key_findings/next_step_input never truncated,
    // data_artifacts truncated for older steps.
    let analysis_notes_context = {
        let notes_prefix = format!("note:{}:", conversation_id);
        match db.get_memories_by_prefix(&notes_prefix) {
            Ok(notes) if !notes.is_empty() => {
                let current_step = current_step_config.as_ref().map(|c| c.step).unwrap_or(0);

                // Separate notes by type: checkpoint, step-grouped, non-step
                let mut checkpoints: std::collections::BTreeMap<u32, crate::llm::checkpoint::StepCheckpoint> = std::collections::BTreeMap::new();
                let mut step_notes: std::collections::BTreeMap<u32, Vec<(String, String)>> = std::collections::BTreeMap::new();
                let mut non_step_notes: Vec<(String, String)> = Vec::new();

                for (key, value) in &notes {
                    let note_name = key.strip_prefix(&notes_prefix).unwrap_or(key);

                    // Try to parse checkpoint notes (highest priority)
                    if note_name.starts_with("step") && note_name.ends_with("_checkpoint") {
                        if let Some(step_str) = note_name.strip_prefix("step") {
                            if let Some(num_str) = step_str.strip_suffix("_checkpoint") {
                                if let Ok(sn) = num_str.parse::<u32>() {
                                    if let Ok(cp) = serde_json::from_str::<crate::llm::checkpoint::StepCheckpoint>(value) {
                                        checkpoints.insert(sn, cp);
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // Group remaining notes by step number
                    if note_name.starts_with("step") {
                        if let Some(step_str) = note_name.strip_prefix("step") {
                            if let Some(sn) = step_str.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok() {
                                step_notes.entry(sn).or_default().push((note_name.to_string(), value.clone()));
                                continue;
                            }
                        }
                    }
                    non_step_notes.push((note_name.to_string(), value.clone()));
                }

                let mut ctx = String::from("\n\n[前序分析记录]\n");
                ctx.push_str("⚠️ 重要：以下是之前步骤保存的分析结论和关键数据，是当前步骤的唯一数据来源。\n");
                ctx.push_str("· 当前步骤必须基于这些记录继续分析\n");
                ctx.push_str("· 所有数据必须来自 execute_python 实际执行，禁止凭空推断\n");
                ctx.push_str("· 当前步骤结束前必须调用 save_analysis_note 保存关键结论，否则下一步将丢失数据\n\n");

                // Non-step notes (e.g., analysis_direction) — always full
                for (name, value) in &non_step_notes {
                    ctx.push_str(&format!("### {}\n{}\n\n", name, value));
                }

                // Collect all step numbers that have any notes
                let all_steps: std::collections::BTreeSet<u32> = checkpoints.keys()
                    .chain(step_notes.keys())
                    .copied()
                    .collect();

                let max_completed_step = if current_step > 0 { current_step - 1 } else { 0 };
                const OLDER_STEP_MAX_CHARS: usize = 3000;
                const RECENT_STEP_MAX_CHARS: usize = 6000;

                for &sn in &all_steps {
                    // The immediately preceding step is "recent" (full detail);
                    // all earlier steps are "older" (truncated data_artifacts).
                    let is_recent = current_step == 0 || sn >= current_step.saturating_sub(1);

                    // Priority: checkpoint > summary > auto_context
                    if let Some(cp) = checkpoints.get(&sn) {
                        let display_name = current_step_config.as_ref()
                            .and_then(|c| c.step_display_names.iter().find(|(n, _)| *n == sn))
                            .map(|(_, name)| name.as_str())
                            .unwrap_or("未知步骤");
                        ctx.push_str(&crate::llm::checkpoint::format_checkpoint_for_injection(
                            cp,
                            sn,
                            display_name,
                            is_recent,
                        ));
                    } else if let Some(notes_for_step) = step_notes.get(&sn) {
                        // Fallback: use existing note-based injection
                        ctx.push_str(&format!("## 第 {} 步记录\n", sn));

                        for (name, value) in notes_for_step {
                            let is_summary = name.contains("_summary");

                            if is_summary {
                                ctx.push_str(&format!("### {}\n{}\n\n", name, value));
                            } else if is_recent {
                                let truncated = if value.len() > RECENT_STEP_MAX_CHARS {
                                    let end = truncate_at_char_boundary(value, RECENT_STEP_MAX_CHARS);
                                    format!("{}...(truncated)", &value[..end])
                                } else {
                                    value.clone()
                                };
                                ctx.push_str(&format!("### {}\n{}\n\n", name, truncated));
                            } else {
                                let truncated = if value.len() > OLDER_STEP_MAX_CHARS {
                                    let end = truncate_at_char_boundary(value, OLDER_STEP_MAX_CHARS);
                                    format!("{}...(truncated)", &value[..end])
                                } else {
                                    value.clone()
                                };
                                ctx.push_str(&format!("### {}\n{}\n\n", name, truncated));
                            }
                        }
                    }
                }

                log::info!(
                    "[notes_injection] Injected {} notes ({} checkpoints + {} step groups + {} non-step) for conversation {}, current_step={}",
                    notes.len(), checkpoints.len(), step_notes.len(), non_step_notes.len(), conversation_id, current_step
                );

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
                // streaming:done is emitted by AgentGuard::clear() after agent_loop returns
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
                    // streaming:done is emitted by AgentGuard::clear() after agent_loop returns
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
                            // streaming:done is emitted by AgentGuard::clear() after agent_loop returns
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

        let plugin_ctx = PluginContext {
            storage: db.clone(),
            file_manager: file_mgr.clone(),
            workspace_path: workspace_path.clone(),
            conversation_id: conversation_id.clone(),
            tavily_api_key: tavily_api_key.clone(),
            bocha_api_key: bocha_api_key.clone(),
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
            let result = tool_registry.execute(&tc.name, &plugin_ctx, tc.arguments.clone()).await;
            let tool_elapsed = tool_start.elapsed();

            let (result_content, result_is_error) = match result {
                Ok(output) => (output.content, output.is_error),
                Err(e) => {
                    log::error!("Tool '{}' failed: {}", tc.name, e);
                    (format!("Error: {}", e), true)
                }
            };

            log::info!(
                "[AGENT] Tool '{}' result: is_error={}, content_len={}, elapsed={:?}, preview='{}'",
                tc.name, result_is_error, result_content.len(), tool_elapsed,
                truncate_for_ui(&result_content, 300),
            );

            let _ = app.emit(
                "tool:completed",
                serde_json::json!({
                    "conversationId": conversation_id,
                    "toolName": tc.name,
                    "toolId": tc.id,
                    "success": !result_is_error,
                    "summary": truncate_for_ui(&result_content, 200),
                }),
            );

            // Collect fileId from tool results (generate_report, export_data, generate_chart, execute_python)
            if !result_is_error {
                // Try parsing as JSON first (generate_report, export_data, generate_chart return JSON)
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result_content) {
                    if let Some(file_id) = parsed.get("fileId").and_then(|v| v.as_str()) {
                        generated_file_ids.push(file_id.to_string());
                    }
                }
                // Also check for "fileId:" pattern in text output (execute_python auto-registered files).
                for line in result_content.lines() {
                    if let Some(pos) = line.find("fileId:") {
                        let after = &line[pos + 7..].trim_start();
                        let id: String = after.chars()
                            .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
                            .collect();
                        if id.len() >= 36 && !generated_file_ids.contains(&id) {
                            generated_file_ids.push(id);
                        }
                    }
                }
            }

            let masked_result = match combined_mask_ctx.as_mut() {
                Some(ctx) => ctx.mask_text(&result_content),
                None => result_content.clone(),
            };
            // Truncate tool results stored in message history to prevent context bloat.
            // Long execute_python outputs can produce megabytes; cap at 8KB per result.
            const MAX_TOOL_RESULT_CHARS: usize = 8000;
            let truncated_result = if masked_result.len() > MAX_TOOL_RESULT_CHARS {
                let end = truncate_at_char_boundary(&masked_result, MAX_TOOL_RESULT_CHARS);
                format!("{}...\n[output truncated — {} chars total]", &masked_result[..end], masked_result.len())
            } else {
                masked_result
            };
            messages.push(ChatMessage::tool_result(
                &tc.id,
                &tc.name,
                truncated_result,
            ));

            // Check cancel signal between tool executions so we don't wait
            // for all queued tools to finish when the user requests stop.
            if *cancel_rx.borrow() {
                log::info!("[AGENT] Cancel signal detected between tool executions for conversation {}", conversation_id);
                stream_cancelled = true;
                break;
            }
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

        // Validate that the step has analysis notes (either LLM-saved summary or auto-captured context).
        // Auto-capture always saves step{N}_auto_context, so this is a sanity check.
        // Still warn if the LLM didn't save a curated summary (auto_context is less structured).
        if completed_step >= 1 && completed_step <= 5 {
            let note_prefix = format!("note:{}:step{}_", conversation_id, completed_step);
            let has_any_note = db.get_memories_by_prefix(&note_prefix)
                .map(|notes| !notes.is_empty())
                .unwrap_or(false);

            let summary_key = format!("note:{}:step{}_summary", conversation_id, completed_step);
            let has_summary = db.get_memory(&summary_key)
                .ok()
                .flatten()
                .map(|v| !v.is_empty())
                .unwrap_or(false);

            if !has_any_note {
                log::warn!(
                    "[AGENT] Step {} completed without ANY notes (prefix '{}') — auto_capture should have saved context as fallback",
                    completed_step, note_prefix
                );
                // NOTE: Do NOT emit streaming:error here. The auto_capture system
                // saves step context as fallback. Emitting streaming:error causes
                // the frontend to clear streaming state, which can interfere with
                // the next step's streaming when the step auto-advances.
            } else if !has_summary {
                log::info!(
                    "[AGENT] Step {} has auto-captured context but no LLM-curated summary. Auto-capture will serve as fallback.",
                    completed_step
                );
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
    gateway.cancel_conversation(&conversation_id).map_err(|e| e.to_string())
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
    if gateway.is_conversation_busy(&conversation_id) {
        log::info!("delete_conversation: cancelling active agent for conversation {}", conversation_id);
        gateway.cancel_conversation(&conversation_id).ok();
        // Give the agent loop a moment to exit gracefully
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Force-clear the gateway state in case the loop didn't exit
        gateway.clear_task(&conversation_id);
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
