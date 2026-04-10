use super::*;

pub(crate) fn build_knowledge_preamble(knowledge_dir: &Path) -> String {
    let mut entries: Vec<(String, String)> = Vec::new();

    let read_dir = match std::fs::read_dir(knowledge_dir) {
        Ok(rd) => rd,
        Err(e) => {
            log::warn!(
                "Failed to read knowledge dir '{}': {}",
                knowledge_dir.display(),
                e
            );
            return String::new();
        }
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Validate key contains only safe characters (prevent Python code injection via filename)
        if !stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            log::warn!("Unsafe knowledge filename '{}', skipping", stem);
            continue;
        }
        // Skip files larger than 5MB to prevent memory issues
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 5 * 1024 * 1024 {
                log::warn!(
                    "Knowledge file '{}' too large ({} bytes), skipping",
                    path.display(),
                    meta.len()
                );
                continue;
            }
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to read knowledge file '{}': {}", path.display(), e);
                continue;
            }
        };
        // Validate that it's valid JSON
        if serde_json::from_str::<serde_json::Value>(&content).is_err() {
            log::warn!(
                "Invalid JSON in knowledge file '{}', skipping",
                path.display()
            );
            continue;
        }
        entries.push((stem, content));
    }

    if entries.is_empty() {
        return String::new();
    }

    // Sort for deterministic output
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut code = String::from("_KNOWLEDGE = {}\n");
    for (key, json_str) in &entries {
        let hex_encoded: String = json_str
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        code.push_str(&format!(
            "_KNOWLEDGE[\"{}\"] = __import__('json').loads(bytes.fromhex('{}').decode())\n",
            key, hex_encoded
        ));
    }
    log::info!(
        "Knowledge preamble: injected {} entries into _KNOWLEDGE",
        entries.len()
    );
    code
}

/// Compress tool result text in message history to save context tokens.
/// Strips verbose headers from execute_python results while keeping the actual output.
pub(crate) fn compress_tool_result(text: &str) -> String {
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
        if line.starts_with("[Purpose:")
            || line.starts_with("Exit code:")
            || line.starts_with("Execution time:")
        {
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
        if in_stderr
            && !line.trim().is_empty()
            && !line.contains("FutureWarning")
            && !line.contains("DeprecationWarning")
        {
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
pub(crate) fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> usize {
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
pub(crate) fn auto_capture_step_context(
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
        log::warn!(
            "[auto_capture] No content to capture for step {} in conversation {}",
            step_num,
            conversation_id
        );
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
        Ok(_) => log::info!(
            "[auto_capture] Saved step {} auto_context ({} chars) for conversation {}",
            step_num,
            combined.len(),
            conversation_id
        ),
        Err(e) => log::warn!(
            "[auto_capture] Failed to save step {} auto_context: {}",
            step_num,
            e
        ),
    }
}

/// Detect if a user message is a general daily question unrelated to the
/// current analysis workflow.
///
/// Returns `true` when the message looks like a general HR question or
/// casual chat, not a step confirmation, abort, or analysis feedback.
/// Used during `confirming`/`analyzing` mode to allow daily chat without
/// leaving the analysis workflow.
pub(crate) fn is_daily_question(text: &str) -> bool {
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
        "请问",
        "什么是",
        "怎么",
        "如何",
        "能不能",
        "帮我查",
        "帮忙查",
        "有没有",
        "是什么意思",
        "是多少",
        "怎么算",
        "怎么计算",
        "政策",
        "规定",
        "法规",
        "标准",
        "社保",
        "公积金",
        "个税",
        "年假",
        "病假",
        "产假",
        "劳动法",
        "劳动合同",
        "试用期",
        "离职",
        "辞退",
        "what is",
        "how to",
        "how do",
        "can you",
        "please explain",
        "tell me about",
    ];

    // Messages containing question patterns are likely daily questions
    if question_patterns.iter().any(|p| lower.contains(p)) {
        // But exclude if they also contain analysis-specific feedback terms
        let feedback_patterns = [
            "这一步",
            "上一步",
            "当前步骤",
            "分析结果",
            "重新分析",
            "调整",
            "修改",
            "补充",
            "岗位族",
            "职级",
            "公平性",
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
pub(crate) fn clear_analysis_notes(
    db: &AppStorage,
    conversation_id: &str,
    workspace_path: &std::path::Path,
) {
    let prefix = format!("note:{}:", conversation_id);
    match db.delete_memories_by_prefix(&prefix) {
        Ok(count) => {
            if count > 0 {
                log::info!(
                    "[cleanup] Cleared {} analysis notes for conversation {}",
                    count,
                    conversation_id
                );
            }
        }
        Err(e) => log::warn!(
            "[cleanup] Failed to clear analysis notes for {}: {}",
            conversation_id,
            e
        ),
    }

    // NOTE: Do NOT delete loaded file markers (loaded:{conv_id}:*) here.
    // These markers are needed by build_loaded_files_preamble() to inject
    // _df/_text variables into execute_python, regardless of conversation mode.
    // Deleting them causes NameError: '_df' is not defined when the user
    // continues using execute_python after exiting analysis mode.

    // Clean up DataFrame snapshot files (analysis/{conversation_id}/)
    let snap_dir = workspace_path.join("analysis").join(conversation_id);
    if snap_dir.exists() {
        match std::fs::remove_dir_all(&snap_dir) {
            Ok(_) => log::info!(
                "[cleanup] Removed snapshot directory {:?} for conversation {}",
                snap_dir,
                conversation_id
            ),
            Err(e) => log::warn!(
                "[cleanup] Failed to remove snapshot directory {:?}: {}",
                snap_dir,
                e
            ),
        }
    }

    // Store completion timestamp for cooldown detection
    let cooldown_key = format!("note:{}:analysis_completed_at", conversation_id);
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = db.set_memory(&cooldown_key, &now, Some("system")) {
        log::warn!(
            "[cleanup] Failed to store analysis cooldown timestamp: {}",
            e
        );
    }
}

/// Build a [`StepConfig`] from an active Skill and its current state.
///
/// Replaces `orchestrator::build_step_config()` — configuration now comes
/// from the Skill plugin rather than hardcoded step tables.
pub(crate) async fn build_config_from_skill(
    skill: &dyn crate::plugin::Skill,
    state: &SkillState,
    tool_registry: &ToolRegistry,
) -> StepConfig {
    let step_num = state
        .current_step
        .as_deref()
        .and_then(|s| s.strip_prefix("step"))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);

    let max_iter = skill.max_iterations(state);
    let token_budget = skill.token_budget(state);
    log::info!(
        "[build_config] skill={} step={:?} step_num={} max_iterations={} token_budget={}",
        skill.id(),
        state.current_step,
        step_num,
        max_iter,
        token_budget
    );

    // Always get all tool schemas for KV cache prefix stability.
    // Runtime enforcement is handled by allowed_tool_names.
    let tool_defs = tool_registry.get_schemas_filtered(&ToolFilter::All).await;

    // Build runtime guard set from skill's allowed_tool_names()
    let allowed_tool_names = skill
        .allowed_tool_names(state)
        .map(|names| names.into_iter().collect::<std::collections::HashSet<_>>());

    // Build step display names from the workflow definition
    let step_display_names = skill
        .workflow()
        .map(|wf| {
            wf.steps
                .iter()
                .enumerate()
                .map(|(i, s)| (i as u32, s.display_name.clone()))
                .collect()
        })
        .unwrap_or_default();

    // Get precompute and feedback config from skill
    let precompute = skill.on_step_enter(state);
    let feedback_config = skill.feedback_config(state);

    StepConfig {
        step: step_num,
        system_prompt: skill.system_prompt(state),
        tool_defs,
        max_iterations: max_iter,
        requires_confirmation: true,
        token_budget,
        step_display_names,
        allowed_tool_names,
        precompute,
        feedback_config,
        is_feedback: false,
    }
}

/// Guard that clears the gateway's active task for a specific conversation when dropped.
/// Ensures the agent state is cleaned up on success, error, or panic.
pub(crate) struct AgentGuard {
    pub(crate) gateway: Arc<LlmGateway>,
    db: Arc<AppStorage>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    app: AppHandle,
    conversation_id: String,
    run_id: RunId,
    cleared: bool,
}

impl AgentGuard {
    pub(crate) fn new(
        gateway: Arc<LlmGateway>,
        db: Arc<AppStorage>,
        session_mgr: Arc<crate::python::session::PythonSessionManager>,
        app: AppHandle,
        conversation_id: String,
        run_id: RunId,
    ) -> Self {
        Self {
            gateway,
            db,
            session_mgr,
            app,
            conversation_id,
            run_id,
            cleared: false,
        }
    }

    /// Remove the run.lock file with one retry on failure.
    fn remove_lock_with_retry(&self) {
        if let Err(e) = self.db.remove_active_task(&self.conversation_id) {
            log::error!(
                "[AgentGuard] Failed to remove run.lock for {} (attempt 1): {}, retrying...",
                self.conversation_id,
                e
            );
            // Single retry after a short delay (file system transient errors)
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Err(e2) = self.db.remove_active_task(&self.conversation_id) {
                log::error!(
                    "[AgentGuard] CRITICAL: Failed to remove run.lock for {} (attempt 2): {}. \
                     Will be cleaned up on next app startup.",
                    self.conversation_id,
                    e2
                );
            }
        }
    }

    /// Explicitly clear the active task and emit cleanup events.
    /// Always emits both `streaming:done` (so frontend clears streaming UI)
    /// and `agent:idle` (so frontend clears busy state).
    pub(crate) async fn clear(&mut self) {
        if !self.cleared {
            self.cleared = true;
            self.gateway.clear_task(&self.conversation_id);
            self.session_mgr.destroy_run(&self.run_id).await;
            self.remove_lock_with_retry();
            // streaming:done MUST fire so frontend clears isStreaming state.
            // finish_agent() also emits this, but if the agent panicked before
            // reaching finish_agent(), this is the only safety net.
            if let Err(e) = self.app.emit(
                "streaming:done",
                serde_json::json!({
                    "conversationId": self.conversation_id,
                    "messageId": "",
                    "runId": self.run_id.as_str(),
                }),
            ) {
                log::warn!(
                    "[AgentGuard] Failed to emit streaming:done for {}: {}",
                    self.conversation_id,
                    e
                );
            }
            if let Err(e) = self.app.emit(
                "agent:idle",
                serde_json::json!({
                    "conversationId": self.conversation_id,
                    "runId": self.run_id.as_str(),
                }),
            ) {
                log::warn!(
                    "[AgentGuard] Failed to emit agent:idle for {}: {}",
                    self.conversation_id,
                    e
                );
            }
            log::info!("[AgentGuard] Cleared active task for conversation {} and emitted streaming:done + agent:idle", self.conversation_id);
        }
    }
}

impl Drop for AgentGuard {
    fn drop(&mut self) {
        if !self.cleared {
            // Synchronous cleanup — gateway.clear_task() is now sync (std::sync::Mutex)
            self.remove_lock_with_retry();
            self.gateway.clear_task(&self.conversation_id);
            let session_mgr = self.session_mgr.clone();
            let run_id = self.run_id.clone();
            tauri::async_runtime::spawn(async move {
                session_mgr.destroy_run(&run_id).await;
            });

            // Event emission is also sync (Tauri emit is sync)
            let _ = self.app.emit(
                "streaming:done",
                serde_json::json!({
                    "conversationId": self.conversation_id,
                    "messageId": "",
                    "runId": self.run_id.as_str(),
                }),
            );
            let _ = self.app.emit(
                "agent:idle",
                serde_json::json!({
                    "conversationId": self.conversation_id,
                    "runId": self.run_id.as_str(),
                }),
            );
            log::info!("[AgentGuard] Drop fallback: cleared active task for conversation {} and emitted streaming:done + agent:idle", self.conversation_id);
        }
    }
}
