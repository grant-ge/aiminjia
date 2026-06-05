//! P1: Intra-step context decay — reduce older tool outputs to save LLM context.
//!
//! During long tool-using turns, the agent may iterate 10+ times, producing tool results
//! (bash stdout, web_search results, etc.) that accumulate in the message history. Older results
//! are less relevant to the current iteration, so we apply progressive truncation:
//!
//! - Most recent iteration: full content preserved
//! - Previous iteration: truncated to `RECENT_LIMIT` chars
//! - Older iterations: truncated to `OLD_LIMIT` chars
//!
//! **Non-destructive**: `apply_decay()` returns a new `Vec<ChatMessage>`; the
//! original messages are never mutated. This ensures `checkpoint_extract()` and
//! `auto_capture_step_context()` still see full data.

use crate::llm::streaming::ChatMessage;

pub const CONTEXT_WINDOW_CLAUDE: usize = 200_000;
pub const CONTEXT_WINDOW_DEEPSEEK: usize = 128_000;
pub const CONTEXT_WINDOW_DEFAULT: usize = 100_000;
pub const CONTEXT_OVERFLOW_THRESHOLD: f64 = 0.8;

/// Conservative context window when no model info is available.
pub const CONSERVATIVE_CONTEXT_WINDOW: usize = 64_000;

/// Buffer tokens subtracted from context window for auto-compact threshold calculation.
pub const AUTOCOMPACT_BUFFER_TOKENS: usize = 13_000;

/// Max output tokens reserved for compact summary generation.
pub const MAX_OUTPUT_TOKENS_FOR_SUMMARY: usize = 20_000;

pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            let mut chars = message.content.len();
            if let Some(tool_calls) = &message.tool_calls {
                chars += serde_json::to_string(tool_calls)
                    .map(|s| s.len())
                    .unwrap_or(0);
            }
            chars
        })
        .sum::<usize>()
        / 4
}

pub fn estimate_context_tokens(system_prompt: &str, messages: &[ChatMessage]) -> usize {
    (system_prompt.len() + estimate_tokens(messages) * 4) / 4
}

pub fn estimate_tokens_from_json(messages: &[serde_json::Value]) -> usize {
    messages
        .iter()
        .map(|value| value.to_string().len())
        .sum::<usize>()
        / 4
}

pub fn context_window_for_provider(provider: &str) -> usize {
    match provider {
        "claude" => CONTEXT_WINDOW_CLAUDE,
        "deepseek-v3" | "deepseek-r1" => CONTEXT_WINDOW_DEEPSEEK,
        _ => CONTEXT_WINDOW_DEFAULT,
    }
}

/// Resolve the context window size for a specific model name.
///
/// Uses substring matching on the model identifier to determine the window.
/// Falls back to CONSERVATIVE_CONTEXT_WINDOW for unknown models.
pub fn context_window_for_model(model: &str) -> usize {
    if model.is_empty() {
        return CONSERVATIVE_CONTEXT_WINDOW;
    }
    let lower = model.to_lowercase();
    if lower.contains("claude") {
        return CONTEXT_WINDOW_CLAUDE;
    }
    if lower.contains("deepseek") {
        return CONTEXT_WINDOW_DEEPSEEK;
    }
    if lower.contains("gpt") {
        return 128_000;
    }
    CONSERVATIVE_CONTEXT_WINDOW
}

/// Resolve the context window for the current conversation.
///
/// Priority:
/// 1. `settings_override` — manual override from AppSettings.context_window
/// 2. `cloud_model` — model name returned by the gateway /v1/models, matched via context_window_for_model()
/// 3. CONSERVATIVE_CONTEXT_WINDOW (64K) — fallback when no info is available
pub fn resolve_context_window(
    settings_override: Option<usize>,
    cloud_model: Option<&str>,
) -> usize {
    settings_override
        .or_else(|| cloud_model.map(|m| context_window_for_model(m)))
        .unwrap_or(CONSERVATIVE_CONTEXT_WINDOW)
}

/// Compute the effective auto-compact threshold in **chars**.
///
/// Formula: (context_window - MAX_OUTPUT_TOKENS_FOR_SUMMARY - AUTOCOMPACT_BUFFER_TOKENS) * 4
///
/// The *4 converts token count to char estimate (consistent with the chars/4 convention).
pub fn effective_auto_compact_threshold(custom_window: Option<usize>) -> usize {
    let raw_window = resolve_context_window(custom_window, None);
    let effective = raw_window.saturating_sub(MAX_OUTPUT_TOKENS_FOR_SUMMARY);
    let threshold_tokens = effective.saturating_sub(AUTOCOMPACT_BUFFER_TOKENS);
    threshold_tokens.saturating_mul(4)
}

/// Max chars for tool results in the second-most-recent iteration.
const RECENT_LIMIT: usize = 2000;

/// Max chars for tool results in older iterations.
const OLD_LIMIT: usize = 500;

/// Configurable decay policy.
#[derive(Debug, Clone)]
pub struct DecayPolicy {
    /// Chars limit for the previous iteration's tool results.
    pub recent_limit: usize,
    /// Chars limit for older iterations' tool results.
    pub old_limit: usize,
}

impl Default for DecayPolicy {
    fn default() -> Self {
        Self {
            recent_limit: RECENT_LIMIT,
            old_limit: OLD_LIMIT,
        }
    }
}

/// An "iteration" is defined as one `assistant_with_tool_calls` message
/// followed by its subsequent tool result messages (until the next assistant
/// or user message).
struct Iteration {
    /// Start index in the messages vec (the assistant message).
    start: usize,
    /// End index (exclusive) — one past the last tool result in this iteration.
    end: usize,
}

/// Identify iteration boundaries in the message list.
///
/// An iteration starts with an assistant message that has `tool_calls` and
/// continues through consecutive tool result messages. Phantom iterations
/// (assistant with tool_calls but no following tool results) are filtered
/// out to avoid skewing decay age calculations.
fn find_iterations(messages: &[ChatMessage]) -> Vec<Iteration> {
    let mut iterations = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let msg = &messages[i];
        if msg.role == "assistant" && msg.tool_calls.as_ref().map_or(false, |tc| !tc.is_empty()) {
            let start = i;
            i += 1;
            // Consume following tool result messages
            while i < messages.len() && messages[i].tool_call_id.is_some() {
                i += 1;
            }
            // Only count iterations that have at least one tool result
            // (phantom iterations with zero results would skew decay age)
            if i > start + 1 {
                iterations.push(Iteration { start, end: i });
            }
        } else {
            i += 1;
        }
    }

    iterations
}

/// Apply progressive decay to tool results in the message history.
///
/// Returns a **new** Vec with truncated tool results for older iterations.
/// The original `messages` slice is not modified.
///
pub fn apply_decay(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return messages.to_vec();
    }

    apply_decay_with_policy(messages, &DecayPolicy::default())
}

/// Apply decay with a custom policy (useful for testing).
pub fn apply_decay_with_policy(messages: &[ChatMessage], policy: &DecayPolicy) -> Vec<ChatMessage> {
    let iterations = find_iterations(messages);

    if iterations.len() <= 1 {
        // 0 or 1 iterations — nothing to decay
        return messages.to_vec();
    }

    // Age assignment: last iteration = 0 (most recent), second-to-last = 1, etc.
    let num_iterations = iterations.len();

    // Build a set of message indices that need truncation, with their limit
    let mut truncation_map: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (iter_idx, iteration) in iterations.iter().enumerate() {
        let age = num_iterations - 1 - iter_idx; // 0 = most recent
        let limit = match age {
            0 => usize::MAX, // most recent — full content
            1 => policy.recent_limit,
            _ => policy.old_limit,
        };
        if limit == usize::MAX {
            continue;
        }
        // Only truncate tool result messages (not the assistant message itself)
        for msg_idx in (iteration.start + 1)..iteration.end {
            truncation_map.insert(msg_idx, limit);
        }
    }

    // Build the output vec
    messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            if let Some(&limit) = truncation_map.get(&idx) {
                if msg.content.len() > limit {
                    let mut truncated = msg.clone();
                    let end = truncate_at_char_boundary(&msg.content, limit);
                    truncated.content = format!(
                        "{}...\n[decayed: {} → {} chars]",
                        &msg.content[..end],
                        msg.content.len(),
                        end
                    );
                    truncated
                } else {
                    msg.clone()
                }
            } else {
                msg.clone()
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::streaming::ToolCall;

    fn make_assistant_with_tools(content: &str) -> ChatMessage {
        ChatMessage::assistant_with_tool_calls(
            content.to_string(),
            vec![ToolCall {
                id: "tc_1".to_string(),
                name: "Bash".to_string(),
                arguments: serde_json::json!({}),
            }],
            None,
            None,
        )
    }

    fn make_tool_result(content: &str) -> ChatMessage {
        ChatMessage::tool_result("tc_1", "Bash", content.to_string())
    }

    fn make_user(content: &str) -> ChatMessage {
        ChatMessage::text("user", content)
    }

    #[test]
    fn decay_noop_for_empty_or_single_iteration_history() {
        let messages = vec![
            make_user("hello"),
            make_assistant_with_tools("running"),
            make_tool_result(&"x".repeat(10000)),
        ];
        let result = apply_decay(&messages);
        assert_eq!(result.len(), messages.len());
        assert_eq!(result[2].content, messages[2].content);
    }

    #[test]
    fn decay_noop_for_single_iteration() {
        let messages = vec![
            make_user("analyze"),
            make_assistant_with_tools("running"),
            make_tool_result(&"x".repeat(10000)),
        ];
        let result = apply_decay(&messages);
        // Single iteration → no decay
        assert_eq!(result[2].content.len(), messages[2].content.len());
    }

    #[test]
    fn decay_truncates_older_iterations() {
        let big_content = "x".repeat(5000);
        let messages = vec![
            make_user("analyze"),
            // Iteration 0 (oldest)
            make_assistant_with_tools("iter0"),
            make_tool_result(&big_content),
            // Iteration 1
            make_assistant_with_tools("iter1"),
            make_tool_result(&big_content),
            // Iteration 2 (most recent)
            make_assistant_with_tools("iter2"),
            make_tool_result(&big_content),
        ];

        let result = apply_decay(&messages);

        // Iteration 2 (most recent, idx=6) — full
        assert_eq!(result[6].content.len(), big_content.len());

        // Iteration 1 (idx=4) — truncated to RECENT_LIMIT
        assert!(result[4].content.len() < big_content.len());
        assert!(result[4].content.contains("[decayed:"));

        // Iteration 0 (idx=2) — truncated to OLD_LIMIT
        assert!(result[2].content.len() < result[4].content.len());
        assert!(result[2].content.contains("[decayed:"));
    }

    #[test]
    fn decay_preserves_original_messages() {
        let big_content = "x".repeat(5000);
        let messages = vec![
            make_user("analyze"),
            make_assistant_with_tools("iter0"),
            make_tool_result(&big_content),
            make_assistant_with_tools("iter1"),
            make_tool_result(&big_content),
        ];

        let _ = apply_decay(&messages);

        // Original messages should be unchanged
        assert_eq!(messages[2].content.len(), big_content.len());
        assert_eq!(messages[4].content.len(), big_content.len());
    }

    #[test]
    fn decay_does_not_truncate_assistant_messages() {
        let big = "x".repeat(5000);
        let messages = vec![
            make_user("analyze"),
            make_assistant_with_tools(&big),
            make_tool_result("short"),
            make_assistant_with_tools(&big),
            make_tool_result("short"),
        ];

        let result = apply_decay(&messages);
        // Assistant messages (indices 1, 3) should not be truncated
        assert_eq!(result[1].content.len(), big.len());
        assert_eq!(result[3].content.len(), big.len());
    }

    #[test]
    fn find_iterations_detects_correctly() {
        let messages = vec![
            make_user("hi"),
            make_assistant_with_tools("a"),
            make_tool_result("r1"),
            make_tool_result("r2"),
            make_assistant_with_tools("b"),
            make_tool_result("r3"),
        ];

        let iters = find_iterations(&messages);
        assert_eq!(iters.len(), 2);
        assert_eq!(iters[0].start, 1);
        assert_eq!(iters[0].end, 4);
        assert_eq!(iters[1].start, 4);
        assert_eq!(iters[1].end, 6);
    }

    #[test]
    fn find_iterations_skips_phantom_iterations() {
        // A "phantom" iteration is an assistant with tool_calls but no following
        // tool results. This can happen if tool execution was cancelled or blocked.
        let messages = vec![
            make_user("hi"),
            make_assistant_with_tools("phantom"), // no tool results follow
            make_assistant_with_tools("real"),
            make_tool_result("r1"),
        ];

        let iters = find_iterations(&messages);
        assert_eq!(iters.len(), 1); // phantom should be filtered out
        assert_eq!(iters[0].start, 2);
        assert_eq!(iters[0].end, 4);
    }
}

#[cfg(test)]
mod resolve_context_window_tests {
    use super::*;

    #[test]
    fn resolves_claude_model() {
        let w = resolve_context_window(None, Some("claude-sonnet-4-6"));
        assert_eq!(w, 200_000);
    }

    #[test]
    fn resolves_deepseek_model() {
        let w = resolve_context_window(None, Some("deepseek-v3-0324"));
        assert_eq!(w, 128_000);
    }

    #[test]
    fn resolves_gpt_model() {
        let w = resolve_context_window(None, Some("gpt-4o"));
        assert_eq!(w, 128_000);
    }

    #[test]
    fn settings_override_wins() {
        let w = resolve_context_window(Some(300_000), Some("claude-sonnet-4-6"));
        assert_eq!(w, 300_000);
    }

    #[test]
    fn falls_back_to_conservative() {
        let w = resolve_context_window(None, None);
        assert_eq!(w, 64_000);
    }

    #[test]
    fn empty_model_falls_back() {
        let w = resolve_context_window(None, Some(""));
        assert_eq!(w, 64_000);
    }

    #[test]
    fn unknown_model_falls_back() {
        let w = resolve_context_window(None, Some("unknown-model-v1"));
        assert_eq!(w, 64_000);
    }

    #[test]
    fn effective_threshold_with_claude() {
        // (200_000 - 20_000 - 13_000) * 4 = 668_000
        assert_eq!(effective_auto_compact_threshold(Some(200_000)), 668_000);
    }

    #[test]
    fn effective_threshold_conservative_fallback() {
        // (64_000 - 20_000 - 13_000) * 4 = 124_000
        assert_eq!(effective_auto_compact_threshold(None), 124_000);
    }
}
