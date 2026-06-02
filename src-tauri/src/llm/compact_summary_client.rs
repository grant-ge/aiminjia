//! Production implementation of `CompactSummaryClient` that calls the LLM
//! through `LlmGateway::send_message_with_segments` to generate conversation
//! summaries for auto-compaction.

use std::sync::Arc;

use async_trait::async_trait;

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, SystemPromptSegment};
use crate::models::settings::AppSettings;
use crate::runtime::chat::compact_client::CompactSummaryClient;
use crate::runtime::chat::turn_config::TurnError;

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
    settings: AppSettings,
}

impl LlmCompactSummaryClient {
    pub fn new(gateway: Arc<LlmGateway>, settings: AppSettings) -> Self {
        Self { gateway, settings }
    }
}

/// Convert raw JSON messages to `Vec<ChatMessage>` for the gateway.
///
/// Only keeps `user` and `assistant` roles. Tool result messages and
/// compact boundary messages are stripped — the LLM doesn't need raw
/// tool outputs to generate a summary.
fn convert_to_chat_messages(messages: &[serde_json::Value]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg.get("role")?.as_str()?;
            match role {
                "user" | "assistant" => {
                    let content = msg
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    // Skip compact boundary and summary messages
                    if msg.get("subtype").and_then(|v| v.as_str()) == Some("compact_boundary") {
                        return None;
                    }
                    if msg.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true) {
                        return None;
                    }
                    Some(ChatMessage::text(role, content))
                }
                _ => None,
            }
        })
        .collect()
}

/// Truncate messages if there are too many, keeping the first N and last N.
/// This prevents the compact LLM call itself from hitting PromptTooLong.
fn truncate_messages_for_compact(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    // Keep at most 100 messages: first 10 (context) + last 90 (recent)
    const MAX_COMPACT_MESSAGES: usize = 100;
    const HEAD_KEEP: usize = 10;

    if messages.len() <= MAX_COMPACT_MESSAGES {
        return messages;
    }

    let tail_keep = MAX_COMPACT_MESSAGES - HEAD_KEEP - 1;
    let mut result: Vec<ChatMessage> = Vec::with_capacity(MAX_COMPACT_MESSAGES);
    result.extend(messages[..HEAD_KEEP].iter().cloned());
    result.push(ChatMessage::text(
        "user",
        format!(
            "[中间省略 {} 条消息]",
            messages.len() - HEAD_KEEP - tail_keep
        ),
    ));
    result.extend(messages[messages.len() - tail_keep..].iter().cloned());
    result
}

#[async_trait]
impl CompactSummaryClient for LlmCompactSummaryClient {
    async fn compact_summary(
        &self,
        conversation_id: &str,
        messages: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        let chat_messages = convert_to_chat_messages(messages);
        let mut chat_messages = truncate_messages_for_compact(chat_messages);

        if chat_messages.is_empty() {
            return Ok(String::new());
        }

        let system_segments = vec![SystemPromptSegment {
            text: COMPACT_SYSTEM_PROMPT.to_string(),
            cache: false, // No cache_control — one-shot call, too small
        }];

        // R3.3: PTL retry — if the compact LLM call itself triggers
        // PromptTooLong, truncate further and retry (max 3 attempts).
        const MAX_PTL_RETRIES: usize = 3;
        let mut last_error = None;

        for attempt in 0..=MAX_PTL_RETRIES {
            let result = self
                .gateway
                .send_message_with_segments(
                    &self.settings,
                    chat_messages.clone(),
                    MaskingLevel::Relaxed,
                    None, // system_prompt
                    None, // context_message
                    None, // tool_defs_override
                    8_000,
                    Some(conversation_id),
                    None, // anthropic_multimodal_turn
                    system_segments.clone(),
                    None, // trace_id
                    None, // run_id
                )
                .await;

            match result {
                Ok(response) => return Ok(response.content),
                Err(e) => {
                    let err_str = e.to_string();
                    // Check if this is a PromptTooLong error
                    if err_str.contains("prompt too long")
                        || err_str.contains("too many tokens")
                        || err_str.contains("context length")
                    {
                        // Truncate 30% more aggressively and retry
                        let new_len = (chat_messages.len() * 7) / 10;
                        if new_len < 20 || attempt >= MAX_PTL_RETRIES {
                            last_error = Some(TurnError::LlmError(format!(
                                "compact_summary PTL exhausted after {} retries: {}",
                                attempt, err_str
                            )));
                            break;
                        }
                        chat_messages = chat_messages[chat_messages.len() - new_len..].to_vec();
                        log::warn!(
                            "[compact] PTL retry {}/{}: truncated to {} messages",
                            attempt + 1,
                            MAX_PTL_RETRIES,
                            chat_messages.len()
                        );
                        last_error =
                            Some(TurnError::LlmError(format!("compact_summary retry: {}", err_str)));
                        continue;
                    }
                    last_error = Some(TurnError::LlmError(format!(
                        "compact_summary failed: {}",
                        err_str
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
    fn test_convert_strips_tool_messages() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "thinking", "toolCalls": []}),
            serde_json::json!({"role": "tool", "toolCallId": "tc1", "content": "result"}),
            serde_json::json!({"role": "assistant", "content": "done"}),
        ];
        let result = convert_to_chat_messages(&messages);
        // tool message should be stripped, 3 messages remain
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
        assert_eq!(result[2].role, "assistant");
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

    #[test]
    fn test_truncate_under_limit() {
        let messages: Vec<ChatMessage> = (0..50)
            .map(|i| ChatMessage::text("user", format!("msg {}", i)))
            .collect();
        let result = truncate_messages_for_compact(messages.clone());
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn test_truncate_over_limit() {
        let messages: Vec<ChatMessage> = (0..200)
            .map(|i| ChatMessage::text("user", format!("msg {}", i)))
            .collect();
        let result = truncate_messages_for_compact(messages);
        // 10 head + 1 truncation marker + 89 tail = 100
        assert_eq!(result.len(), 100);
        assert_eq!(result[0].content, "msg 0");
        assert_eq!(result[10].content, "[中间省略 101 条消息]");
        assert_eq!(result.last().unwrap().content, "msg 199");
    }
}
