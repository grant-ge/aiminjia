// src-tauri/src/runtime/chat/metrics.rs
//!
//! Observability helpers extracted from the legacy agent_loop (Blocks 11, 26,
//! 30, 31 in `chat_runtime_impl.rs`).
//!
//! All functions are **fire-and-forget**: they only emit log lines and write
//! telemetry JSONL.  They never fail visibly — callers do not need to handle
//! errors from these.
//!
//! # Log tags (kept verbatim for grep-ability)
//!
//! | tag | function |
//! |-----|----------|
//! | `[CTX_METRICS]` | `log_context_baseline`, `log_step_done` |
//! | `[METRICS:tool]` | `log_tool_round` / `record_tool_round` |
//! | `[METRICS:step]` | `log_step_lifecycle` / `record_step_lifecycle` |

use std::path::Path;
use std::time::Duration;

use serde_json::Value as JsonValue;

use crate::runtime::chat::turn_config::{TurnConfig, TurnIterationState};

// ---------------------------------------------------------------------------
// Block 11 — baseline context snapshot before the iteration loop
// ---------------------------------------------------------------------------

/// Log a baseline snapshot of the LLM context before the iteration loop.
///
/// `file_context` and `notes_context` are strings whose lengths are logged;
/// `mode` should be `"analysis"` or `"daily"`; `step_label` is the
/// `Debug`-formatted step identifier (e.g. `"Some(1)"` or `"None"`).
///
/// Precision: mirrors the `[CTX_METRICS]` log line in Block 11 verbatim.
pub fn log_context_baseline(
    config: &TurnConfig,
    messages: &[JsonValue],
    file_context_len: usize,
    notes_context_len: usize,
    analysis_ctx_len: usize,
    step_label: &str,
) {
    let msg_total_chars: usize = messages
        .iter()
        .filter_map(|m| m.get("content").and_then(|v| v.as_str()))
        .map(|s| s.len())
        .sum();
    let tool_result_chars: usize = messages
        .iter()
        .filter(|m| m.get("tool_call_id").is_some() || m.get("toolCallId").is_some())
        .filter_map(|m| m.get("content").and_then(|v| v.as_str()))
        .map(|s| s.len())
        .sum();
    let tool_result_count = messages
        .iter()
        .filter(|m| m.get("tool_call_id").is_some() || m.get("toolCallId").is_some())
        .count();

    log::info!(
        "[CTX_METRICS] conversation={} mode={} step={} | \
         system_prompt={}chars (STATIC) | messages={} (tool_results={}) | \
         msg_total={}chars (tool_results={}chars) | \
         file_context={}chars | notes_context={}chars | \
         analysis_ctx={}chars",
        config.conversation_id,
        if config.is_analysis { "analysis" } else { "daily" },
        step_label,
        config.system_prompt.len(),
        messages.len(),
        tool_result_count,
        msg_total_chars,
        tool_result_chars,
        file_context_len,
        notes_context_len,
        analysis_ctx_len,
    );
}

// ---------------------------------------------------------------------------
// Block 26 — per-iteration tool-round metrics
// ---------------------------------------------------------------------------

/// Summary of one completed tool round, consumed by `log_tool_round` and
/// `record_tool_round`.
#[derive(Debug)]
pub struct ToolRoundMetrics<'a> {
    /// Names of every tool that was invoked in this round.
    pub tool_names: &'a [String],
    /// Number of tool calls that succeeded (not is_error).
    pub success_count: usize,
    /// Number of tool calls that failed or were blocked.
    pub error_count: usize,
    /// Wall-clock time for the whole round (driver start → all results back).
    pub elapsed: Duration,
    /// 1-based iteration index within the turn.
    pub iteration: usize,
    /// `Debug`-formatted step label (e.g. `"Some(1)"` or `"None"`).
    pub step_label: String,
}

/// Emit the `[METRICS:tool]` log line (Block 26 verbatim format).
pub fn log_tool_round(config: &TurnConfig, m: &ToolRoundMetrics<'_>) {
    let blocked = m.tool_names.len().saturating_sub(m.success_count + m.error_count);
    log::info!(
        "[METRICS:tool] conv={} step={} iter={} | \
         total={} success={} error={} blocked={} | \
         names={:?} | total_elapsed_ms={}",
        config.conversation_id,
        m.step_label,
        m.iteration,
        m.tool_names.len(),
        m.success_count,
        m.error_count,
        blocked,
        m.tool_names,
        m.elapsed.as_millis(),
    );
}

/// Write a `"tool"` telemetry entry (Block 26 verbatim fields).
///
/// `workspace_path` and `primary_model` are passed separately because they
/// live in settings/LLM config, not in `TurnConfig`.
pub fn record_tool_round(
    config: &TurnConfig,
    m: &ToolRoundMetrics<'_>,
    workspace_path: &Path,
    primary_model: &str,
) {
    let blocked = m.tool_names.len().saturating_sub(m.success_count + m.error_count);
    crate::telemetry::record(
        "tool",
        workspace_path,
        &[
            ("conv", config.conversation_id.as_str()),
            ("step", m.step_label.as_str()),
            ("iter", m.iteration.to_string().as_str()),
            ("total", m.tool_names.len().to_string().as_str()),
            ("success", m.success_count.to_string().as_str()),
            ("error", m.error_count.to_string().as_str()),
            ("blocked", blocked.to_string().as_str()),
            ("names", format!("{:?}", m.tool_names).as_str()),
            ("total_elapsed_ms", m.elapsed.as_millis().to_string().as_str()),
            ("model", primary_model),
        ],
    );
}

// ---------------------------------------------------------------------------
// Block 30 — end-of-step context summary
// ---------------------------------------------------------------------------

/// Emit the `[CTX_METRICS] STEP_DONE` log line (Block 30 verbatim format).
///
/// `step_label` is the `Debug`-formatted step identifier.
/// `output_len` is `full_content.len()`.
pub fn log_step_done(
    config: &TurnConfig,
    state: &TurnIterationState,
    step_label: &str,
    output_len: usize,
) {
    let final_msg_chars: usize = state
        .messages
        .iter()
        .filter_map(|m| m.get("content").and_then(|v| v.as_str()))
        .map(|s| s.len())
        .sum();
    let final_tool_chars: usize = state
        .messages
        .iter()
        .filter(|m| m.get("tool_call_id").is_some() || m.get("toolCallId").is_some())
        .filter_map(|m| m.get("content").and_then(|v| v.as_str()))
        .map(|s| s.len())
        .sum();
    let final_tool_count = state
        .messages
        .iter()
        .filter(|m| m.get("tool_call_id").is_some() || m.get("toolCallId").is_some())
        .count();

    log::info!(
        "[CTX_METRICS] STEP_DONE conv={} step={} iterations={} | \
         final_messages={} (tool_results={}) | \
         final_total={}chars (tool_results={}chars) | \
         output={}chars",
        config.conversation_id,
        step_label,
        state.iteration_count,
        state.messages.len(),
        final_tool_count,
        final_msg_chars,
        final_tool_chars,
        output_len,
    );
}

// ---------------------------------------------------------------------------
// Block 31 — step lifecycle metrics + telemetry
// ---------------------------------------------------------------------------

/// Terminal status of a turn step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Completed,
    Cancelled,
    MaxIterationsReached,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::MaxIterationsReached => "max_iter",
        }
    }
}

/// Inputs for the step lifecycle log + telemetry record (Block 31).
#[derive(Debug)]
pub struct StepLifecycleMetrics<'a> {
    /// Terminal status of this step.
    pub status: StepStatus,
    /// Wall-clock duration of the full step (from before the iteration loop).
    pub duration: Duration,
    /// Total LLM input tokens consumed across all iterations.
    pub tokens_in: u64,
    /// Total LLM output tokens produced across all iterations.
    pub tokens_out: u64,
    /// Short string describing what persistent notes were saved; e.g.
    /// `"checkpoint"`, `"summary"`, `"auto_capture"`, `"none"`, `"n/a"`.
    pub note_saved: &'a str,
    /// Length of the final text output (full_content.len()).
    pub output_len: usize,
    /// `Debug`-formatted step label (e.g. `"Some(1)"` or `"None"`).
    pub step_label: &'a str,
}

/// Emit the `[METRICS:step]` log line (Block 31 verbatim format).
///
/// `iteration_count` is the final 1-based iteration count (from
/// `TurnIterationState::iteration_count`).
pub fn log_step_lifecycle(
    config: &TurnConfig,
    m: &StepLifecycleMetrics<'_>,
    iteration_count: usize,
) {
    log::info!(
        "[METRICS:step] conv={} step={} status={} | \
         iterations={}/{} duration_ms={} | \
         tokens_in={} tokens_out={} | \
         note_saved={} | text_output={}chars",
        config.conversation_id,
        m.step_label,
        m.status.as_str(),
        iteration_count,
        config.max_iterations,
        m.duration.as_millis(),
        m.tokens_in,
        m.tokens_out,
        m.note_saved,
        m.output_len,
    );
}

/// Write a `"step"` telemetry entry (Block 31 verbatim fields).
///
/// `workspace_path` and `primary_model` are passed separately (not in TurnConfig).
pub fn record_step_lifecycle(
    config: &TurnConfig,
    m: &StepLifecycleMetrics<'_>,
    iteration_count: usize,
    workspace_path: &Path,
    primary_model: &str,
) {
    crate::telemetry::record(
        "step",
        workspace_path,
        &[
            ("conv", config.conversation_id.as_str()),
            ("step", m.step_label),
            ("status", m.status.as_str()),
            ("iterations", iteration_count.to_string().as_str()),
            ("duration_ms", m.duration.as_millis().to_string().as_str()),
            ("tokens_in", m.tokens_in.to_string().as_str()),
            ("tokens_out", m.tokens_out.to_string().as_str()),
            ("note_saved", m.note_saved),
            ("model", primary_model),
        ],
    );
}
