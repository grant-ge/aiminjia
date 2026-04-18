// src-tauri/src/runtime/chat/tool_result_collector.rs
//!
//! Pure-data transform: takes a `Vec<ToolRoundResult>` (one per tool call)
//! and produces the structured data the driver needs to advance the turn:
//!
//! - JSON tool-result messages ready to append to the LLM messages list
//! - Serialised `FileMeta` objects for downstream file-claim verification
//! - New generated file IDs extracted from tool output content
//! - Success / error counts for metrics / safeguard logic
//!
//! This module is **transport-neutral** and has **no side effects** (no
//! logging, no persistence, no masking).  Callers are responsible for
//! applying masking *before* passing content here, or applying it after.
//! Currently, masking is intentionally left to the caller because it
//! depends on an external `MaskingContext` — see the FIXME in the driver.

use serde_json::{json, Value as JsonValue};

use crate::runtime::chat::tool_round_driver::ToolRoundResult;

/// Collected output from a single tool round.
#[derive(Debug)]
pub struct ToolRoundResults {
    /// Tool-result messages ready to be appended to the LLM messages list.
    ///
    /// Each entry is a JSON object compatible with the OpenAI/Anthropic
    /// "tool" message format:
    /// `{ "role": "tool", "toolCallId": "...", "name": "...", "content": "..." }`
    pub tool_result_messages: Vec<JsonValue>,

    /// Additional context messages requested by tool executions for the next LLM step.
    pub context_modifier_messages: Vec<JsonValue>,

    /// `FileMeta` objects serialised to JSON, one per tool that produced a file.
    /// Consumers (e.g. `verify_file_claims`) receive these to cross-check LLM output.
    pub new_file_metas: Vec<JsonValue>,

    /// File IDs extracted from tool result content (JSON `fileId` field or
    /// plain-text `fileId: <uuid>` lines).
    pub new_generated_file_ids: Vec<String>,

    /// Number of tool calls that completed without error.
    pub success_count: usize,

    /// Number of tool calls that completed with an error (including blocked and AskRequired).
    pub error_count: usize,
}

/// Helper: truncate `s` at the largest byte position `<= max_bytes` that
/// falls on a UTF-8 character boundary.
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

/// Collect one tool round's results into structured output.
///
/// # Arguments
/// * `round_results` — results returned by `ToolRoundDriver::execute_round`.
pub fn collect_results(round_results: Vec<ToolRoundResult>) -> ToolRoundResults {
    let mut tool_result_messages: Vec<JsonValue> = Vec::with_capacity(round_results.len());
    let mut context_modifier_messages: Vec<JsonValue> = Vec::new();
    let mut new_file_metas: Vec<JsonValue> = Vec::new();
    let mut new_generated_file_ids: Vec<String> = Vec::new();
    let mut success_count: usize = 0;
    let mut error_count: usize = 0;

    for round_result in &round_results {
        // Unpack the result into common fields for downstream processing.
        // AskRequired outcomes use the helper methods which synthesise the
        // "permission ask required" content string for the LLM.
        let (tr_id, tr_name, tr_content, tr_is_error, max_tool_result_chars) = match round_result {
            ToolRoundResult::Ok(outcome) => (
                outcome.tool_call_id(),
                outcome.tool_name(),
                outcome.content(),
                outcome.is_error(),
                outcome.max_result_size_chars(),
            ),
            ToolRoundResult::Blocked(blocked) => (
                blocked.tool_call_id.as_str(),
                blocked.tool_name.as_str(),
                blocked.reason.as_str(),
                true, // blocked tools are treated as errors for stats
                8_000,
            ),
        };

        // Collect file_meta from successful tool outcomes into new_file_metas
        // so verify_file_claims and file card semantics work correctly.
        if let ToolRoundResult::Ok(outcome) = round_result {
            if let Some(meta) = outcome.file_meta() {
                // Serialise FileMeta to JsonValue; the consumer (TurnIterationState)
                // stores file metas as Vec<JsonValue>.
                if let Ok(v) = serde_json::to_value(meta) {
                    new_file_metas.push(v);
                }
            }
            if let Some(modifier_message) = outcome.context_modifier_message() {
                context_modifier_messages.push(modifier_message.clone());
            }
        }

        // M1: accumulate per-tool stats
        if tr_is_error {
            error_count += 1;
        } else {
            success_count += 1;
        }

        // Collect fileId from tool results
        if !tr_is_error {
            let parsed_json = serde_json::from_str::<serde_json::Value>(tr_content)
                .ok()
                .or_else(|| {
                    tr_content.find('{').and_then(|pos| {
                        serde_json::from_str::<serde_json::Value>(&tr_content[pos..]).ok()
                    })
                });
            if let Some(parsed) = parsed_json {
                if let Some(file_id) = parsed.get("fileId").and_then(|v| v.as_str()) {
                    new_generated_file_ids.push(file_id.to_string());
                }
            }
            for line in tr_content.lines() {
                if let Some(pos) = line.find("fileId:") {
                    let after = &line[pos + 7..].trim_start();
                    let id: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
                        .collect();
                    if id.len() >= 36 && !new_generated_file_ids.contains(&id) {
                        new_generated_file_ids.push(id);
                    }
                }
            }
        }

        // Apply truncation (masking is intentionally left to the caller — the
        // driver applies it before or after depending on its MaskingContext).
        // FIXME(masking): once the driver is fully extracted, masking should
        // be applied by the caller before passing content to collect_results.
        let truncated_result = if tr_content.len() > max_tool_result_chars {
            let end = truncate_at_char_boundary(tr_content, max_tool_result_chars);
            format!(
                "{}\n[Output truncated: exceeded {} char limit. Use a more specific query to get smaller results.]",
                &tr_content[..end],
                max_tool_result_chars
            )
        } else {
            tr_content.to_string()
        };

        // Build the tool-result message as a plain JSON object (camelCase
        // matches the serialisation format of `ChatMessage`).
        tool_result_messages.push(json!({
            "role": "tool",
            "toolCallId": tr_id,
            "name": tr_name,
            "content": truncated_result,
        }));
    }

    ToolRoundResults {
        tool_result_messages,
        context_modifier_messages,
        new_file_metas,
        new_generated_file_ids,
        success_count,
        error_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::chat::tool_round_types::{BlockedToolOutcome, RuntimeToolCallOutcome};
    use crate::runtime::chat::tool_round_driver::ToolRoundResult;

    fn completed(id: &str, name: &str, content: &str, is_error: bool) -> ToolRoundResult {
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            content: content.to_string(),
            is_error,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
            max_result_size_chars: 8_000,
            context_modifier_message: None,
        })
    }

    #[test]
    fn empty_round_produces_empty_output() {
        let out = collect_results(vec![]);
        assert_eq!(out.success_count, 0);
        assert_eq!(out.error_count, 0);
        assert!(out.tool_result_messages.is_empty());
        assert!(out.context_modifier_messages.is_empty());
    }

    #[test]
    fn success_and_error_counts_are_correct() {
        let results = vec![
            completed("tc1", "search", "found it", false),
            completed("tc2", "load", "error loading", true),
        ];
        let out = collect_results(results);
        assert_eq!(out.success_count, 1);
        assert_eq!(out.error_count, 1);
    }

    #[test]
    fn blocked_tool_counts_as_error() {
        let results = vec![ToolRoundResult::Blocked(BlockedToolOutcome {
            tool_call_id: "tc1".to_string(),
            tool_name: "forbidden".to_string(),
            reason: "not allowed".to_string(),
        })];
        let out = collect_results(results);
        assert_eq!(out.error_count, 1);
        assert_eq!(out.success_count, 0);
    }

    #[test]
    fn message_fields_are_populated_correctly() {
        let results = vec![completed("tc-42", "my_tool", "hello world", false)];
        let out = collect_results(results);
        assert_eq!(out.tool_result_messages.len(), 1);
        let msg = &out.tool_result_messages[0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["toolCallId"], "tc-42");
        assert_eq!(msg["name"], "my_tool");
        assert_eq!(msg["content"], "hello world");
    }

    #[test]
    fn context_modifier_messages_are_collected() {
        let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "edit_file".to_string(),
            content: "done".to_string(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
            max_result_size_chars: 8_000,
            context_modifier_message: Some(json!({
                "role": "user",
                "content": "<context-update>updated</context-update>",
            })),
        })];

        let out = collect_results(results);
        assert_eq!(out.context_modifier_messages.len(), 1);
        assert_eq!(out.context_modifier_messages[0]["role"], "user");
    }

    #[test]
    fn long_content_is_truncated() {
        let long = "a".repeat(10000);
        let results = vec![completed("tc1", "tool", &long, false)];
        let out = collect_results(results);
        let content = out.tool_result_messages[0]["content"].as_str().unwrap();
        assert!(content.contains("[Output truncated:"));
        assert!(content.contains("Use a more specific query"));
        assert!(content.len() < long.len());
    }

    #[test]
    fn file_id_extracted_from_json_content() {
        let content = r#"{"status":"ok","fileId":"aaaabbbb-1234-5678-9012-abcdef012345","name":"report.pdf"}"#;
        let results = vec![completed("tc1", "make_report", content, false)];
        let out = collect_results(results);
        assert_eq!(out.new_generated_file_ids, vec!["aaaabbbb-1234-5678-9012-abcdef012345"]);
    }
}
