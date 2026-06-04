//! Production implementation of `CompactSummaryClient` that calls the LLM
//! through `LlmGateway::send_message_with_segments` to generate conversation
//! summaries for auto-compaction.

use std::sync::Arc;

use async_trait::async_trait;

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, SystemPromptSegment};
use crate::runtime::chat::compact_client::CompactSummaryClient;
use crate::runtime::chat::compaction::truncate_messages_for_ptl_retry;
use crate::runtime::chat::turn_config::{ResolvedLlmSettings, TurnError};

const COMPACT_SYSTEM_PROMPT: &str = r#"你是一个对话摘要助手。请用中文生成对话历史的结构化摘要。
要求：
1. 保留所有用户请求和意图
2. 保留所有文件/代码变更和关键决策
3. 保留所有错误和修复过程
4. 丢弃冗余的工具输出和重复内容
5. 保留未完成的操作和待办事项
6. 输出不超过 8000 字符
7. 以"以下是对话历史摘要："开头"#;

/// Production `CompactSummaryClient` backed by `LlmGateway`.
///
/// Sends a non-streaming LLM call with a dedicated compaction system prompt
/// to generate a conversation summary. The summary is consumed by
/// `compact_messages_via_llm()` to replace the full message history with a
/// compact boundary message + summary + tail round.
///
/// **No cache_control**: compaction is a one-shot call — too small to justify
/// occupying ephemeral cache quota, and completely decoupled from the main
/// conversation's cache state.
pub struct LlmCompactSummaryClient {
    gateway: Arc<LlmGateway>,
}

impl LlmCompactSummaryClient {
    pub fn new(gateway: Arc<LlmGateway>) -> Self {
        Self { gateway }
    }
}

/// Convert raw JSON messages to `Vec<ChatMessage>` for the gateway.
///
/// The compact request itself has no tools, so tool-use/result structures are
/// serialized into bounded plain-text markers instead of being sent as tool
/// role messages. This preserves continuity for the summary without violating
/// provider tool-pair invariants.
fn message_content_text(msg: &serde_json::Value) -> String {
    let Some(content) = msg.get("content") else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(text) = content.get("text").and_then(|value| value.as_str()) {
        return text.to_string();
    }
    if let Some(blocks) = content.as_array() {
        return blocks
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(|value| value.as_str()) == Some("text") {
                    block.get("text").and_then(|value| value.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

fn compact_preview(text: &str, max_chars: usize) -> String {
    let mut preview: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn tool_call_id(call: &serde_json::Value) -> String {
    call.get("id")
        .or_else(|| call.get("toolCallId"))
        .or_else(|| call.get("tool_call_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn tool_call_name(call: &serde_json::Value) -> String {
    call.get("name")
        .or_else(|| call.get("toolName"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            call.get("function")
                .and_then(|function| function.get("name"))
                .and_then(|value| value.as_str())
        })
        .unwrap_or("unknown")
        .to_string()
}

fn tool_call_arguments(call: &serde_json::Value) -> String {
    let arguments = call
        .get("arguments")
        .or_else(|| call.get("input"))
        .or_else(|| call.get("args"))
        .or_else(|| {
            call.get("function")
                .and_then(|function| function.get("arguments"))
        });
    arguments
        .and_then(|value| serde_json::to_string(value).ok())
        .map(|value| compact_preview(&value, 600))
        .unwrap_or_default()
}

fn tool_use_markers(msg: &serde_json::Value) -> Vec<String> {
    let Some(calls) = msg
        .get("toolCalls")
        .or_else(|| msg.get("tool_calls"))
        .and_then(|value| value.as_array())
    else {
        return Vec::new();
    };

    calls
        .iter()
        .map(|call| {
            let id = tool_call_id(call);
            let name = tool_call_name(call);
            let arguments = tool_call_arguments(call);
            if arguments.is_empty() {
                format!("[tool_use id={} name={}]", id, name)
            } else {
                format!("[tool_use id={} name={} arguments={}]", id, name, arguments)
            }
        })
        .collect()
}

fn tool_result_marker(msg: &serde_json::Value) -> Option<String> {
    let id = msg
        .get("toolCallId")
        .or_else(|| msg.get("tool_call_id"))
        .and_then(|value| value.as_str())?;
    let name = msg
        .get("name")
        .or_else(|| msg.get("toolName"))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let content = compact_preview(&message_content_text(msg), 1200);
    Some(format!(
        "[tool_result id={} name={}]\n{}",
        id, name, content
    ))
}

fn convert_to_chat_messages(messages: &[serde_json::Value]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg.get("role")?.as_str()?;
            if msg.get("subtype").and_then(|v| v.as_str()) == Some("compact_boundary") {
                return None;
            }
            if msg.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true) {
                return None;
            }
            match role {
                "user" | "assistant" => {
                    let mut parts = Vec::new();
                    let content = message_content_text(msg);
                    if !content.trim().is_empty() {
                        parts.push(content);
                    }
                    parts.extend(tool_use_markers(msg));
                    (!parts.is_empty()).then(|| ChatMessage::text(role, parts.join("\n")))
                }
                "tool" => tool_result_marker(msg).map(|marker| ChatMessage::text("user", marker)),
                _ => None,
            }
        })
        .collect()
}

fn is_prompt_too_long_error(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    lower.contains("prompt too long")
        || lower.contains("too many tokens")
        || lower.contains("context length")
        || lower.contains("context_length_exceeded")
}

fn serialized_message_len(messages: &[serde_json::Value]) -> usize {
    messages
        .iter()
        .map(|message| message.to_string().len())
        .sum()
}

#[async_trait]
impl CompactSummaryClient for LlmCompactSummaryClient {
    async fn compact_summary(
        &self,
        conversation_id: &str,
        messages: &[serde_json::Value],
        llm_settings: &ResolvedLlmSettings,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<String, TurnError> {
        let mut retry_messages = messages.to_vec();
        if convert_to_chat_messages(&retry_messages).is_empty() {
            return Ok(String::new());
        }
        let settings = llm_settings.to_app_settings();

        let system_segments = vec![SystemPromptSegment {
            text: COMPACT_SYSTEM_PROMPT.to_string(),
            cache: false, // No cache_control — one-shot call, too small
        }];

        // R3.3: PTL retry — if the compact LLM call itself triggers
        // PromptTooLong, truncate further and retry (max 3 attempts).
        const MAX_PTL_RETRIES: usize = 3;
        let mut last_error: Option<TurnError> = None;

        for attempt in 0..=MAX_PTL_RETRIES {
            let chat_messages = convert_to_chat_messages(&retry_messages);
            log::debug!(
                "[compact] summary attempt={} conv={} raw_messages={} chat_messages={}",
                attempt + 1,
                conversation_id,
                retry_messages.len(),
                chat_messages.len()
            );
            let result = self
                .gateway
                .send_message_with_segments(
                    &settings,
                    chat_messages.clone(),
                    MaskingLevel::Relaxed,
                    None,         // system_prompt
                    None,         // context_message
                    Some(vec![]), // tool_defs_override
                    8_000,
                    None, // conversation_id: compact summary must not use sticky routing
                    None, // anthropic_multimodal_turn
                    system_segments.clone(),
                    trace_id,
                    run_id,
                )
                .await;

            match result {
                Ok(response) => {
                    if response.content.trim().is_empty() {
                        return Err(TurnError::LlmError(
                            "compact_summary returned empty summary".to_string(),
                        ));
                    }
                    return Ok(response.content);
                }
                Err(e) => {
                    let err_text = e.to_string();
                    if is_prompt_too_long_error(&e) {
                        if attempt >= MAX_PTL_RETRIES {
                            last_error = Some(TurnError::PromptTooLong(format!(
                                "compact_summary PTL exhausted after {} retries: {}",
                                MAX_PTL_RETRIES, err_text
                            )));
                            break;
                        }
                        let truncated = truncate_messages_for_ptl_retry(&retry_messages);
                        if serialized_message_len(&truncated)
                            >= serialized_message_len(&retry_messages)
                        {
                            last_error = Some(TurnError::PromptTooLong(format!(
                                "compact_summary PTL retry could not reduce messages: {}",
                                err_text
                            )));
                            break;
                        }
                        retry_messages = truncated;
                        log::warn!(
                            "[compact] PTL retry {}/{}: truncated to {} raw messages",
                            attempt + 1,
                            MAX_PTL_RETRIES,
                            retry_messages.len()
                        );
                        last_error = Some(TurnError::PromptTooLong(format!(
                            "compact_summary retry: {}",
                            err_text
                        )));
                        continue;
                    }
                    last_error = Some(TurnError::LlmError(format!(
                        "compact_summary failed: {}",
                        err_text
                    )));
                    break;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            TurnError::LlmError("compact_summary failed: unknown error".to_string())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_empty_messages() {
        let result = convert_to_chat_messages(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_convert_serializes_tool_messages_as_text_markers() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({
                "role": "assistant",
                "content": "thinking",
                "toolCalls": [{ "id": "tc1", "name": "Read", "arguments": { "path": "/tmp/a.txt" } }]
            }),
            serde_json::json!({"role": "tool", "toolCallId": "tc1", "name": "Read", "content": "result"}),
            serde_json::json!({"role": "assistant", "content": "done"}),
        ];
        let result = convert_to_chat_messages(&messages);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
        assert!(result[1].content.contains("[tool_use id=tc1 name=Read"));
        assert!(result[1].content.contains("/tmp/a.txt"));
        assert_eq!(result[2].role, "user");
        assert!(result[2].content.contains("[tool_result id=tc1 name=Read]"));
        assert!(result[2].content.contains("result"));
        assert_eq!(result[3].role, "assistant");
    }

    #[test]
    fn test_convert_strips_compact_boundary() {
        let messages = vec![
            serde_json::json!({"role": "system", "subtype": "compact_boundary", "content": "compacted"}),
            serde_json::json!({"role": "user", "isCompactSummary": true, "content": "summary"}),
            serde_json::json!({"role": "user", "content": "real question"}),
        ];
        let result = convert_to_chat_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "real question");
    }
}
