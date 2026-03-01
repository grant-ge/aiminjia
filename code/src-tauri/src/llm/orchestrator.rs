//! Analysis orchestrator — manages the 6-step compensation analysis workflow
//! (Step 0: direction confirmation + Steps 1–5: analysis pipeline).
//!
//! The orchestrator sits between `send_message` and `agent_loop`. It reads the
//! conversation's mode (`daily`/`confirming`/`analyzing`) from the DB, and
//! returns a deterministic `AnalysisAction` to drive the agent loop.
//!
//! **Key invariant**: The `conversation.mode` column is the single source of
//! truth for whether a conversation is in analysis flow. Step state in
//! `analysis_states` is subordinate — it tracks *which step* within analysis,
//! not *whether* we are analyzing.
#![allow(dead_code)]

use crate::llm::prompts;
use crate::llm::streaming::ChatMessage;
use crate::llm::streaming::ToolDefinition;
use crate::llm::tools;
use crate::storage::file_store::AppStorage;
use std::sync::Arc;

// ────────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────────

/// Configuration for a single analysis step.
#[derive(Debug, Clone)]
pub struct StepConfig {
    /// Step number (0–5).
    pub step: u32,
    /// Full system prompt (BASE + step-specific) from `prompts.rs`.
    pub system_prompt: String,
    /// Tool definitions available during this step.
    pub tool_defs: Vec<ToolDefinition>,
    /// Max tool-loop iterations within this step.
    pub max_iterations: usize,
    /// Whether the step requires user confirmation before advancing.
    pub requires_confirmation: bool,
}

/// Status of the current analysis step.
#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    /// Step is currently running (agent loop active or waiting for tool results).
    InProgress,
    /// Step completed, waiting for user confirmation to advance.
    Completed,
    /// Step was paused (e.g., due to crash recovery).
    Paused,
}

/// Snapshot of the analysis step state read from the database.
#[derive(Debug, Clone)]
pub struct StepState {
    pub step: u32,
    pub status: StepStatus,
}

/// What the orchestrator tells `send_message` to do next.
///
/// Each variant carries enough information for `chat.rs` to execute
/// the correct flow without additional orchestrator queries.
#[derive(Debug)]
pub enum AnalysisAction {
    /// Not in analysis mode — route to daily chat.
    /// Returned when `conversation_mode` is `"daily"` or unrecognized.
    DailyChat,
    /// User confirmed direction (Step 0 done) — start Step 1.
    /// `chat.rs` should set `conversation.mode = "analyzing"`.
    StartAnalysis(StepConfig),
    /// User confirmed the current step — advance to the next step.
    AdvanceStep(StepConfig),
    /// User gave feedback — re-run the current step with that feedback.
    RerunStep(StepConfig),
    /// Resume a paused step (crash recovery).
    ResumeStep(StepConfig),
    /// All steps complete and user confirmed — exit analysis mode.
    /// `chat.rs` should set `conversation.mode = "daily"` and call
    /// `db.finalize_analysis(conversation_id, "completed")`.
    FinishAnalysis,
    /// User explicitly aborted the analysis (e.g. "算了", "取消", "cancel").
    /// `chat.rs` should set `conversation.mode = "daily"` and call
    /// `db.finalize_analysis(conversation_id, "aborted")`.
    AbortAnalysis,
}

// ────────────────────────────────────────────────────────────────
// Detection (used only for daily → confirming transition)
// ────────────────────────────────────────────────────────────────

/// Check if a user message indicates they want a structured analysis.
///
/// Called by `send_message` only when `conversation.mode == "daily"`.
/// Returns `true` when:
/// - Explicit analysis request keywords are present, OR
/// - The conversation has uploaded files AND salary-related keywords appear.
pub fn detect_analysis_mode(messages: &[ChatMessage], has_files: bool) -> bool {
    let last_user = messages.iter().rev().find(|m| m.role == "user");
    let text = match last_user {
        Some(msg) => msg.content.to_lowercase(),
        None => return false,
    };

    // Explicit analysis request keywords
    let explicit_keywords = [
        "薪酬分析", "薪酬诊断", "公平性分析", "薪酬公平",
        "开始分析", "帮我分析", "做一次分析", "深度分析",
        "compensation analysis", "pay equity", "salary analysis",
        "fairness analysis",
    ];
    if explicit_keywords.iter().any(|kw| text.contains(kw)) {
        return true;
    }

    // File upload + salary-related keywords
    if has_files {
        let salary_keywords = [
            "工资", "薪酬", "薪资", "工资表", "薪酬表",
            "salary", "compensation", "payroll", "wage",
        ];
        if salary_keywords.iter().any(|kw| text.contains(kw)) {
            return true;
        }
    }

    false
}

// ────────────────────────────────────────────────────────────────
// State management
// ────────────────────────────────────────────────────────────────

/// Read the current analysis step and its status from the database.
///
/// Returns `None` if:
/// - No `analysis_states` row exists for this conversation, OR
/// - The analysis has a `final_status` (completed/aborted) — meaning
///   the previous analysis is finished and should not be continued.
pub fn get_step_state(db: &AppStorage, conversation_id: &str) -> Option<StepState> {
    let state = match db.get_analysis_state(conversation_id) {
        Ok(Some(state)) => state,
        _ => return None,
    };

    // If analysis is finalized, treat as "no active state".
    // This allows a conversation to start a new analysis after completing one.
    if state.get("finalStatus").and_then(|v| v.as_str()).is_some() {
        return None;
    }

    let step = state["currentStep"].as_i64().unwrap_or(0) as u32;

    // Read the status for the current step from the step_status JSON.
    // The JSON has the shape: {"stepN_status": "completed"|"in_progress"|"paused"}
    let step_status_json = &state["stepStatus"];
    let status_key = format!("step{}_status", step);
    let status_str = step_status_json
        .get(&status_key)
        .and_then(|v| v.as_str())
        .unwrap_or("in_progress");

    let status = match status_str {
        "completed" => StepStatus::Completed,
        "paused" => StepStatus::Paused,
        _ => StepStatus::InProgress,
    };

    Some(StepState { step, status })
}

/// Save step progress to the database.
pub fn advance_step(
    db: &Arc<AppStorage>,
    conversation_id: &str,
    step: u32,
    status: &str,
) -> Result<(), String> {
    let step_status = format!(r#"{{"step{}_status":"{}"}}"#, step, status);
    let state_data = "{}";
    db.upsert_analysis_state(conversation_id, step as i32, &step_status, state_data)
        .map_err(|e| e.to_string())
}

// ────────────────────────────────────────────────────────────────
// Step config builder
// ────────────────────────────────────────────────────────────────

/// Build the [`StepConfig`] for a given step number.
pub fn build_step_config(step: u32) -> StepConfig {
    StepConfig {
        step,
        system_prompt: prompts::get_system_prompt(Some(step)),
        tool_defs: tools::get_tool_definitions_for_step(step),
        max_iterations: match step {
            0 => 5,  // direction confirmation: peek file + save note + ask user
            1 => 15, // data cleaning
            2 => 15, // job normalization
            3 => 15, // level inference
            4 => 20, // 6-dimension fairness analysis (most complex)
            5 => 15, // report generation
            _ => 10,
        },
        requires_confirmation: true, // all steps need user confirmation
    }
}

// ────────────────────────────────────────────────────────────────
// Action routing
// ────────────────────────────────────────────────────────────────

/// Determine the next action based on the conversation mode and step state.
///
/// # Arguments
/// * `conversation_mode` — The `mode` column from the conversations table
///   (`"daily"`, `"confirming"`, or `"analyzing"`).
/// * `db` — Storage handle for reading analysis state.
/// * `conversation_id` — The conversation to check.
/// * `last_user_message` — The user's latest message text.
///
/// # Mode routing
/// * `"daily"` → `DailyChat` (caller should not normally call this in daily
///   mode — it handles detection separately — but we return gracefully).
/// * `"confirming"` → User responded to Step 0 → `StartAnalysis(step 1)`.
/// * `"analyzing"` → Read step state → route to advance/rerun/finish/resume.
pub fn next_action(
    conversation_mode: &str,
    db: &Arc<AppStorage>,
    conversation_id: &str,
    last_user_message: &str,
) -> AnalysisAction {
    match conversation_mode {
        "confirming" => {
            // User responded to Step 0 (direction confirmation).
            // Check if user wants to abort before proceeding.
            if is_abort(last_user_message) {
                AnalysisAction::AbortAnalysis
            } else {
                // Any other response is treated as confirmation.
                // The user's direction/focus was captured by save_analysis_note in Step 0.
                AnalysisAction::StartAnalysis(build_step_config(1))
            }
        }

        "analyzing" => {
            let step_state = get_step_state(db, conversation_id);
            match step_state {
                None => {
                    // No active step state (e.g., fresh entry or finalized analysis).
                    // This shouldn't happen if mode is "analyzing" correctly,
                    // but handle gracefully by starting Step 1.
                    log::warn!(
                        "Mode is 'analyzing' but no step state found for {}; starting Step 1",
                        conversation_id
                    );
                    AnalysisAction::StartAnalysis(build_step_config(1))
                }
                Some(state) => route_analysis_step(&state, last_user_message),
            }
        }

        // "daily" or any other value — not in analysis flow
        _ => AnalysisAction::DailyChat,
    }
}

/// Route within the analysis pipeline based on the current step state.
fn route_analysis_step(state: &StepState, last_user_message: &str) -> AnalysisAction {
    match (&state.status, state.step) {
        // Step 5 completed: user confirms → finish; abort → abort; otherwise re-run step 5
        (StepStatus::Completed, step) if step >= 5 => {
            if is_abort(last_user_message) {
                AnalysisAction::AbortAnalysis
            } else if is_confirmation(last_user_message) {
                AnalysisAction::FinishAnalysis
            } else {
                AnalysisAction::RerunStep(build_step_config(5))
            }
        }

        // Step N (0–4) completed: abort → abort; confirm → advance; otherwise re-run
        (StepStatus::Completed, step) => {
            if is_abort(last_user_message) {
                AnalysisAction::AbortAnalysis
            } else if is_confirmation(last_user_message) {
                AnalysisAction::AdvanceStep(build_step_config(step + 1))
            } else {
                AnalysisAction::RerunStep(build_step_config(step))
            }
        }

        // Paused step (crash recovery) → resume where we left off
        (StepStatus::Paused, step) => {
            AnalysisAction::ResumeStep(build_step_config(step))
        }

        // In-progress step (edge case: user sent message while step still running)
        (StepStatus::InProgress, step) => {
            if is_abort(last_user_message) {
                AnalysisAction::AbortAnalysis
            } else {
                log::warn!(
                    "User sent message while step {} is still in_progress; re-running step",
                    step
                );
                AnalysisAction::RerunStep(build_step_config(step))
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────
// Confirmation detection
// ────────────────────────────────────────────────────────────────

/// Check if the user's message is a confirmation to proceed.
///
/// Uses exact matching against a curated list of confirmation phrases.
/// Hardened with:
/// - **Trailing punctuation stripping**: handles "ok！", "好的。", "确认!!"
/// - **20-character length cutoff**: feedback sentences are longer
/// - **Case-insensitive matching**: "OK" = "ok" = "Ok"
fn is_confirmation(text: &str) -> bool {
    let text = text.trim();

    // Length cutoff: messages beyond 20 characters are almost certainly
    // feedback or follow-up questions, not simple confirmations.
    // All confirmation phrases in the list are well under 20 chars.
    if text.chars().count() > 20 {
        return false;
    }

    // Strip trailing punctuation (Chinese and English) and normalize case
    let stripped = text
        .trim_end_matches(|c: char| {
            matches!(
                c,
                '.' | '!' | '?' | '。' | '！' | '？' | '~' | '～' | '，' | ',' | '、'
            )
        })
        .to_lowercase();

    // Exact-match confirmation phrases (must be the entire message after stripping)
    let exact_phrases = [
        // Chinese single-word
        "确认", "继续", "好的", "可以", "没问题", "好", "行", "对",
        "是的", "确定", "通过", "下一步", "继续吧", "没有问题", "同意", "认可",
        // Chinese compound
        "好的好的", "可以可以", "没问题的", "好的继续",
        "可以的", "好的吧", "行吧",
        // Chinese with internal punctuation
        "好的，继续", "可以，下一步", "可以，继续",
        // English
        "ok", "okay", "yes", "proceed", "continue", "confirm", "next",
        "lgtm", "looks good", "ok ok",
        // Start analysis (Step 0 → Step 1 transition)
        "开始", "开始分析", "开始吧", "start",
    ];

    exact_phrases.iter().any(|p| stripped == *p)
}

/// Check if the user's message indicates they want to abort analysis.
///
/// Uses exact matching against abort/cancel phrases, similar to
/// `is_confirmation()`. Hardened with the same punctuation stripping
/// and length cutoff.
fn is_abort(text: &str) -> bool {
    let text = text.trim();

    // Length cutoff: abort phrases are short
    if text.chars().count() > 20 {
        return false;
    }

    // Strip trailing punctuation (Chinese and English) and normalize case
    let stripped = text
        .trim_end_matches(|c: char| {
            matches!(
                c,
                '.' | '!' | '?' | '。' | '！' | '？' | '~' | '～' | '，' | ',' | '、'
            )
        })
        .to_lowercase();

    let abort_phrases = [
        // Chinese
        "算了", "不分析了", "取消", "取消分析", "退出", "退出分析",
        "停止", "停止分析", "不做了", "不用了", "算了吧", "放弃",
        "不做分析", "不要分析", "别分析", "别分析了", "不用分析",
        "不用分析了", "不需要分析", "先不分析", "先不做分析",
        // English
        "cancel", "abort", "stop", "exit", "quit", "nevermind",
        "no", "no thanks", "don't analyze", "skip", "skip analysis",
    ];

    abort_phrases.iter().any(|p| stripped == *p)
}

// ────────────────────────────────────────────────────────────────
// Message building
// ────────────────────────────────────────────────────────────────

/// Build the message array with the system prompt prepended.
///
/// The system prompt is inserted as the first message with role "system".
/// This is used by the agent loop to inject step-specific guidance.
pub fn build_step_messages(
    base_messages: &[ChatMessage],
    system_prompt: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(base_messages.len() + 1);
    messages.push(ChatMessage::text("system", system_prompt));
    messages.extend_from_slice(base_messages);
    messages
}

// ────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_messages(texts: &[(&str, &str)]) -> Vec<ChatMessage> {
        texts
            .iter()
            .map(|(role, content)| ChatMessage::text(role, *content))
            .collect()
    }

    fn test_db() -> (Arc<AppStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(AppStorage::new(dir.path()).unwrap());
        db.create_conversation("test_conv", "Test").unwrap();
        (db, dir)
    }

    // ── detect_analysis_mode ──

    #[test]
    fn test_detect_explicit_analysis_request() {
        let msgs = make_messages(&[("user", "请帮我做一次薪酬公平性分析")]);
        assert!(detect_analysis_mode(&msgs, false));
    }

    #[test]
    fn test_detect_explicit_english() {
        let msgs = make_messages(&[("user", "Run a compensation analysis on this data")]);
        assert!(detect_analysis_mode(&msgs, false));
    }

    #[test]
    fn test_detect_file_with_salary_keyword() {
        let msgs = make_messages(&[("user", "这是我们的工资表，帮我看看")]);
        assert!(detect_analysis_mode(&msgs, true));
    }

    #[test]
    fn test_no_detect_general_chat() {
        let msgs = make_messages(&[("user", "你好，请问社保基数是多少？")]);
        assert!(!detect_analysis_mode(&msgs, false));
    }

    #[test]
    fn test_no_detect_file_without_salary() {
        let msgs = make_messages(&[("user", "这是我们的组织架构图")]);
        assert!(!detect_analysis_mode(&msgs, true));
    }

    #[test]
    fn test_no_detect_empty_messages() {
        let msgs: Vec<ChatMessage> = vec![];
        assert!(!detect_analysis_mode(&msgs, false));
    }

    // ── is_confirmation (basic) ──

    #[test]
    fn test_confirmation_chinese() {
        assert!(is_confirmation("确认"));
        assert!(is_confirmation("好的，继续"));
        assert!(is_confirmation("没问题"));
        assert!(is_confirmation("可以，下一步"));
    }

    #[test]
    fn test_confirmation_english() {
        assert!(is_confirmation("ok"));
        assert!(is_confirmation("Yes"));
        assert!(is_confirmation("LGTM"));
        assert!(is_confirmation("continue"));
    }

    #[test]
    fn test_confirmation_start_analysis() {
        assert!(is_confirmation("开始"));
        assert!(is_confirmation("开始分析"));
        assert!(is_confirmation("开始吧"));
        assert!(is_confirmation("start"));
    }

    // ── is_confirmation (punctuation stripping) ──

    #[test]
    fn test_confirmation_with_trailing_punctuation() {
        assert!(is_confirmation("ok！"));
        assert!(is_confirmation("好的。"));
        assert!(is_confirmation("确认!"));
        assert!(is_confirmation("可以!"));
        assert!(is_confirmation("好的～"));
        assert!(is_confirmation("是的？"));
    }

    #[test]
    fn test_confirmation_with_multiple_trailing_punctuation() {
        assert!(is_confirmation("确认!!"));
        assert!(is_confirmation("ok!!!"));
        assert!(is_confirmation("好的。。"));
    }

    // ── is_confirmation (rejection) ──

    #[test]
    fn test_not_confirmation_modification() {
        assert!(!is_confirmation(
            "把品质合并到生产里，重新调整岗位族方案，我觉得 6 个族太多了，减少到 5 个"
        ));
    }

    #[test]
    fn test_not_confirmation_question() {
        assert!(!is_confirmation(
            "为什么张三被标记为偏低？他的绩效一直很好啊"
        ));
    }

    #[test]
    fn test_not_confirmation_continue_with_context() {
        assert!(!is_confirmation("继续描述问题"));
        assert!(!is_confirmation("继续分析一下原因"));
        assert!(!is_confirmation("确认一下这个数据对不对"));
    }

    #[test]
    fn test_not_confirmation_long_text() {
        // Over 20 chars should be rejected regardless of content
        assert!(!is_confirmation("好的好的好的好的好的好的好的好的好的好的"));
    }

    // ── build_step_messages ──

    #[test]
    fn test_build_step_messages_prepends_system() {
        let base = make_messages(&[
            ("user", "Hello"),
            ("assistant", "Hi there"),
        ]);
        let result = build_step_messages(&base, "You are a helpful assistant.");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content, "You are a helpful assistant.");
        assert_eq!(result[1].role, "user");
        assert_eq!(result[2].role, "assistant");
    }

    // ── build_step_config ──

    #[test]
    fn test_step_config_has_correct_prompt() {
        let config = build_step_config(1);
        assert_eq!(config.step, 1);
        assert!(config.system_prompt.contains("Step 1"));
        assert!(config.requires_confirmation);
    }

    #[test]
    fn test_step_config_tools_not_empty() {
        for step in 0..=5 {
            let config = build_step_config(step);
            assert!(
                !config.tool_defs.is_empty(),
                "Step {} should have tool definitions",
                step
            );
        }
    }

    #[test]
    fn test_step4_has_most_iterations() {
        let config = build_step_config(4);
        assert_eq!(config.max_iterations, 20);
    }

    #[test]
    fn test_step0_config() {
        let config = build_step_config(0);
        assert_eq!(config.step, 0);
        assert_eq!(config.max_iterations, 5);
        assert!(config.requires_confirmation);
        assert!(config.system_prompt.contains("分析方向确认"));
    }

    #[test]
    fn test_step0_tools() {
        let config = build_step_config(0);
        let tool_names: Vec<&str> = config.tool_defs.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"analyze_file"));
        assert!(tool_names.contains(&"save_analysis_note"));
        assert_eq!(tool_names.len(), 2);
    }

    // ── get_step_state ──

    #[test]
    fn test_get_step_state_no_record() {
        let (db, _dir) = test_db();
        let state = get_step_state(&db, "test_conv");
        assert!(state.is_none());
    }

    #[test]
    fn test_get_step_state_in_progress() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 2, "in_progress").unwrap();
        let state = get_step_state(&db, "test_conv").unwrap();
        assert_eq!(state.step, 2);
        assert_eq!(state.status, StepStatus::InProgress);
    }

    #[test]
    fn test_get_step_state_completed() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 3, "completed").unwrap();
        let state = get_step_state(&db, "test_conv").unwrap();
        assert_eq!(state.step, 3);
        assert_eq!(state.status, StepStatus::Completed);
    }

    #[test]
    fn test_get_step_state_paused() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 1, "paused").unwrap();
        let state = get_step_state(&db, "test_conv").unwrap();
        assert_eq!(state.step, 1);
        assert_eq!(state.status, StepStatus::Paused);
    }

    #[test]
    fn test_get_step_state_finalized_returns_none() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 5, "completed").unwrap();
        db.finalize_analysis("test_conv", "completed").unwrap();
        // After finalization, get_step_state should return None
        // so the conversation can start a new analysis
        let state = get_step_state(&db, "test_conv");
        assert!(state.is_none());
    }

    // ── route_analysis_step (unit tests without DB) ──

    #[test]
    fn test_route_completed_step_with_confirmation() {
        let state = StepState { step: 2, status: StepStatus::Completed };
        let action = route_analysis_step(&state, "确认");
        assert!(matches!(action, AnalysisAction::AdvanceStep(config) if config.step == 3));
    }

    #[test]
    fn test_route_completed_step_with_feedback() {
        let state = StepState { step: 2, status: StepStatus::Completed };
        let action = route_analysis_step(&state, "把品质合并到生产里");
        assert!(matches!(action, AnalysisAction::RerunStep(config) if config.step == 2));
    }

    #[test]
    fn test_route_paused_step() {
        let state = StepState { step: 3, status: StepStatus::Paused };
        let action = route_analysis_step(&state, "anything");
        assert!(matches!(action, AnalysisAction::ResumeStep(config) if config.step == 3));
    }

    #[test]
    fn test_route_step5_completed_confirm() {
        let state = StepState { step: 5, status: StepStatus::Completed };
        let action = route_analysis_step(&state, "确认");
        assert!(matches!(action, AnalysisAction::FinishAnalysis));
    }

    #[test]
    fn test_route_step5_completed_feedback() {
        let state = StepState { step: 5, status: StepStatus::Completed };
        let action = route_analysis_step(&state, "再加一个图表");
        assert!(matches!(action, AnalysisAction::RerunStep(config) if config.step == 5));
    }

    #[test]
    fn test_route_in_progress_step() {
        let state = StepState { step: 1, status: StepStatus::InProgress };
        let action = route_analysis_step(&state, "hello");
        assert!(matches!(action, AnalysisAction::RerunStep(config) if config.step == 1));
    }

    // ── next_action routing (with DB) ──

    #[test]
    fn test_next_action_daily_returns_daily_chat() {
        let (db, _dir) = test_db();
        let action = next_action("daily", &db, "test_conv", "hello");
        assert!(matches!(action, AnalysisAction::DailyChat));
    }

    #[test]
    fn test_next_action_confirming_returns_start_analysis() {
        let (db, _dir) = test_db();
        let action = next_action("confirming", &db, "test_conv", "开始");
        assert!(matches!(action, AnalysisAction::StartAnalysis(config) if config.step == 1));
    }

    #[test]
    fn test_next_action_confirming_any_response_starts_analysis() {
        let (db, _dir) = test_db();
        // Even non-confirmation text should advance from confirming → Step 1
        let action = next_action("confirming", &db, "test_conv", "重点看技术部门");
        assert!(matches!(action, AnalysisAction::StartAnalysis(config) if config.step == 1));
    }

    #[test]
    fn test_next_action_analyzing_no_state_starts_step1() {
        let (db, _dir) = test_db();
        // No analysis state in DB — graceful fallback
        let action = next_action("analyzing", &db, "test_conv", "hello");
        assert!(matches!(action, AnalysisAction::StartAnalysis(config) if config.step == 1));
    }

    #[test]
    fn test_next_action_analyzing_step2_completed_confirm() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 2, "completed").unwrap();
        let action = next_action("analyzing", &db, "test_conv", "确认");
        assert!(matches!(action, AnalysisAction::AdvanceStep(config) if config.step == 3));
    }

    #[test]
    fn test_next_action_analyzing_step2_completed_feedback() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 2, "completed").unwrap();
        let action = next_action("analyzing", &db, "test_conv", "请重新分组");
        assert!(matches!(action, AnalysisAction::RerunStep(config) if config.step == 2));
    }

    #[test]
    fn test_next_action_analyzing_step5_confirm_finishes() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 5, "completed").unwrap();
        let action = next_action("analyzing", &db, "test_conv", "ok");
        assert!(matches!(action, AnalysisAction::FinishAnalysis));
    }

    #[test]
    fn test_next_action_analyzing_paused_step_resumes() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 3, "paused").unwrap();
        let action = next_action("analyzing", &db, "test_conv", "hello");
        assert!(matches!(action, AnalysisAction::ResumeStep(config) if config.step == 3));
    }

    #[test]
    fn test_next_action_analyzing_finalized_starts_fresh() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 5, "completed").unwrap();
        db.finalize_analysis("test_conv", "completed").unwrap();
        // After finalization, mode should be "daily" in practice,
        // but if somehow still "analyzing", graceful fallback to Step 1
        let action = next_action("analyzing", &db, "test_conv", "hello");
        assert!(matches!(action, AnalysisAction::StartAnalysis(config) if config.step == 1));
    }

    // ── is_abort (basic) ──

    #[test]
    fn test_abort_chinese() {
        assert!(is_abort("算了"));
        assert!(is_abort("取消"));
        assert!(is_abort("退出分析"));
        assert!(is_abort("不分析了"));
        assert!(is_abort("停止"));
        assert!(is_abort("不做了"));
        assert!(is_abort("算了吧"));
        assert!(is_abort("放弃"));
        // Additional variations
        assert!(is_abort("不做分析"));
        assert!(is_abort("不要分析"));
        assert!(is_abort("别分析"));
        assert!(is_abort("别分析了"));
        assert!(is_abort("不用分析"));
        assert!(is_abort("不用分析了"));
        assert!(is_abort("不需要分析"));
        assert!(is_abort("先不分析"));
        assert!(is_abort("先不做分析"));
    }

    #[test]
    fn test_abort_english() {
        assert!(is_abort("cancel"));
        assert!(is_abort("Cancel"));
        assert!(is_abort("ABORT"));
        assert!(is_abort("stop"));
        assert!(is_abort("quit"));
        assert!(is_abort("nevermind"));
        assert!(is_abort("no"));
        assert!(is_abort("no thanks"));
        assert!(is_abort("don't analyze"));
        assert!(is_abort("skip"));
        assert!(is_abort("skip analysis"));
    }

    #[test]
    fn test_abort_with_punctuation() {
        assert!(is_abort("算了！"));
        assert!(is_abort("取消。"));
        assert!(is_abort("cancel!"));
        assert!(is_abort("stop!!"));
    }

    #[test]
    fn test_not_abort_long_text() {
        assert!(!is_abort("算了，我觉得这个分析还是需要的"));
        assert!(!is_abort("取消原来的方案，换一种分析方式"));
    }

    #[test]
    fn test_not_abort_confirmation() {
        // Confirmations should NOT be treated as abort
        assert!(!is_abort("确认"));
        assert!(!is_abort("ok"));
        assert!(!is_abort("继续"));
    }

    // ── abort routing ──

    #[test]
    fn test_route_completed_step_with_abort() {
        let state = StepState { step: 2, status: StepStatus::Completed };
        let action = route_analysis_step(&state, "算了");
        assert!(matches!(action, AnalysisAction::AbortAnalysis));
    }

    #[test]
    fn test_route_step5_completed_abort() {
        let state = StepState { step: 5, status: StepStatus::Completed };
        let action = route_analysis_step(&state, "取消");
        assert!(matches!(action, AnalysisAction::AbortAnalysis));
    }

    #[test]
    fn test_route_in_progress_step_with_abort() {
        let state = StepState { step: 3, status: StepStatus::InProgress };
        let action = route_analysis_step(&state, "退出分析");
        assert!(matches!(action, AnalysisAction::AbortAnalysis));
    }

    #[test]
    fn test_next_action_confirming_abort() {
        let (db, _dir) = test_db();
        let action = next_action("confirming", &db, "test_conv", "算了");
        assert!(matches!(action, AnalysisAction::AbortAnalysis));
    }

    #[test]
    fn test_next_action_analyzing_abort() {
        let (db, _dir) = test_db();
        advance_step(&db, "test_conv", 2, "completed").unwrap();
        let action = next_action("analyzing", &db, "test_conv", "cancel");
        assert!(matches!(action, AnalysisAction::AbortAnalysis));
    }
}
