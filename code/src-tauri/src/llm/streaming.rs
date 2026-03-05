//! SSE stream parser and Tauri event emission for LLM streaming responses.
#![allow(dead_code)]

use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Token usage from an LLM response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Events emitted during streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    /// Text content delta
    ContentDelta { delta: String },
    /// Thinking/reasoning content (for R1-style models)
    ThinkingDelta { delta: String },
    /// Model wants to call a tool
    ToolCallStart { tool_call: ToolCall },
    /// Stream completed
    Done {
        stop_reason: StopReason,
        usage: TokenUsage,
    },
    /// Error occurred
    Error { error: String },
}

/// Full (non-streaming) LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponse {
    pub content: String,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    pub tool_calls: Vec<ToolCall>,
}

/// Messages sent to the LLM.
///
/// Supports three roles:
/// - `"user"` / `"system"`: plain text messages (`content` only)
/// - `"assistant"`: may include `tool_calls` when the model requests tool use
/// - `"tool"`: tool execution result, requires `tool_call_id`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Tool calls made by the assistant (only for role="assistant").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call this message is responding to (only for role="tool").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool name (only for role="tool").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    /// Create a simple text message (user, assistant, or system).
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// Create an assistant message that includes tool calls.
    pub fn assistant_with_tool_calls(content: String, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: &str, tool_name: &str, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            name: Some(tool_name.to_string()),
        }
    }
}

/// A tool definition for the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

/// Request to send to an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stream: bool,
}

impl Default for LlmRequest {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: 4096,
            temperature: 0.7,
            stream: true,
        }
    }
}

/// Type alias for a boxed stream of StreamEvents.
pub type StreamBox = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

/// Parse a Server-Sent Events (SSE) line.
/// Returns the data content if the line starts with "data: ".
/// Returns `None` for non-data lines, empty lines, comments, or the "[DONE]" sentinel.
pub fn parse_sse_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(data) = trimmed.strip_prefix("data: ") {
        if data == "[DONE]" {
            return None;
        }
        Some(data.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_data_line() {
        let result = parse_sse_line("data: {\"choices\":[]}");
        assert_eq!(result, Some("{\"choices\":[]}".to_string()));
    }

    #[test]
    fn test_parse_sse_done_line() {
        let result = parse_sse_line("data: [DONE]");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_sse_empty_line() {
        let result = parse_sse_line("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_sse_comment_line() {
        let result = parse_sse_line(": this is a comment");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_sse_event_line() {
        let result = parse_sse_line("event: message");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_sse_whitespace_prefix() {
        let result = parse_sse_line("  data: hello  ");
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn test_llm_request_default() {
        let req = LlmRequest::default();
        assert!(req.messages.is_empty());
        assert!(req.tools.is_empty());
        assert_eq!(req.max_tokens, 4096);
        assert!((req.temperature - 0.7).abs() < f32::EPSILON);
        assert!(req.stream);
    }

    #[test]
    fn test_stream_event_serialization() {
        let event = StreamEvent::ContentDelta {
            delta: "Hello".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"contentDelta\""));
        assert!(json.contains("\"delta\":\"Hello\""));
    }

    #[test]
    fn test_stream_event_done_serialization() {
        let event = StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"done\""));
        assert!(json.contains("\"stop_reason\":\"end_turn\""));
        assert!(json.contains("\"inputTokens\":100"));
        assert!(json.contains("\"outputTokens\":50"));
    }

    #[test]
    fn test_stop_reason_equality() {
        assert_eq!(StopReason::EndTurn, StopReason::EndTurn);
        assert_ne!(StopReason::EndTurn, StopReason::MaxTokens);
    }

    #[test]
    fn test_tool_call_serialization() {
        let tc = ToolCall {
            id: "call_123".to_string(),
            name: "run_code".to_string(),
            arguments: serde_json::json!({"code": "print('hi')"}),
        };
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("\"id\":\"call_123\""));
        assert!(json.contains("\"name\":\"run_code\""));
    }

    #[test]
    fn test_llm_response_deserialization() {
        let json = r#"{
            "content": "Hello world",
            "stopReason": "end_turn",
            "usage": { "inputTokens": 10, "outputTokens": 20 },
            "toolCalls": []
        }"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content, "Hello world");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 20);
        assert!(resp.tool_calls.is_empty());
    }
}
