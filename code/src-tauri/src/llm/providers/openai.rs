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

    // Debug: log whether tools are included in the request
    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        log::info!("[STREAM-REQ] url={} model={} tools_count={} tool_names={:?}",
            url, model, tools.len(),
            tools.iter().filter_map(|t| t["function"]["name"].as_str()).collect::<Vec<_>>());
    } else {
        log::info!("[STREAM-REQ] url={} model={} tools_count=0 (no tools in body)", url, model);
    }
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

                    match serde_json::from_str::<Value>(&json_str) {
                        Ok(chunk) => {
                            process_sse_chunk(&chunk, &mut st);
                        }
                        Err(e) => {
                            log::warn!(
                                "[SSE] Failed to parse chunk JSON: err={} line_len={} line='{}'",
                                e,
                                json_str.len(),
                                json_str.chars().take(300).collect::<String>(),
                            );
                        }
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

            // Accumulate argument fragments using .as_str() (standard approach).
            if let Some(frag) = tc["function"]["arguments"].as_str() {
                if !frag.is_empty() {
                    log::debug!("[SSE] Tool args fragment: len={} total_so_far={}", frag.len(), st.tool_args.len() + frag.len());
                }
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
        let arguments = match serde_json::from_str(&st.tool_args) {
            Ok(v) => v,
            Err(e) => {
                let tail: String = st.tool_args.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
                log::error!(
                    "[SSE] Failed to parse tool args as JSON: err={} args_len={} first_500='{}' last_200='{}'",
                    e, st.tool_args.len(), st.tool_args.chars().take(500).collect::<String>(), tail
                );
                // Log hex dump of first 100 bytes to detect invisible control chars
                let hex: String = st.tool_args.bytes().take(100).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                log::error!("[SSE] Tool args hex dump (first 100 bytes): {}", hex);
                // Dump full tool_args to temp file for debugging
                {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs()).unwrap_or(0);
                    let dump_path = format!("/tmp/tool_args_dump_{}.txt", ts);
                    let _ = std::fs::write(&dump_path, &st.tool_args);
                    log::error!("[SSE] Full tool_args dumped to {}", dump_path);
                }
                // Recovery pipeline:
                // 1. Sanitize control chars + unescaped content quotes (gateway bugs)
                // 2. If still invalid, attempt truncated JSON completion (max_tokens hit)
                let sanitized = sanitize_json_control_chars(&st.tool_args);
                match serde_json::from_str(&sanitized) {
                    Ok(v) => {
                        log::info!("[SSE] Tool args recovered after sanitizing control chars");
                        v
                    }
                    Err(e2) => {
                        log::warn!("[SSE] Sanitize didn't fix it ({}), trying truncated JSON repair", e2);
                        let repaired = repair_truncated_json(&sanitized);
                        match serde_json::from_str(&repaired) {
                            Ok(v) => {
                                log::info!("[SSE] Tool args recovered after truncated JSON repair");
                                v
                            }
                            Err(e3) => {
                                log::error!("[SSE] All recovery failed: sanitize={}, repair={}", e2, e3);
                                Value::Null
                            }
                        }
                    }
                }
            }
        };
        let tc = ToolCall {
            id,
            name,
            arguments,
        };
        st.tool_args.clear();
        st.pending_events.push(StreamEvent::ToolCallStart { tool_call: tc });
    }
}

/// Sanitize malformed JSON from API gateway roundtrips.
///
/// Handles two classes of problems that occur when Go-based API gateways
/// convert between Anthropic and OpenAI streaming formats:
///
/// 1. **Raw control characters** (U+0000–U+001F) inside JSON string values.
///    JSON spec requires these be escaped as `\uXXXX`, but gateway roundtrips
///    can produce raw bytes (e.g., real 0x0a newlines inside strings).
///
/// 2. **Unescaped ASCII double-quotes** inside JSON string values. This happens
///    when the gateway loses an escaping layer during format conversion. For
///    example, Chinese emphasis like `"先发制人"` uses ASCII `"` (0x22) which
///    breaks the JSON structure when not escaped as `\"`.
///
/// The function tracks quote/escape state to distinguish structural quotes from
/// content quotes using a look-ahead heuristic: a `"` inside a string that is
/// followed (after optional whitespace) by a JSON structural character
/// (`,` `}` `]` `:`) or EOF is treated as a structural close-quote; otherwise
/// it is escaped as `\"`.
fn sanitize_json_control_chars(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(input.len() + 64);
    let mut in_string = false;
    let mut i = 0;

    while i < len {
        let c = chars[i];

        if in_string {
            if c == '\\' && i + 1 < len {
                // Escape sequence — keep as-is
                result.push(c);
                result.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                // Heuristic: is this a structural close-quote or a content quote?
                // Look ahead past whitespace to find the next meaningful character.
                let mut j = i + 1;
                while j < len && matches!(chars[j], ' ' | '\t' | '\n' | '\r') {
                    j += 1;
                }
                let next = if j < len { Some(chars[j]) } else { None };

                if matches!(next, Some(',' | '}' | ']' | ':') | None) {
                    // Structural close-quote
                    in_string = false;
                    result.push('"');
                } else {
                    // Content quote (e.g., "先发制人" emphasis in Chinese text)
                    // — escape it so JSON remains valid
                    result.push('\\');
                    result.push('"');
                }
                i += 1;
                continue;
            }
            // Inside a string: escape raw control chars (0x00-0x1F)
            if (c as u32) < 0x20 {
                result.push_str(&format!("\\u{:04x}", c as u32));
                i += 1;
                continue;
            }
        } else if c == '"' {
            in_string = true;
        }

        result.push(c);
        i += 1;
    }

    result
}

/// Repair truncated JSON caused by `finish_reason=length` (max_tokens hit).
///
/// When the LLM runs out of tokens mid-way through a tool call's JSON arguments,
/// the accumulated string is syntactically incomplete (unclosed strings, arrays,
/// objects). This function closes all open structures so `serde_json` can parse
/// the partial data. The result is a valid JSON object with whatever fields were
/// completed before truncation.
///
/// Strategy: scan the input tracking nesting depth, then append closing tokens
/// in reverse order (close string → close array → close object).
fn repair_truncated_json(input: &str) -> String {
    let mut result = input.to_string();
    let mut stack: Vec<char> = Vec::new(); // tracks open delimiters: '{', '['
    let mut in_string = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                i += 2; // skip escape
                continue;
            }
            if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' => stack.push('{'),
                '[' => stack.push('['),
                '}' => { stack.pop(); }
                ']' => { stack.pop(); }
                _ => {}
            }
        }
        i += 1;
    }

    // If we ended inside a string, close it
    if in_string {
        result.push('"');
    }

    // Trim trailing comma (e.g., `"items": ["a",` → need to remove the comma before closing)
    let trimmed = result.trim_end();
    if trimmed.ends_with(',') {
        result = trimmed[..trimmed.len() - 1].to_string();
    }

    // Close open brackets/braces in reverse order
    for &opener in stack.iter().rev() {
        match opener {
            '{' => result.push('}'),
            '[' => result.push(']'),
            _ => {}
        }
    }

    result
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
                        "function": { "name": "load_file", "arguments": "" }
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
            assert_eq!(tool_call.name, "load_file");
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

    #[test]
    fn test_sanitize_json_control_chars() {
        // Newline between tokens (valid JSON whitespace) — should be preserved
        let input = "{\n  \"key\": \"value\"\n}";
        assert_eq!(sanitize_json_control_chars(input), input);

        // Newline inside string value — should be escaped
        let input = "{\"content\": \"line1\nline2\"}";
        let sanitized = sanitize_json_control_chars(input);
        assert_eq!(sanitized, "{\"content\": \"line1\\u000aline2\"}");
        // And it should now parse
        let v: Value = serde_json::from_str(&sanitized).unwrap();
        assert_eq!(v["content"].as_str().unwrap(), "line1\nline2");

        // Tab inside string value
        let input = "{\"content\": \"col1\tcol2\"}";
        let sanitized = sanitize_json_control_chars(input);
        assert!(sanitized.contains("\\u0009"));
        let v: Value = serde_json::from_str(&sanitized).unwrap();
        assert_eq!(v["content"].as_str().unwrap(), "col1\tcol2");

        // Chinese characters preserved
        let input = "{\"title\": \"测试报告\"}";
        assert_eq!(sanitize_json_control_chars(input), input);

        // Mixed: structural newlines + newline in string
        let input = "{\n  \"content\": \"第一段\n第二段\"\n}";
        let sanitized = sanitize_json_control_chars(input);
        let v: Value = serde_json::from_str(&sanitized).unwrap();
        assert_eq!(v["content"].as_str().unwrap(), "第一段\n第二段");
    }

    /// Unescaped ASCII quotes inside JSON string values (gateway escaping bug).
    /// Chinese text uses ASCII `"` for emphasis like `"先发制人"`, which breaks
    /// JSON structure when the gateway loses an escaping layer.
    #[test]
    fn test_sanitize_unescaped_content_quotes() {
        // Simplest case: one pair of content quotes
        let input = r#"{"content": "美国宣布"先发制人"打击"}"#;
        assert!(serde_json::from_str::<Value>(input).is_err(), "should be invalid JSON");
        let sanitized = sanitize_json_control_chars(input);
        let v: Value = serde_json::from_str(&sanitized).expect("should parse after sanitize");
        assert_eq!(v["content"].as_str().unwrap(), r#"美国宣布"先发制人"打击"#);
    }

    /// Multiple unescaped content quotes in different fields.
    #[test]
    fn test_sanitize_multiple_content_quotes() {
        let input = r#"{"a": "局势超过"十二日战争"水平", "b": "发动"大规模"军事行动"}"#;
        assert!(serde_json::from_str::<Value>(input).is_err());
        let sanitized = sanitize_json_control_chars(input);
        let v: Value = serde_json::from_str(&sanitized).expect("should parse after sanitize");
        assert_eq!(v["a"].as_str().unwrap(), r#"局势超过"十二日战争"水平"#);
        assert_eq!(v["b"].as_str().unwrap(), r#"发动"大规模"军事行动"#);
    }

    /// Real-world dump file pattern: content quotes + structural newlines.
    #[test]
    fn test_sanitize_real_world_gateway_dump() {
        let input = concat!(
            r#"{"highlight": "局势严峻程度已超过2025年6月"十二日战争"水平。","#,
            r#" "metrics": [{"label": "油价", "value": "$80/桶"}]}"#
        );
        assert!(serde_json::from_str::<Value>(input).is_err());
        let sanitized = sanitize_json_control_chars(input);
        let v: Value = serde_json::from_str(&sanitized).expect("should parse after sanitize");
        let highlight = v["highlight"].as_str().unwrap();
        assert!(highlight.contains(r#""十二日战争""#));
        assert_eq!(v["metrics"][0]["label"].as_str().unwrap(), "油价");
    }

    /// Structural quotes must not be corrupted.
    #[test]
    fn test_sanitize_preserves_valid_json() {
        let input = r#"{"key": "normal value", "num": 42, "arr": ["a", "b"]}"#;
        let sanitized = sanitize_json_control_chars(input);
        assert_eq!(sanitized, input, "valid JSON should not be modified");
    }

    /// Escaped quotes inside strings must be preserved.
    #[test]
    fn test_sanitize_preserves_escaped_quotes() {
        let input = r#"{"content": "he said \"hello\" to her"}"#;
        let sanitized = sanitize_json_control_chars(input);
        assert_eq!(sanitized, input, "already-escaped quotes should stay");
    }

    // ── repair_truncated_json tests ──

    /// Truncated mid-string (e.g., max_tokens hit while writing a string value).
    #[test]
    fn test_repair_truncated_mid_string() {
        let input = r#"{"title": "伊朗局势报告", "sections": [{"heading": "摘要", "content": "未完成的内容"#;
        assert!(serde_json::from_str::<Value>(input).is_err());
        let repaired = repair_truncated_json(input);
        let v: Value = serde_json::from_str(&repaired).expect("should parse after repair");
        assert_eq!(v["title"].as_str().unwrap(), "伊朗局势报告");
        assert_eq!(v["sections"][0]["heading"].as_str().unwrap(), "摘要");
    }

    /// Truncated after a comma (trailing comma before close).
    #[test]
    fn test_repair_truncated_after_comma() {
        let input = r#"{"items": ["a", "b","#;
        let repaired = repair_truncated_json(input);
        let v: Value = serde_json::from_str(&repaired).expect("should parse after repair");
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
    }

    /// Already-valid JSON should pass through unchanged.
    #[test]
    fn test_repair_valid_json_unchanged() {
        let input = r#"{"key": "value"}"#;
        let repaired = repair_truncated_json(input);
        assert_eq!(repaired, input);
    }

    /// Deeply nested truncation.
    #[test]
    fn test_repair_deep_nesting() {
        let input = r#"{"a": {"b": [{"c": "d"#;
        let repaired = repair_truncated_json(input);
        let v: Value = serde_json::from_str(&repaired).expect("should parse");
        assert_eq!(v["a"]["b"][0]["c"].as_str().unwrap(), "d");
    }

    /// Simulate the Anthropic-to-OpenAI gateway streaming pattern.
    /// The gateway converts Anthropic's input_json_delta events to
    /// OpenAI tool_calls delta format. Arguments are accumulated and
    /// parsed when finish_reason="tool_calls" arrives.
    #[test]
    fn test_anthropic_gateway_tool_args_with_newlines() {
        let mut st = test_state();

        // Gateway sends initial chunk with id and name (from content_block_start)
        let start: Value = serde_json::from_str(r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "toolu_01abc",
                        "type": "function",
                        "function": { "name": "generate_report", "arguments": "" }
                    }]
                }
            }]
        }"#).unwrap();
        process_sse_chunk(&start, &mut st);
        assert_eq!(st.tool_id.as_deref(), Some("toolu_01abc"));

        // Simulate what the gateway does: Go json.Marshal converts each
        // partial_json fragment into an arguments string field.
        // Fragments represent pretty-printed JSON with structural newlines.
        // After Go's json roundtrip, the client receives these fragments.
        //
        // Build chunks properly using serde_json to avoid escaping issues.
        let fragments = vec![
            "{\"title\": \"测试报告\"",
            ", \"sections\": [",
            "\n  {",                          // structural newline
            "\n    \"heading\": \"概述\"",
            ",",
            "\n    \"content\": \"第一段\\n第二段\"",  // \\n = escaped newline in JSON string value
            "\n  }",
            "\n]",
            "}",
        ];

        for frag in &fragments {
            let chunk = serde_json::json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": { "arguments": frag }
                        }]
                    }
                }]
            });
            process_sse_chunk(&chunk, &mut st);
        }

        // Finish → should flush
        let finish: Value = serde_json::from_str(r#"{
            "choices": [{"delta": {}, "finish_reason": "tool_calls"}]
        }"#).unwrap();
        process_sse_chunk(&finish, &mut st);

        // Check accumulated args
        let expected_accumulated = fragments.join("");
        eprintln!("Accumulated tool_args: {:?}", expected_accumulated);

        // Verify tool call was parsed correctly
        let tool_events: Vec<_> = st.pending_events.iter()
            .filter(|e| matches!(e, StreamEvent::ToolCallStart { .. }))
            .collect();
        assert_eq!(tool_events.len(), 1);

        if let StreamEvent::ToolCallStart { tool_call } = &tool_events[0] {
            assert_eq!(tool_call.name, "generate_report");
            // The accumulated JSON should parse correctly
            assert!(tool_call.arguments != Value::Null,
                "arguments should not be null! accumulated: {:?}",
                expected_accumulated);
            assert_eq!(
                tool_call.arguments.get("title").and_then(|v| v.as_str()),
                Some("测试报告"),
            );
            assert!(tool_call.arguments.get("sections").is_some());
        }
    }

    /// Test that the accumulated tool_args with real newlines (0x0a) from Go
    /// gateway roundtrip can be parsed by serde_json.
    #[test]
    fn test_tool_args_with_real_newlines_from_gateway() {
        let mut st = test_state();

        // Simulate content_block_start
        let start = serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "toolu_01xyz", "type": "function",
                "function": { "name": "generate_report", "arguments": "" }
            }]}}]
        });
        process_sse_chunk(&start, &mut st);

        // These fragments simulate what the Rust client receives after Go
        // json.Marshal/Unmarshal roundtrip. Structural newlines in the original
        // partial_json become real 0x0a bytes in the arguments string.
        // The JSON escape \n inside string values remains as two chars (\ + n).
        let fragments = vec![
            "{\"title\": \"\u{6D4B}\u{8BD5}\u{62A5}\u{544A}\"",  // 测试报告
            ", \"sections\": [",
            "\n  {",                                               // real 0x0a
            "\n    \"heading\": \"\u{6982}\u{8FF0}\"",             // real 0x0a + 概述
            ",",
            "\n    \"content\": \"\u{7B2C}\u{4E00}\u{6BB5}\\n\u{7B2C}\u{4E8C}\u{6BB5}\"",
            // real 0x0a (structural) + content with \n (JSON escape, two chars)
            "\n  }",
            "\n]",
            "}",
        ];

        for frag in &fragments {
            let chunk = serde_json::json!({
                "choices": [{"delta": {"tool_calls": [{
                    "index": 0,
                    "function": { "arguments": frag }
                }]}}]
            });
            process_sse_chunk(&chunk, &mut st);
        }

        // Finish
        let finish = serde_json::json!({
            "choices": [{"delta": {}, "finish_reason": "tool_calls"}]
        });
        process_sse_chunk(&finish, &mut st);

        // Verify
        let tool_events: Vec<_> = st.pending_events.iter()
            .filter(|e| matches!(e, StreamEvent::ToolCallStart { .. }))
            .collect();
        assert_eq!(tool_events.len(), 1);

        if let StreamEvent::ToolCallStart { tool_call } = &tool_events[0] {
            assert_eq!(tool_call.name, "generate_report");
            assert!(
                tool_call.arguments != Value::Null,
                "arguments must not be null! accumulated args contain real 0x0a newlines"
            );
            assert_eq!(
                tool_call.arguments.get("title").and_then(|v| v.as_str()),
                Some("测试报告"),
            );
            // Content should have a decoded newline from the JSON \n escape
            let content = tool_call.arguments["sections"][0]["content"].as_str().unwrap();
            assert!(content.contains('\n'), "content should contain decoded newline");
        }
    }
}
