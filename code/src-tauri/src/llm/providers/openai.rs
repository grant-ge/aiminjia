//! OpenAI GPT-4o provider.
//!
//! Endpoint: `https://api.openai.com/v1/chat/completions`
//! Supports tool use via the function calling format.
//!
//! This module also exposes `pub(super)` helpers that other OpenAI-compatible
//! providers (DeepSeek V3, DeepSeek R1, Volcano) reuse for request building,
//! response parsing, and SSE stream process.
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};

use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{
    parse_sse_line, ChatMessage, LlmRequest, LlmResponse, StopReason, StreamBox, StreamEvent,
    TokenUsage, ToolCall,
};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-4o";

// ---------------------------------------------------------------------------
// Provider struct
// ---------------------------------------------------------------------------

/// OpenAI GPT-4o provider.
pub struct OpenAiProvider {
    api_key: String,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: super::build_http_client(),
        }
    }
}

impl LlmProviderTrait for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        send_openai_compat(
            &self.client,
            &self.api_key,
            API_URL,
            DEFAULT_MODEL,
            &request,
            true,
        )
        .await
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        stream_openai_compat(
            &self.client,
            &self.api_key,
            API_URL,
            DEFAULT_MODEL,
            &request,
            true,
            false,
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        validate_key_openai_compat(&self.client, &self.api_key, API_URL, DEFAULT_MODEL).await
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for OpenAI-compatible APIs (pub(super) for sibling modules)
// ---------------------------------------------------------------------------

/// Build the JSON request body in OpenAI chat completions format.
pub(super) fn build_request_body(
    request: &LlmRequest,
    model: &str,
    stream: bool,
    include_tools: bool,
) -> Value {
    log::info!(
        "[API-BUILD] model={} stream={} include_tools={} msg_count={} tool_count={}",
        model, stream, include_tools, request.messages.len(), request.tools.len()
    );
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m| {
            let mut msg = json!({
                "role": m.role,
                "content": m.content,
            });
            // Assistant messages with tool_calls
            if let Some(ref tcs) = m.tool_calls {
                let tc_json: Vec<Value> = tcs.iter().map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        }
                    })
                }).collect();
                msg["tool_calls"] = json!(tc_json);
                // OpenAI spec: content can be null when tool_calls present
                if m.content.is_empty() {
                    msg["content"] = Value::Null;
                }
            }
            // Tool result messages
            if let Some(ref tc_id) = m.tool_call_id {
                msg["tool_call_id"] = json!(tc_id);
            }
            if let Some(ref name) = m.name {
                msg["name"] = json!(name);
            }
            msg
        })
        .collect();

    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "stream": stream,
    });

    if include_tools && !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = json!(tools);
    }

    body
}

/// Parse a non-streaming OpenAI-format response JSON into `LlmResponse`.
pub(super) fn parse_response(data: &Value) -> Result<LlmResponse> {
    let choice = data["choices"]
        .get(0)
        .ok_or_else(|| anyhow!("No choices in response"))?;

    // Some models (DeepSeek R1) put reasoning in `reasoning_content`.
    // We prepend it to `content` so callers can access the full output.
    let reasoning = choice["message"]
        .get("reasoning_content")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let main_content = choice["message"]["content"]
        .as_str()
        .unwrap_or("");

    let content = if reasoning.is_empty() {
        main_content.to_string()
    } else {
        format!("{}\n\n{}", reasoning, main_content)
    };

    let finish_reason = match choice["finish_reason"].as_str().unwrap_or("stop") {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        _ => StopReason::EndTurn,
    };

    let usage = if let Some(u) = data.get("usage") {
        TokenUsage {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
        }
    } else {
        TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
        }
    };

    let tool_calls = parse_tool_calls_from_message(&choice["message"]);

    Ok(LlmResponse {
        content,
        stop_reason: finish_reason,
        usage,
        tool_calls,
    })
}

/// Extract tool calls from an OpenAI-format message object.
fn parse_tool_calls_from_message(message: &Value) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    if let Some(tcs) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tcs {
            let id = tc["id"].as_str().unwrap_or("").to_string();
            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
            let arguments: Value = tc["function"]["arguments"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);
            calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }
    }
    calls
}

/// Send a non-streaming request to an OpenAI-compatible endpoint.
pub(super) async fn send_openai_compat(
    client: &Client,
    api_key: &str,
    url: &str,
    model: &str,
    request: &LlmRequest,
    include_tools: bool,
) -> Result<LlmResponse> {
    let body = build_request_body(request, model, false, include_tools);

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let error_text = resp.text().await.unwrap_or_default();
        log::error!("Streaming API error: status={}, url={}, body={}", status.as_u16(), url, error_text);
        return Err(anyhow!("API error ({}): {}", status.as_u16(), error_text));
    }

    let data: Value = resp.json().await?;
    parse_response(&data)
}

/// Start a streaming request to an OpenAI-compatible endpoint and return a
/// `StreamBox` that yields `StreamEvent` items.
///
/// When `emit_thinking` is true, `reasoning_content` deltas are emitted as
/// `StreamEvent::ThinkingDelta` (used by DeepSeek R1).
pub(super) async fn stream_openai_compat(
    client: &Client,
    api_key: &str,
    url: &str,
    model: &str,
    request: &LlmRequest,
    include_tools: bool,
    emit_thinking: bool,
) -> Result<StreamBox> {
    let body = build_request_body(request, model, true, include_tools);

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let error_text = resp.text().await.unwrap_or_default();
        log::error!("Streaming API error: status={}, url={}, body={}", status.as_u16(), url, error_text);
        return Err(anyhow!(
            "Streaming API error ({}): {}",
            status.as_u16(),
            error_text
        ));
    }

    let byte_stream = resp.bytes_stream();
    let event_stream = sse_bytes_to_events(byte_stream, emit_thinking);
    Ok(Box::pin(event_stream))
}

/// Validate an API key by sending a minimal request.
pub(super) async fn validate_key_openai_compat(
    client: &Client,
    api_key: &str,
    url: &str,
    model: &str,
) -> Result<bool> {
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        tools: vec![],
        max_tokens: 5,
        temperature: 0.0,
        stream: false,
    };

    let body = build_request_body(&request, model, false, false);

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    Ok(resp.status().is_success())
}

// ---------------------------------------------------------------------------
// SSE stream processing
// ---------------------------------------------------------------------------

/// Internal mutable state for the SSE unfold loop.
struct SseState<S> {
    inner: S,
    buffer: String,
    emit_thinking: bool,
    done: bool,
    /// Buffered events ready to yield (from a single SSE data line that
    /// produced multiple events).
    pending_events: Vec<StreamEvent>,
    // Tool-call accumulation across streamed chunks.
    tool_id: Option<String>,
    tool_name: Option<String>,
    tool_args: String,
    /// The actual finish_reason from the API (e.g. "tool_calls" → ToolUse).
    /// Stored when we see `finish_reason` in a chunk, used when emitting Done.
    final_stop_reason: Option<StopReason>,
}

/// Convert a raw byte stream (from `reqwest::Response::bytes_stream()`) into
/// a `futures::Stream<Item = StreamEvent>` by parsing SSE framing, then
/// interpreting each JSON chunk according to the OpenAI chat completions
/// streaming protocol.
fn sse_bytes_to_events(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>>
        + Send
        + Unpin
        + 'static,
    emit_thinking: bool,
) -> impl futures::Stream<Item = StreamEvent> + Send + 'static {
    let state = SseState {
        inner: byte_stream,
        buffer: String::new(),
        emit_thinking,
        done: false,
        pending_events: Vec::new(),
        tool_id: None,
        tool_name: None,
        tool_args: String::new(),
        final_stop_reason: None,
    };

    stream::unfold(state, |mut st| async move {
        loop {
            if st.done {
                return None;
            }

            // Drain any buffered events first.
            if !st.pending_events.is_empty() {
                let evt = st.pending_events.remove(0);
                if matches!(evt, StreamEvent::Done { .. }) {
                    st.done = true;
                }
                return Some((evt, st));
            }

            // Try to extract a complete line from the buffer.
            if let Some(pos) = st.buffer.find('\n') {
                let line = st.buffer[..pos].trim_end_matches('\r').to_string();
                st.buffer = st.buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(json_str) = parse_sse_line(&line) {
                    if json_str == "[DONE]" {
                        flush_pending_tool(&mut st);
                        let reason = st.final_stop_reason.take()
                            .unwrap_or(StopReason::EndTurn);
                        st.pending_events.push(StreamEvent::Done {
                            stop_reason: reason,
                            usage: TokenUsage::default(),
                        });
                        continue;
                    }

                    if let Ok(chunk) = serde_json::from_str::<Value>(&json_str) {
                        process_sse_chunk(&chunk, &mut st);
                    }
                }

                continue;
            }

            // Need more data from the byte stream.
            match st.inner.next().await {
                Some(Ok(bytes)) => {
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        st.buffer.push_str(&text);
                    }
                }
                Some(Err(e)) => {
                    st.done = true;
                    return Some((
                        StreamEvent::Error { error: format!("Stream read error: {}", e) },
                        st,
                    ));
                }
                None => {
                    // End of stream.
                    flush_pending_tool(&mut st);
                    let reason = st.final_stop_reason.take()
                        .unwrap_or(StopReason::EndTurn);
                    st.pending_events.push(StreamEvent::Done {
                        stop_reason: reason,
                        usage: TokenUsage::default(),
                    });
                    continue;
                }
            }
        }
    })
}

/// Process a single parsed SSE JSON chunk and push events into
/// `st.pending_events`.
fn process_sse_chunk<S>(chunk: &Value, st: &mut SseState<S>) {
    let Some(choice) = chunk["choices"].get(0) else {
        return;
    };
    let delta = &choice["delta"];

    // Reasoning content (DeepSeek R1).
    if st.emit_thinking {
        if let Some(thinking) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
            if !thinking.is_empty() {
                st.pending_events
                    .push(StreamEvent::ThinkingDelta { delta: thinking.to_string() });
            }
        }
    }

    // Text content delta.
    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
        if !content.is_empty() {
            st.pending_events
                .push(StreamEvent::ContentDelta { delta: content.to_string() });
        }
    }

    // Tool call deltas.
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tool_calls {
            if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                if !id.is_empty() {
                    // Check if this is truly a NEW tool call or a continuation
                    // of the current one. Some providers (e.g. Qwen) send the same
                    // tool call id on every continuation chunk, not just the first.
                    let is_new = st.tool_id.as_deref() != Some(id);
                    if is_new {
                        // Flush the previous tool call (if any) before starting a new one.
                        flush_pending_tool(st);
                        st.tool_id = Some(id.to_string());
                        log::info!("[SSE] New tool call: id={}", id);
                    }
                }
            }

            // Update tool name if provided (may arrive in any chunk).
            if let Some(name) = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()) {
                if !name.is_empty() {
                    st.tool_name = Some(name.to_string());
                }
            }

            // Accumulate argument fragments.
            if let Some(frag) = tc["function"]["arguments"].as_str() {
                st.tool_args.push_str(frag);
            }
        }
    }

    // Finish reason — flush tool call if any, and store the reason for the
    // final Done event.
    if let Some(reason_str) = choice.get("finish_reason").and_then(|v| v.as_str()) {
        log::info!("[SSE] finish_reason={}", reason_str);
        flush_pending_tool(st);
        let reason = match reason_str {
            "stop" => StopReason::EndTurn,
            "tool_calls" => StopReason::ToolUse,
            "length" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            _ => StopReason::EndTurn,
        };
        st.final_stop_reason = Some(reason);
    }
}

/// If there is an in-progress tool call being accumulated, finalize it and
/// push a `ToolCallStart` event.
fn flush_pending_tool<S>(st: &mut SseState<S>) {
    if let Some(id) = st.tool_id.take() {
        let name = st.tool_name.take().unwrap_or_default();
        let args_preview: String = st.tool_args.chars().take(200).collect();
        log::info!(
            "[SSE] Flushing tool call: id={} name={} args_len={} args='{}'…",
            id, name, st.tool_args.len(), args_preview
        );
        let tc = ToolCall {
            id,
            name,
            arguments: serde_json::from_str(&st.tool_args).unwrap_or(Value::Null),
        };
        st.tool_args.clear();
        st.pending_events.push(StreamEvent::ToolCallStart { tool_call: tc });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    /// Helper: create a minimal SseState for testing process_sse_chunk.
    fn test_state() -> SseState<futures::stream::Empty<Result<bytes::Bytes, reqwest::Error>>> {
        SseState {
            inner: stream::empty(),
            buffer: String::new(),
            emit_thinking: false,
            done: false,
            pending_events: Vec::new(),
            tool_id: None,
            tool_name: None,
            tool_args: String::new(),
            final_stop_reason: None,
        }
    }

    /// Simulate the Qwen SSE pattern where the same tool call id is sent
    /// across multiple continuation chunks. The parser must accumulate args
    /// into a single ToolCall instead of flushing on every chunk.
    #[test]
    fn test_tool_call_same_id_across_chunks_not_split() {
        let mut st = test_state();

        // Chunk 1: new tool call with id and name
        let chunk1: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_abc123",
                        "function": { "name": "analyze_file", "arguments": "" }
                    }]
                }
            }]
        }"#).unwrap();
        process_sse_chunk(&chunk1, &mut st);
        assert!(st.pending_events.is_empty(), "no flush yet");
        assert_eq!(st.tool_id.as_deref(), Some("call_abc123"));

        // Chunk 2: same id, continuation with partial args (Qwen pattern)
        let chunk2: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_abc123",
                        "function": { "name": "", "arguments": "{\"file_id\":" }
                    }]
                }
            }]
        }"#).unwrap();
        process_sse_chunk(&chunk2, &mut st);
        assert!(st.pending_events.is_empty(), "should NOT flush — same id");
        assert_eq!(st.tool_args, "{\"file_id\":");

        // Chunk 3: no id, just more args
        let chunk3: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "function": { "arguments": " \"abc\"}" }
                    }]
                }
            }]
        }"#).unwrap();
        process_sse_chunk(&chunk3, &mut st);
        assert!(st.pending_events.is_empty(), "still accumulating");
        assert_eq!(st.tool_args, "{\"file_id\": \"abc\"}");

        // Chunk 4: finish_reason triggers flush
        let chunk4: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        }"#).unwrap();
        process_sse_chunk(&chunk4, &mut st);

        // Should produce exactly 1 ToolCallStart event
        let tool_events: Vec<_> = st.pending_events.iter()
            .filter(|e| matches!(e, StreamEvent::ToolCallStart { .. }))
            .collect();
        assert_eq!(tool_events.len(), 1, "exactly one tool call, not split");

        if let StreamEvent::ToolCallStart { tool_call } = &tool_events[0] {
            assert_eq!(tool_call.id, "call_abc123");
            assert_eq!(tool_call.name, "analyze_file");
            assert_eq!(tool_call.arguments, serde_json::json!({"file_id": "abc"}));
        }
    }

    /// When a truly different tool call id arrives, the previous one should
    /// be flushed correctly.
    #[test]
    fn test_tool_call_different_ids_flush_correctly() {
        let mut st = test_state();

        // First tool call
        let chunk1: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": { "name": "tool_a", "arguments": "{\"x\":1}" }
                    }]
                }
            }]
        }"#).unwrap();
        process_sse_chunk(&chunk1, &mut st);
        assert!(st.pending_events.is_empty());

        // Second tool call with different id → should flush the first
        let chunk2: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_2",
                        "function": { "name": "tool_b", "arguments": "{\"y\":2}" }
                    }]
                }
            }]
        }"#).unwrap();
        process_sse_chunk(&chunk2, &mut st);

        // First tool call should have been flushed
        assert_eq!(st.pending_events.len(), 1);
        if let StreamEvent::ToolCallStart { tool_call } = &st.pending_events[0] {
            assert_eq!(tool_call.id, "call_1");
            assert_eq!(tool_call.name, "tool_a");
        }

        // Second tool call still pending
        assert_eq!(st.tool_id.as_deref(), Some("call_2"));
    }
}
