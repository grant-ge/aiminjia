//! P1: Intra-step context decay — reduce older tool outputs to save LLM context.
//!
//! During analysis steps, the agent may iterate 10+ times, producing tool results
//! (execute_python stdout, etc.) that accumulate in the message history. Older results
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
/// Only applies when `is_analysis` is true; daily mode messages are returned as-is.
pub fn apply_decay(messages: &[ChatMessage], is_analysis: bool) -> Vec<ChatMessage> {
    if !is_analysis || messages.is_empty() {
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
    let mut truncation_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
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
                name: "execute_python".to_string(),
                arguments: serde_json::json!({}),
            }],
        )
    }

    fn make_tool_result(content: &str) -> ChatMessage {
        ChatMessage::tool_result("tc_1", "execute_python", content.to_string())
    }

    fn make_user(content: &str) -> ChatMessage {
        ChatMessage::text("user", content)
    }

    #[test]
    fn decay_skipped_in_daily_mode() {
        let messages = vec![
            make_user("hello"),
            make_assistant_with_tools("running"),
            make_tool_result(&"x".repeat(10000)),
        ];
        let result = apply_decay(&messages, false);
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
        let result = apply_decay(&messages, true);
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

        let result = apply_decay(&messages, true);

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

        let _ = apply_decay(&messages, true);

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

        let result = apply_decay(&messages, true);
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
            make_assistant_with_tools("phantom"),  // no tool results follow
            make_assistant_with_tools("real"),
            make_tool_result("r1"),
        ];

        let iters = find_iterations(&messages);
        assert_eq!(iters.len(), 1); // phantom should be filtered out
        assert_eq!(iters[0].start, 2);
        assert_eq!(iters[0].end, 4);
    }
}
