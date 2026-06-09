//! SSE stream parser and Tauri event emission for LLM streaming responses.
#![allow(dead_code)]

use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Token usage from an LLM response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Anthropic-style prompt-cache write tokens (tokens that resulted in new
    /// cache entries this turn). `None` when the provider does not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Anthropic-style prompt-cache read tokens (tokens served from existing
    /// cache entries this turn). `None` when the provider does not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Aborted,
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolCall {
    pub fn into_valid(self) -> Result<Self, String> {
        let id = self.id.trim().to_string();
        if id.is_empty() {
            return Err("tool_call id is empty".to_string());
        }

        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(format!("tool_call name is empty for id {id}"));
        }

        if !self.arguments.is_object() {
            return Err(format!(
                "tool_call arguments must be a JSON object for id {id} name {name}"
            ));
        }

        Ok(Self {
            id,
            name,
            arguments: self.arguments,
        })
    }
}

/// Events emitted during streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    /// Text content delta
    ContentDelta { delta: String },
    /// Thinking/reasoning content (for R1-style models)
    ThinkingDelta { delta: String },
    /// Complete thinking block with signature (from Anthropic via gateway)
    ThinkingBlock { block: serde_json::Value },
    /// Model wants to call a tool
    ToolCallStart { tool_call: ToolCall },
    /// Stream completed
    Done {
        stop_reason: StopReason,
        usage: TokenUsage,
    },
    /// Gateway notice such as auto-failover success.
    Notice { notice: serde_json::Value },
    /// Error occurred
    Error { error: String },
    /// Liveness tick — emitted when a real SSE `data:` event arrived on the
    /// wire but carried no user-visible content (Anthropic `ping`,
    /// `input_json_delta` tool-argument fragments, `message_start`,
    /// `signature_delta`, …). Consumers ignore it except to reset
    /// stall/inactivity watchdogs: it proves the stream is alive during long
    /// tool-argument writes or ping-only thinking windows, preventing false
    /// "响应超时（90秒无数据）" timeouts that abort an otherwise-healthy stream.
    Keepalive,
}

/// Full (non-streaming) LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponse {
    pub content: String,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub thinking_blocks: Vec<serde_json::Value>,
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
    /// Whether a tool result represents a tool-level error.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_error: bool,
    /// Thinking/reasoning content from the model (e.g. Claude extended thinking,
    /// DeepSeek R1 reasoning). Must be passed back to the API on subsequent turns
    /// for providers that require it (Anthropic thinking mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Full thinking blocks with signatures from Anthropic API (passed through
    /// the gateway as `_thinking_blocks`). These are opaque and must be echoed
    /// back verbatim so the upstream can validate them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<serde_json::Value>>,
    /// Current-turn Anthropic image sidecar. This is never persisted and only
    /// the Claude/Lotus Anthropic serializer consumes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
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
            is_error: false,
            thinking: None,
            thinking_blocks: None,
            anthropic_multimodal_turn: None,
        }
    }

    /// Create an assistant message that includes tool calls and optional thinking.
    pub fn assistant_with_tool_calls(
        content: String,
        tool_calls: Vec<ToolCall>,
        thinking: Option<String>,
        thinking_blocks: Option<Vec<serde_json::Value>>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
            is_error: false,
            thinking,
            thinking_blocks,
            anthropic_multimodal_turn: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: &str, tool_name: &str, content: String) -> Self {
        Self::tool_result_with_status(tool_call_id, tool_name, content, false)
    }

    pub fn tool_result_with_status(
        tool_call_id: &str,
        tool_name: &str,
        content: String,
        is_error: bool,
    ) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            name: Some(tool_name.to_string()),
            is_error,
            thinking: None,
            thinking_blocks: None,
            anthropic_multimodal_turn: None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    Text { text: String },
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicImageSource {
    Base64 { media_type: String, data: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicMultimodalTurn {
    pub image_blocks: Vec<AnthropicContentBlock>,
    pub image_count: usize,
    pub image_bytes_total: u64,
    pub degraded_count: usize,
}

/// Extended thinking configuration for providers that support explicit reasoning controls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    Adaptive,
    Enabled { budget_tokens: u32 },
    Disabled,
}

/// Request to send to an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stream: bool,
    pub thinking_config: Option<ThinkingConfig>,
    pub anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
    /// Per-block cache_control passthrough for Anthropic-style prompt caching.
    /// When `Some` and non-empty, providers that support block-level
    /// `cache_control` (currently `claude.rs`) render the `system` field as
    /// an array of text blocks where each segment with `cache=true` carries
    /// `cache_control: {type: "ephemeral"}`. Other providers ignore this and
    /// fall back to reading the flattened system message from `messages`.
    pub system_segments: Option<Vec<SystemPromptSegment>>,
    /// Conversation id, when known. Used by the lotus gateway to do
    /// soft conversation-level sticky routing (decision: keep the same
    /// upstream provider within one turn-loop to avoid model jitter).
    /// Sent as the `X-Lotus-Conversation-ID` HTTP header by providers that
    /// route via lotus. `None` / empty disables sticky for this call.
    pub conversation_id: Option<String>,
    /// Trace id for this top-level user turn. Generated by send_message
    /// (uuid). Sent as `X-Aijia-Trace-Id` HTTP header so gateway / SLS can
    /// correlate gateway events back to the client-side turn. Pure
    /// observability — never affects routing or billing.
    pub trace_id: Option<String>,
    /// Run id for the current agentic-loop iteration. Sent as
    /// `X-Aijia-Run-Id` HTTP header. Pure observability.
    pub run_id: Option<String>,
}

/// A single segment of the assembled system prompt with caching intent.
/// Mirrors `PromptCachePolicy` semantics at the wire-protocol boundary so we
/// can transport per-block caching decisions to providers that support them
/// (Anthropic) without leaking renderer types into the llm layer.
#[derive(Debug, Clone, Default)]
pub struct SystemPromptSegment {
    pub text: String,
    /// If true, the segment is marked as a cache breakpoint
    /// (`cache_control: {type: "ephemeral"}` for Anthropic).
    pub cache: bool,
}

impl Default for LlmRequest {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: 8192,
            temperature: 0.7,
            stream: true,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
            conversation_id: None,
            trace_id: None,
            run_id: None,
        }
    }
}

/// Type alias for a boxed stream of StreamEvents.
pub type StreamBox = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

/// Parse a Server-Sent Events (SSE) line.
/// Returns the data content if the line starts with "data: ".
/// Returns `Some("[DONE]")` for the `[DONE]` sentinel so callers can handle it.
/// Returns `None` for non-data lines, empty lines, and comments.
pub fn parse_sse_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(data) = trimmed.strip_prefix("data: ") {
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
        assert_eq!(result, Some("[DONE]".to_string()));
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
        assert_eq!(req.max_tokens, 8192);
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
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
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

#[cfg(test)]
mod chat_message_error_status_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_is_error_defaults_false() {
        let message: ChatMessage = serde_json::from_value(json!({
            "role": "tool",
            "content": "ok",
            "toolCallId": "call_1",
            "name": "Bash"
        }))
        .expect("deserialize chat message");

        assert!(!message.is_error);
    }

    #[test]
    fn tool_result_with_status_serializes_camel_case_is_error() {
        let message =
            ChatMessage::tool_result_with_status("call_1", "Bash", "failed".to_string(), true);
        let value = serde_json::to_value(message).expect("serialize chat message");

        assert_eq!(value["role"], "tool");
        assert_eq!(value["toolCallId"], "call_1");
        assert_eq!(value["name"], "Bash");
        assert_eq!(value["isError"], true);
    }

    #[test]
    fn success_tool_result_omits_is_error() {
        let message = ChatMessage::tool_result("call_1", "Bash", "ok".to_string());
        let value = serde_json::to_value(message).expect("serialize chat message");

        assert!(value.get("isError").is_none());
    }
}
