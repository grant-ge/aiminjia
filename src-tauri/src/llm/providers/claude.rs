//! Claude (Anthropic) provider — Anthropic Messages API format.
//!
//! Uses `x-api-key` auth, `input_schema` for tools, and Anthropic-specific SSE
//! event types (`content_block_start`, `content_block_delta`, `message_delta`).
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use log::{debug, error, warn};
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;

use crate::llm::streaming::{
    parse_sse_line, LlmRequest, LlmResponse, StopReason, StreamBox, StreamEvent,
    TokenUsage, ToolCall,
};

use super::LlmProviderTrait;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

/// Anthropic Claude provider.
pub struct ClaudeProvider {
    client: Client,
    api_key: String,
    model: String,
}

// ---------------------------------------------------------------------------
// SSE stream state — tracks partial tool_use accumulation across events
// ---------------------------------------------------------------------------

/// Mutable state carried through the SSE stream via `unfold`.
struct SseState {
    /// Leftover bytes not yet split into lines.
    buffer: String,
    /// Tool-use block currently being accumulated (id).
    current_tool_id: Option<String>,
    /// Tool-use block currently being accumulated (name).
    current_tool_name: Option<String>,
    /// Partial JSON fragments for the tool input.
    tool_json_fragments: String,
    /// Input token count (reported in `message_start`).
    input_tokens: u32,
}

impl SseState {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            current_tool_id: None,
            current_tool_name: None,
            tool_json_fragments: String::new(),
            input_tokens: 0,
        }
    }
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            client: super::build_http_client(),
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    /// Build the JSON request body for Anthropic Messages API.
    fn build_request_body(&self, request: &LlmRequest) -> Value {
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|msg| {
                json!({
                    "role": msg.role,
                    "content": msg.content,
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "messages": messages,
        });

        // Only include temperature if non-default (Anthropic default is 1.0)
        if (request.temperature - 1.0).abs() > f32::EPSILON {
            body["temperature"] = json!(request.temperature);
        }

        // Anthropic uses `input_schema` instead of OpenAI's `parameters`
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect();
            body["tools"] = json!(tools);
        }

        if request.stream {
            body["stream"] = json!(true);
        }

        body
    }

    /// Parse the non-streaming response into `LlmResponse`.
    fn parse_response(body: &Value) -> Result<LlmResponse> {
        let content_blocks = body["content"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing 'content' array in Anthropic response"))?;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in content_blocks {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        text_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    let id = block["id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let name = block["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let arguments = block["input"].clone();
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
                other => {
                    debug!("Unknown content block type: {:?}", other);
                }
            }
        }

        let stop_reason =
            Self::parse_stop_reason(body["stop_reason"].as_str().unwrap_or("end_turn"));

        let usage = TokenUsage {
            input_tokens: body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse {
            content: text_parts.join(""),
            stop_reason,
            usage,
            tool_calls,
        })
    }

    /// Map Anthropic stop_reason strings to our `StopReason` enum.
    fn parse_stop_reason(reason: &str) -> StopReason {
        match reason {
            "end_turn" => StopReason::EndTurn,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            other => {
                warn!("Unknown Anthropic stop_reason: {}", other);
                StopReason::EndTurn
            }
        }
    }
}

impl LlmProviderTrait for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        let body = self.build_request_body(&request);

        debug!("Claude send request to model: {}", self.model);

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!(
                "Anthropic API error ({}): {}",
                status,
                response_text
            ));
        }

        let response_json: Value = serde_json::from_str(&response_text)
            .map_err(|e| anyhow!("Failed to parse Anthropic response: {}", e))?;

        Self::parse_response(&response_json)
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        let mut stream_request = request;
        stream_request.stream = true;
        let body = self.build_request_body(&stream_request);

        debug!("Claude stream request to model: {}", self.model);

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Anthropic API stream error ({}): {}",
                status,
                error_text
            ));
        }

        let byte_stream = response.bytes_stream();
        let pinned_byte_stream = Box::pin(byte_stream);
        let state = SseState::new();

        let event_stream = stream::unfold(
            (pinned_byte_stream, state),
            |(mut byte_stream, mut state)| async move {
                loop {
                    // Try to extract a complete line from the buffer
                    if let Some(newline_pos) = state.buffer.find('\n') {
                        let line = state.buffer[..newline_pos].trim_end().to_string();
                        state.buffer = state.buffer[newline_pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }

                        if let Some(data) = parse_sse_line(&line) {
                            if let Some(events) = process_sse_data(&data, &mut state) {
                                return Some((stream::iter(events), (byte_stream, state)));
                            }
                        }
                        continue;
                    }

                    // Need more data from the byte stream
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            let chunk = String::from_utf8_lossy(&bytes);
                            state.buffer.push_str(&chunk);
                        }
                        Some(Err(e)) => {
                            error!("Stream read error: {}", e);
                            let events = vec![StreamEvent::Error {
                                error: format!("Stream read error: {}", e),
                            }];
                            return Some((stream::iter(events), (byte_stream, state)));
                        }
                        None => {
                            // Stream ended — flush any pending tool call
                            if state.current_tool_id.is_some() {
                                let events = finalize_tool_call(&mut state);
                                if !events.is_empty() {
                                    return Some((
                                        stream::iter(events),
                                        (byte_stream, state),
                                    ));
                                }
                            }
                            return None;
                        }
                    }
                }
            },
        )
        .flatten();

        Ok(Pin::from(Box::new(event_stream)))
    }

    async fn validate_key(&self) -> Result<bool> {
        // Send a minimal request to check if the API key is valid
        let body = json!({
            "model": self.model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
        });

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Ok(false);
        }
        if status.is_success() || status.as_u16() == 429 {
            // 429 = rate limited but key is valid
            return Ok(true);
        }

        let error_text = response.text().await.unwrap_or_default();
        Err(anyhow!(
            "Unexpected status {} during key validation: {}",
            status,
            error_text
        ))
    }
}

// ---------------------------------------------------------------------------
// SSE event processing helpers
// ---------------------------------------------------------------------------

/// Process a single SSE JSON data payload. Returns `Some(events)` when there
/// are `StreamEvent`s to emit, `None` otherwise.
fn process_sse_data(data: &str, state: &mut SseState) -> Option<Vec<StreamEvent>> {
    let parsed: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            debug!("Failed to parse SSE data: {} -- raw: {}", e, data);
            return None;
        }
    };

    let event_type = parsed["type"].as_str().unwrap_or("");

    match event_type {
        // message_start: extract input token count
        "message_start" => {
            if let Some(tokens) = parsed["message"]["usage"]["input_tokens"].as_u64() {
                state.input_tokens = tokens as u32;
            }
            None
        }

        // content_block_start: may begin a tool_use block
        "content_block_start" => {
            let block = &parsed["content_block"];
            if block["type"].as_str() == Some("tool_use") {
                state.current_tool_id = block["id"].as_str().map(String::from);
                state.current_tool_name = block["name"].as_str().map(String::from);
                state.tool_json_fragments.clear();
            }
            None
        }

        // content_block_delta: text, thinking, or tool input fragments
        "content_block_delta" => {
            let delta = &parsed["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => delta["text"].as_str().map(|text| {
                    vec![StreamEvent::ContentDelta {
                        delta: text.to_string(),
                    }]
                }),
                Some("thinking_delta") => delta["thinking"].as_str().map(|text| {
                    vec![StreamEvent::ThinkingDelta {
                        delta: text.to_string(),
                    }]
                }),
                Some("input_json_delta") => {
                    if let Some(partial) = delta["partial_json"].as_str() {
                        state.tool_json_fragments.push_str(partial);
                    }
                    None
                }
                _ => None,
            }
        }

        // content_block_stop: finalize any pending tool call
        "content_block_stop" => {
            let events = finalize_tool_call(state);
            if events.is_empty() {
                None
            } else {
                Some(events)
            }
        }

        // message_delta: stop_reason + final usage
        "message_delta" => {
            let stop_reason_str = parsed["delta"]["stop_reason"]
                .as_str()
                .unwrap_or("end_turn");
            let stop_reason = ClaudeProvider::parse_stop_reason(stop_reason_str);
            let output_tokens =
                parsed["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

            Some(vec![StreamEvent::Done {
                stop_reason,
                usage: TokenUsage {
                    input_tokens: state.input_tokens,
                    output_tokens,
                },
            }])
        }

        // ping, message_stop, etc.
        _ => {
            debug!("Ignored SSE event type: {}", event_type);
            None
        }
    }
}

/// If a tool_use block is being accumulated, parse the collected JSON
/// fragments and emit a `ToolCallStart` event. Returns empty vec if no
/// tool was pending.
fn finalize_tool_call(state: &mut SseState) -> Vec<StreamEvent> {
    let id = state.current_tool_id.take();
    let name = state.current_tool_name.take();

    if let (Some(id), Some(name)) = (id, name) {
        let arguments: Value = if state.tool_json_fragments.is_empty() {
            json!({})
        } else {
            serde_json::from_str(&state.tool_json_fragments).unwrap_or_else(|e| {
                warn!(
                    "Failed to parse accumulated tool JSON: {} -- raw: {}",
                    e, state.tool_json_fragments
                );
                json!({})
            })
        };
        state.tool_json_fragments.clear();

        vec![StreamEvent::ToolCallStart {
            tool_call: ToolCall {
                id,
                name,
                arguments,
            },
        }]
    } else {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::streaming::{ChatMessage, ToolDefinition};

    #[test]
    fn test_parse_stop_reason() {
        assert_eq!(
            ClaudeProvider::parse_stop_reason("end_turn"),
            StopReason::EndTurn
        );
        assert_eq!(
            ClaudeProvider::parse_stop_reason("tool_use"),
            StopReason::ToolUse
        );
        assert_eq!(
            ClaudeProvider::parse_stop_reason("max_tokens"),
            StopReason::MaxTokens
        );
        assert_eq!(
            ClaudeProvider::parse_stop_reason("stop_sequence"),
            StopReason::StopSequence
        );
        assert_eq!(
            ClaudeProvider::parse_stop_reason("unknown"),
            StopReason::EndTurn
        );
    }

    #[test]
    fn test_parse_response_text_only() {
        let json_body: Value = serde_json::from_str(
            r#"{
                "content": [{"type": "text", "text": "Hello world"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }"#,
        )
        .unwrap();

        let resp = ClaudeProvider::parse_response(&json_body).unwrap();
        assert_eq!(resp.content, "Hello world");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert!(resp.tool_calls.is_empty());
    }

    #[test]
    fn test_parse_response_with_tool_use() {
        let json_body: Value = serde_json::from_str(
            r#"{
                "content": [
                    {"type": "text", "text": "Let me search."},
                    {
                        "type": "tool_use",
                        "id": "toolu_abc123",
                        "name": "web_search",
                        "input": {"query": "rust async"}
                    }
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 20, "output_tokens": 15}
            }"#,
        )
        .unwrap();

        let resp = ClaudeProvider::parse_response(&json_body).unwrap();
        assert_eq!(resp.content, "Let me search.");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "toolu_abc123");
        assert_eq!(resp.tool_calls[0].name, "web_search");
        assert_eq!(resp.tool_calls[0].arguments["query"], "rust async");
    }

    #[test]
    fn test_parse_response_multiple_text_blocks() {
        let json_body: Value = serde_json::from_str(
            r#"{
                "content": [
                    {"type": "text", "text": "Part one. "},
                    {"type": "text", "text": "Part two."}
                ],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 8}
            }"#,
        )
        .unwrap();

        let resp = ClaudeProvider::parse_response(&json_body).unwrap();
        assert_eq!(resp.content, "Part one. Part two.");
    }

    #[test]
    fn test_build_request_body_minimal() {
        let provider = ClaudeProvider::new("test-key".to_string(), None);
        let request = LlmRequest {
            messages: vec![ChatMessage::text("user", "Hello")],
            tools: vec![],
            max_tokens: 1024,
            temperature: 1.0,
            stream: false,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["max_tokens"], 1024);
        assert!(body.get("tools").is_none());
        assert!(body.get("stream").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn test_build_request_body_with_tools_and_stream() {
        let provider =
            ClaudeProvider::new("key".to_string(), Some("claude-opus-4-20250514".to_string()));
        let request = LlmRequest {
            messages: vec![ChatMessage::text("user", "Search")],
            tools: vec![ToolDefinition {
                name: "web_search".to_string(),
                description: "Search the web".to_string(),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            }],
            max_tokens: 4096,
            temperature: 0.7,
            stream: true,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["model"], "claude-opus-4-20250514");
        assert_eq!(body["stream"], true);
        // f32 0.7 serializes to 0.699999988079071 — compare with tolerance
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001);

        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert!(tools[0].get("input_schema").is_some());
        assert!(tools[0].get("parameters").is_none());
        assert_eq!(tools[0]["name"], "web_search");
    }

    #[test]
    fn test_process_sse_message_start() {
        let mut state = SseState::new();
        let data = r#"{"type":"message_start","message":{"usage":{"input_tokens":42}}}"#;
        let result = process_sse_data(data, &mut state);
        assert!(result.is_none());
        assert_eq!(state.input_tokens, 42);
    }

    #[test]
    fn test_process_sse_text_delta() {
        let mut state = SseState::new();
        let data = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}"#;
        let result = process_sse_data(data, &mut state);
        assert!(result.is_some());
        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta { delta } => assert_eq!(delta, "Hello"),
            _ => panic!("Expected ContentDelta"),
        }
    }

    #[test]
    fn test_process_sse_tool_use_flow() {
        let mut state = SseState::new();

        // 1. content_block_start with tool_use
        let data = r#"{"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_1","name":"calc"}}"#;
        assert!(process_sse_data(data, &mut state).is_none());
        assert_eq!(state.current_tool_id.as_deref(), Some("toolu_1"));
        assert_eq!(state.current_tool_name.as_deref(), Some("calc"));

        // 2. input_json_delta fragments
        let data = r#"{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"x\""}}"#;
        assert!(process_sse_data(data, &mut state).is_none());

        let data = r#"{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":": 42}"}}"#;
        assert!(process_sse_data(data, &mut state).is_none());

        // 3. content_block_stop finalizes the tool call
        let data = r#"{"type":"content_block_stop"}"#;
        let result = process_sse_data(data, &mut state);
        assert!(result.is_some());
        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallStart { tool_call } => {
                assert_eq!(tool_call.id, "toolu_1");
                assert_eq!(tool_call.name, "calc");
                assert_eq!(tool_call.arguments["x"], 42);
            }
            _ => panic!("Expected ToolCallStart"),
        }
    }

    #[test]
    fn test_process_sse_message_delta() {
        let mut state = SseState::new();
        state.input_tokens = 100;

        let data =
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":50}}"#;
        let result = process_sse_data(data, &mut state);
        assert!(result.is_some());
        let events = result.unwrap();
        match &events[0] {
            StreamEvent::Done {
                stop_reason,
                usage,
            } => {
                assert_eq!(*stop_reason, StopReason::EndTurn);
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
            }
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_finalize_tool_call_no_pending() {
        let mut state = SseState::new();
        let events = finalize_tool_call(&mut state);
        assert!(events.is_empty());
    }

    #[test]
    fn test_finalize_tool_call_empty_json() {
        let mut state = SseState::new();
        state.current_tool_id = Some("id1".to_string());
        state.current_tool_name = Some("tool1".to_string());
        // No JSON fragments accumulated
        let events = finalize_tool_call(&mut state);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallStart { tool_call } => {
                assert_eq!(tool_call.arguments, json!({}));
            }
            _ => panic!("Expected ToolCallStart"),
        }
    }
}
