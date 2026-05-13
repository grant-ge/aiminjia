//! Claude (Anthropic) provider — Anthropic Messages API.
//!
//! Uses `x-api-key` auth, `input_schema` for tools, and Anthropic-specific SSE
//! event types (`content_block_start`, `content_block_delta`, `message_delta`).
//!
//! Phase C (2026-05-09): URL is parameterized via `with_url(...)` so this same
//! impl drives both direct anthropic.com calls (via `new(...)`, `is_direct=true`)
//! and the lotus gateway anthropic ingress (via `LotusProvider`,
//! `is_direct=false`). Beta thinking headers are gated on `is_direct` because
//! the gateway is byte-level passthrough but does not advertise beta gating.
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use log::{debug, error, warn};
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;

use crate::llm::streaming::{
    parse_sse_line, AnthropicContentBlock, AnthropicMultimodalTurn, LlmRequest, LlmResponse,
    StopReason, StreamBox, StreamEvent, TokenUsage, ToolCall,
};

use super::LlmProviderTrait;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
const ANTHROPIC_BETA_THINKING: &str = "interleaved-thinking-2025-05-14";

/// Anthropic Claude provider. URL is configurable so the same SSE state
/// machine, body builder, tool/thinking handling can drive both direct
/// `api.anthropic.com` calls and lotus-gateway `/anthropic/v1/messages`
/// calls (the gateway is byte-level passthrough).
///
/// `anthropic-beta` headers are direct-only — the gateway forwards bodies
/// verbatim but does not advertise beta feature gating, so betas like
/// `interleaved-thinking-*` are suppressed when `is_direct=false`.
pub struct ClaudeProvider {
    client: Client,
    api_key: String,
    model: String,
    api_url: String,
    is_direct: bool,
    /// When true, the request body OMITS the `model` field entirely so the
    /// upstream (e.g. lotus gateway) decides routing on its own (protocol +
    /// priority). Used by the Lotus path so the desktop client no longer
    /// pins users to whichever model they happened to start with.
    omit_model: bool,
}

// ---------------------------------------------------------------------------
// SSE stream state — tracks partial tool_use accumulation across events
// ---------------------------------------------------------------------------

/// Mutable state carried through the SSE stream via `unfold`.
struct SseState {
    /// Leftover bytes not yet split into lines.
    buffer: String,
    /// Incomplete UTF-8 bytes from the previous chunk. Multi-byte characters
    /// (e.g. Chinese) can be split across HTTP chunks; we hold the trailing
    /// incomplete sequence here and prepend it to the next chunk.
    incomplete_utf8: Vec<u8>,
    /// Tool-use block currently being accumulated (id).
    current_tool_id: Option<String>,
    /// Tool-use block currently being accumulated (name).
    current_tool_name: Option<String>,
    /// Partial JSON fragments for the tool input.
    tool_json_fragments: String,
    /// Input token count (reported in `message_start`).
    input_tokens: u32,
    /// Cache-creation input tokens (reported in `message_start.usage`).
    cache_creation_input_tokens: Option<u32>,
    /// Cache-read input tokens (reported in `message_start.usage`).
    cache_read_input_tokens: Option<u32>,
    /// Whether the currently open content block is a thinking block.
    in_thinking_block: bool,
    /// Accumulated thinking text for the current thinking block.
    thinking_text: String,
    /// Accumulated signature for the current thinking block.
    thinking_signature: String,
}

impl SseState {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            incomplete_utf8: Vec::new(),
            current_tool_id: None,
            current_tool_name: None,
            tool_json_fragments: String::new(),
            input_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            in_thinking_block: false,
            thinking_text: String::new(),
            thinking_signature: String::new(),
        }
    }
}

impl ClaudeProvider {
    /// Direct anthropic.com client (uses `x-api-key` + `anthropic-beta` headers).
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self::with_url(api_key, model, ANTHROPIC_API_URL.to_string(), true)
    }

    /// Construct with a custom URL. Pass `is_direct=false` for the lotus
    /// gateway path so beta thinking headers are suppressed.
    pub fn with_url(
        api_key: String,
        model: Option<String>,
        api_url: String,
        is_direct: bool,
    ) -> Self {
        Self::with_url_opts(api_key, model, api_url, is_direct, false)
    }

    /// Like `with_url` but lets the caller request the `model` field be
    /// omitted from outgoing request bodies (gateway-decides-model mode).
    pub fn with_url_opts(
        api_key: String,
        model: Option<String>,
        api_url: String,
        is_direct: bool,
        omit_model: bool,
    ) -> Self {
        Self {
            client: super::build_http_client(),
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            api_url,
            is_direct,
            omit_model,
        }
    }

    fn build_request_headers(
        &self,
        request: &LlmRequest,
    ) -> std::collections::HashMap<String, String> {
        let mut headers = std::collections::HashMap::new();
        headers.insert("x-api-key".to_string(), self.api_key.clone());
        headers.insert(
            "anthropic-version".to_string(),
            ANTHROPIC_VERSION.to_string(),
        );
        headers.insert("content-type".to_string(), "application/json".to_string());
        // Beta features (interleaved thinking) are direct-anthropic-only:
        // the lotus gateway forwards bodies verbatim but does not advertise
        // beta gating, and unknown beta tags would be silently ignored.
        if self.is_direct {
            match request.thinking_config {
                Some(crate::llm::streaming::ThinkingConfig::Adaptive)
                | Some(crate::llm::streaming::ThinkingConfig::Enabled { .. }) => {
                    headers.insert(
                        "anthropic-beta".to_string(),
                        ANTHROPIC_BETA_THINKING.to_string(),
                    );
                }
                _ => {}
            }
        }
        headers
    }

    /// Build the JSON request body for Anthropic Messages API.
    ///
    /// Assistant messages with thinking or tool_calls are serialized as
    /// structured content block arrays, as required by the Anthropic API.
    /// Tool result messages use `tool_result` content blocks.
    fn build_request_body(&self, request: &LlmRequest) -> Value {
        let last_user_index = request
            .messages
            .iter()
            .rposition(|message| message.role == "user");
        let messages: Vec<Value> = request
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, msg)| {
                // System messages handled via top-level "system" field
                if msg.role == "system" {
                    return None;
                }

                // Assistant messages may need structured content blocks
                if msg.role == "assistant" {
                    let needs_blocks = msg.thinking.is_some()
                        || msg.thinking_blocks.is_some()
                        || msg.tool_calls.as_ref().map_or(false, |tc| !tc.is_empty());

                    if needs_blocks {
                        let mut blocks: Vec<Value> = Vec::new();

                        // Thinking blocks must come first.
                        // Prefer full blocks (with signatures) over plain text.
                        if let Some(ref tb) = msg.thinking_blocks {
                            blocks.extend(tb.iter().cloned());
                        } else if let Some(ref thinking) = msg.thinking {
                            blocks.push(json!({
                                "type": "thinking",
                                "thinking": thinking,
                            }));
                        }

                        // Text block
                        if !msg.content.is_empty() {
                            blocks.push(json!({
                                "type": "text",
                                "text": msg.content,
                            }));
                        }

                        // Tool use blocks
                        if let Some(ref tool_calls) = msg.tool_calls {
                            for tc in tool_calls {
                                blocks.push(json!({
                                    "type": "tool_use",
                                    "id": tc.id,
                                    "name": tc.name,
                                    "input": tc.arguments,
                                }));
                            }
                        }

                        return Some(json!({
                            "role": "assistant",
                            "content": blocks,
                        }));
                    }
                }

                // Tool result messages use content blocks
                if msg.role == "tool" {
                    if let Some(ref tc_id) = msg.tool_call_id {
                        return Some(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tc_id,
                                "content": msg.content,
                            }],
                        }));
                    }
                }

                if msg.role == "user" {
                    let request_turn = if Some(index) == last_user_index
                        && msg.anthropic_multimodal_turn.is_none()
                    {
                        request.anthropic_multimodal_turn.as_ref()
                    } else {
                        None
                    };
                    if let Some(content) = anthropic_user_content_blocks(msg, request_turn) {
                        return Some(json!({
                            "role": "user",
                            "content": content,
                        }));
                    }
                }

                // Default: plain text message
                Some(json!({
                    "role": msg.role,
                    "content": msg.content,
                }))
            })
            .collect();

        // Extract system prompt (Anthropic requires it at top-level, not in messages)
        let system_content: Option<String> = request
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let mut body = json!({
            "max_tokens": request.max_tokens,
            "messages": messages,
        });
        if !self.omit_model {
            body["model"] = json!(self.model);
        }

        if let Some(system) = system_content {
            if !system.is_empty() {
                // Prefer structured per-block cache passthrough when the
                // caller supplied segments; otherwise fall back to the old
                // single-block wrap-or-string behaviour.
                let segments_non_empty = request
                    .system_segments
                    .as_ref()
                    .map(|segs| segs.iter().any(|s| !s.text.trim().is_empty()))
                    .unwrap_or(false);

                if segments_non_empty && self.supports_prompt_caching() {
                    let segments = request.system_segments.as_ref().expect("checked non-empty");
                    // Anthropic allows at most 4 cache_control breakpoints
                    // across the whole request. The tools array may also
                    // carry one (see below), so keep a conservative cap of
                    // 3 on the system side and warn if exceeded.
                    const MAX_SYSTEM_CACHE_BREAKPOINTS: usize = 3;
                    let mut remaining = MAX_SYSTEM_CACHE_BREAKPOINTS;
                    let mut blocks: Vec<Value> = Vec::with_capacity(segments.len());
                    let mut dropped_cache_flags: usize = 0;
                    for seg in segments {
                        let trimmed = seg.text.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let mut block = json!({
                            "type": "text",
                            "text": seg.text,
                        });
                        if seg.cache {
                            if remaining > 0 {
                                block["cache_control"] = json!({ "type": "ephemeral" });
                                remaining -= 1;
                            } else {
                                dropped_cache_flags += 1;
                            }
                        }
                        blocks.push(block);
                    }
                    if dropped_cache_flags > 0 {
                        log::warn!(
                            "[claude] dropped {} cache_control breakpoints beyond the system-side cap of {}",
                            dropped_cache_flags,
                            MAX_SYSTEM_CACHE_BREAKPOINTS
                        );
                    }
                    if blocks.is_empty() {
                        // all segments empty after trim — fall back to string
                        body["system"] = json!(system);
                    } else {
                        body["system"] = json!(blocks);
                    }
                } else if self.supports_prompt_caching() {
                    body["system"] = json!([{
                        "type": "text",
                        "text": system,
                        "cache_control": { "type": "ephemeral" },
                    }]);
                } else {
                    body["system"] = json!(system);
                }
            }
        }

        // Only include temperature if non-default (Anthropic default is 1.0)
        if (request.temperature - 1.0).abs() > f32::EPSILON {
            body["temperature"] = json!(request.temperature);
        }

        // Anthropic uses `input_schema` instead of OpenAI's `parameters`
        if !request.tools.is_empty() {
            let mut tools: Vec<Value> = request
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

            if self.supports_prompt_caching() {
                if let Some(last) = tools.last_mut() {
                    if let Some(obj) = last.as_object_mut() {
                        obj.insert("cache_control".to_string(), json!({ "type": "ephemeral" }));
                    }
                }
            }
            body["tools"] = json!(tools);
        }

        if request.stream {
            body["stream"] = json!(true);
        }

        match &request.thinking_config {
            Some(crate::llm::streaming::ThinkingConfig::Adaptive) => {
                body["thinking"] = json!({ "type": "adaptive" });
                if let Some(obj) = body.as_object_mut() {
                    obj.remove("temperature");
                }
            }
            Some(crate::llm::streaming::ThinkingConfig::Enabled { budget_tokens }) => {
                let budget = (*budget_tokens).min(request.max_tokens.saturating_sub(1));
                body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
                if let Some(obj) = body.as_object_mut() {
                    obj.remove("temperature");
                }
            }
            Some(crate::llm::streaming::ThinkingConfig::Disabled) | None => {}
        }

        body
    }

    #[doc(hidden)]
    pub fn build_request_body_for_test(&self, request: &LlmRequest) -> Value {
        self.build_request_body(request)
    }

    #[doc(hidden)]
    pub fn build_request_headers_for_test(
        &self,
        request: &LlmRequest,
    ) -> std::collections::HashMap<String, String> {
        self.build_request_headers(request)
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
                    let id = block["id"].as_str().unwrap_or_default().to_string();
                    let name = block["name"].as_str().unwrap_or_default().to_string();
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
            cache_creation_input_tokens: body["usage"]["cache_creation_input_tokens"]
                .as_u64()
                .map(|v| v as u32),
            cache_read_input_tokens: body["usage"]["cache_read_input_tokens"]
                .as_u64()
                .map(|v| v as u32),
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

fn anthropic_user_content_blocks(
    msg: &crate::llm::streaming::ChatMessage,
    request_turn: Option<&AnthropicMultimodalTurn>,
) -> Option<Vec<Value>> {
    let turn = msg.anthropic_multimodal_turn.as_ref().or(request_turn)?;
    if turn.image_blocks.is_empty() {
        return None;
    }

    let mut blocks = Vec::with_capacity(1 + turn.image_blocks.len());
    if !msg.content.is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": msg.content,
        }));
    }
    blocks.extend(turn.image_blocks.iter().map(anthropic_block_to_json));
    Some(blocks)
}

fn anthropic_block_to_json(block: &AnthropicContentBlock) -> Value {
    match block {
        AnthropicContentBlock::Text { text } => json!({
            "type": "text",
            "text": text,
        }),
        AnthropicContentBlock::Image { source } => json!({
            "type": "image",
            "source": source,
        }),
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

    fn supports_prompt_caching(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        let body = self.build_request_body(&request);

        debug!("Claude send request to model: {}", self.model);

        let headers = self.build_request_headers(&request);
        let mut req = self.client.post(&self.api_url);
        for (key, value) in headers {
            req = req.header(key, value);
        }
        let response = req.json(&body).send().await?;

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

        let headers = self.build_request_headers(&stream_request);
        let mut req = self.client.post(&self.api_url);
        for (key, value) in headers {
            req = req.header(key, value);
        }
        let response = req.json(&body).send().await?;

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
                            // Prepend any incomplete UTF-8 bytes from the
                            // previous chunk so multi-byte characters that
                            // span HTTP chunk boundaries decode correctly.
                            let to_decode = if state.incomplete_utf8.is_empty() {
                                bytes.to_vec()
                            } else {
                                let mut combined = std::mem::take(&mut state.incomplete_utf8);
                                combined.extend_from_slice(&bytes);
                                combined
                            };
                            match String::from_utf8(to_decode) {
                                Ok(text) => {
                                    state.buffer.push_str(&text);
                                }
                                Err(e) => {
                                    let valid_up_to = e.utf8_error().valid_up_to();
                                    let raw = e.into_bytes();
                                    // Append the valid prefix to the buffer
                                    // SAFETY: raw[..valid_up_to] is guaranteed valid UTF-8
                                    state.buffer.push_str(
                                        unsafe { std::str::from_utf8_unchecked(&raw[..valid_up_to]) }
                                    );
                                    // Stash trailing incomplete bytes for the next chunk
                                    state.incomplete_utf8 = raw[valid_up_to..].to_vec();
                                }
                            }
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
                                    return Some((stream::iter(events), (byte_stream, state)));
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
            .post(&self.api_url)
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
        // message_start: extract input token count + cache usage
        "message_start" => {
            if let Some(tokens) = parsed["message"]["usage"]["input_tokens"].as_u64() {
                state.input_tokens = tokens as u32;
            }
            if let Some(v) = parsed["message"]["usage"]["cache_creation_input_tokens"].as_u64() {
                state.cache_creation_input_tokens = Some(v as u32);
            }
            if let Some(v) = parsed["message"]["usage"]["cache_read_input_tokens"].as_u64() {
                state.cache_read_input_tokens = Some(v as u32);
            }
            None
        }

        // content_block_start: may begin a tool_use, thinking, or redacted_thinking block
        "content_block_start" => {
            let block = &parsed["content_block"];
            match block["type"].as_str() {
                Some("tool_use") => {
                    state.current_tool_id = block["id"].as_str().map(String::from);
                    state.current_tool_name = block["name"].as_str().map(String::from);
                    state.tool_json_fragments.clear();
                }
                Some("thinking") => {
                    state.in_thinking_block = true;
                    state.thinking_text.clear();
                    state.thinking_signature.clear();
                }
                Some("redacted_thinking") => {
                    // Pass the encrypted block through verbatim so the caller can
                    // echo it back on subsequent turns (Anthropic requires it for
                    // tool-use validation when extended thinking is enabled).
                    // Clone the whole block to preserve any future fields beyond
                    // `type` / `data` that the API may add.
                    return Some(vec![StreamEvent::ThinkingBlock {
                        block: block.clone(),
                    }]);
                }
                _ => {}
            }
            None
        }

        // content_block_delta: text, thinking, signature, or tool input fragments
        "content_block_delta" => {
            let delta = &parsed["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => delta["text"].as_str().map(|text| {
                    vec![StreamEvent::ContentDelta {
                        delta: text.to_string(),
                    }]
                }),
                Some("thinking_delta") => delta["thinking"].as_str().map(|text| {
                    state.thinking_text.push_str(text);
                    vec![StreamEvent::ThinkingDelta {
                        delta: text.to_string(),
                    }]
                }),
                Some("signature_delta") => {
                    if let Some(sig) = delta["signature"].as_str() {
                        state.thinking_signature.push_str(sig);
                    }
                    None
                }
                Some("input_json_delta") => {
                    if let Some(partial) = delta["partial_json"].as_str() {
                        state.tool_json_fragments.push_str(partial);
                    }
                    None
                }
                _ => None,
            }
        }

        // content_block_stop: finalize any pending tool call or thinking block
        "content_block_stop" => {
            if state.in_thinking_block {
                let mut block = serde_json::Map::new();
                block.insert("type".to_string(), Value::String("thinking".to_string()));
                block.insert(
                    "thinking".to_string(),
                    Value::String(std::mem::take(&mut state.thinking_text)),
                );
                if !state.thinking_signature.is_empty() {
                    block.insert(
                        "signature".to_string(),
                        Value::String(std::mem::take(&mut state.thinking_signature)),
                    );
                }
                state.in_thinking_block = false;
                return Some(vec![StreamEvent::ThinkingBlock {
                    block: Value::Object(block),
                }]);
            }
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
            let output_tokens = parsed["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
            // Anthropic also re-emits cache_* fields on message_delta when the
            // final values differ from message_start; prefer the latest.
            if let Some(v) = parsed["usage"]["cache_creation_input_tokens"].as_u64() {
                state.cache_creation_input_tokens = Some(v as u32);
            }
            if let Some(v) = parsed["usage"]["cache_read_input_tokens"].as_u64() {
                state.cache_read_input_tokens = Some(v as u32);
            }
            if let Some(v) = parsed["usage"]["input_tokens"].as_u64() {
                state.input_tokens = v as u32;
            }

            Some(vec![StreamEvent::Done {
                stop_reason,
                usage: TokenUsage {
                    input_tokens: state.input_tokens,
                    output_tokens,
                    cache_creation_input_tokens: state.cache_creation_input_tokens,
                    cache_read_input_tokens: state.cache_read_input_tokens,
                },
            }])
        }

        "error" => {
            let message = parsed["error"]["message"]
                .as_str()
                .unwrap_or("unknown SSE error")
                .to_string();
            let error_type = parsed["error"]["type"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            warn!(
                "SSE error event from provider: type={} message={}",
                error_type, message
            );
            Some(vec![StreamEvent::Error {
                error: format!("{}: {}", error_type, message),
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
                        "name": "WebSearch",
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
        assert_eq!(resp.tool_calls[0].name, "WebSearch");
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
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["max_tokens"], 1024);
        assert!(body.get("tools").is_none());
        assert!(body.get("stream").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn test_build_request_body_omit_model() {
        // Lotus gateway path: the desktop client sends no `model` field so
        // the gateway decides routing on its own (protocol + priority).
        let provider = ClaudeProvider::with_url_opts(
            "test-key".to_string(),
            None,
            "https://example.com/v1/messages".to_string(),
            false,
            true, // omit_model
        );
        let request = LlmRequest {
            messages: vec![ChatMessage::text("user", "Hello")],
            tools: vec![],
            max_tokens: 1024,
            temperature: 1.0,
            stream: false,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
        };

        let body = provider.build_request_body(&request);
        assert!(
            body.get("model").is_none(),
            "expected no `model` field when omit_model=true, got {:?}",
            body
        );
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn test_build_request_body_with_tools_and_stream() {
        let provider = ClaudeProvider::new(
            "key".to_string(),
            Some("claude-opus-4-20250514".to_string()),
        );
        let request = LlmRequest {
            messages: vec![ChatMessage::text("user", "Search")],
            tools: vec![ToolDefinition {
                name: "WebSearch".to_string(),
                description: "Search the web".to_string(),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            }],
            max_tokens: 4096,
            temperature: 0.7,
            stream: true,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
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
        assert_eq!(tools[0]["name"], "WebSearch");
    }

    #[test]
    fn test_build_request_body_with_anthropic_image_blocks() {
        use crate::llm::streaming::{
            AnthropicContentBlock, AnthropicImageSource, AnthropicMultimodalTurn,
        };

        let provider = ClaudeProvider::new("test-key".to_string(), None);
        let request = LlmRequest {
            messages: vec![
                ChatMessage::text("user", "Context message"),
                ChatMessage::text("user", "Describe this image"),
            ],
            tools: vec![],
            max_tokens: 1024,
            temperature: 1.0,
            stream: false,
            thinking_config: None,
            anthropic_multimodal_turn: Some(AnthropicMultimodalTurn {
                image_blocks: vec![AnthropicContentBlock::Image {
                    source: AnthropicImageSource::Base64 {
                        media_type: "image/png".to_string(),
                        data: "iVBORw0KGgo=".to_string(),
                    },
                }],
                image_count: 1,
                image_bytes_total: 8,
                degraded_count: 0,
            }),
            system_segments: None,
        };

        let body = provider.build_request_body(&request);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["content"], "Context message");
        let content = messages[1]["content"].as_array().unwrap();

        assert_eq!(
            content[0],
            json!({"type": "text", "text": "Describe this image"})
        );
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], "iVBORw0KGgo=");
        assert!(!body.to_string().contains("image_url"));
        assert!(!body.to_string().contains("data:image/png;base64"));
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

        let data = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":50}}"#;
        let result = process_sse_data(data, &mut state);
        assert!(result.is_some());
        let events = result.unwrap();
        match &events[0] {
            StreamEvent::Done { stop_reason, usage } => {
                assert_eq!(*stop_reason, StopReason::EndTurn);
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
            }
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_process_sse_data_error_event_emits_stream_error() {
        let mut state = SseState::new();
        let data =
            r#"{"type":"error","error":{"type":"overloaded_error","message":"API overloaded"}}"#;
        let result = process_sse_data(data, &mut state);
        assert!(
            result.is_some(),
            "error event must not be silently discarded"
        );
        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Error { error } => {
                assert!(error.contains("overloaded_error"));
                assert!(error.contains("API overloaded"));
            }
            other => panic!("Expected Error event, got {:?}", other),
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

    /// message_delta 缺失 cache_* 字段时，必须保留 message_start 已读到的值，
    /// 不能用 None 覆盖。
    #[test]
    fn test_message_delta_does_not_clobber_cache_tokens_from_message_start() {
        let mut state = SseState::new();

        // message_start: cache_creation=100, cache_read=200
        let start = json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 50,
                    "cache_creation_input_tokens": 100,
                    "cache_read_input_tokens": 200,
                }
            }
        });
        process_sse_data(&start.to_string(), &mut state);
        assert_eq!(state.cache_creation_input_tokens, Some(100));
        assert_eq!(state.cache_read_input_tokens, Some(200));

        // message_delta WITHOUT cache_* fields — must keep prior values.
        let delta = json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn" },
            "usage": { "output_tokens": 17 }
        });
        let events = process_sse_data(&delta.to_string(), &mut state).expect("Done event");
        match &events[0] {
            StreamEvent::Done { usage, .. } => {
                assert_eq!(usage.cache_creation_input_tokens, Some(100));
                assert_eq!(usage.cache_read_input_tokens, Some(200));
                assert_eq!(usage.output_tokens, 17);
                assert_eq!(usage.input_tokens, 50);
            }
            _ => panic!("Expected Done event"),
        }
    }

    /// usage 字段为 null / 字段不是数字时，as_u64() 应返回 None，
    /// cache token 字段保持 None，不应 panic。
    #[test]
    fn test_cache_tokens_robust_to_missing_or_invalid_usage() {
        // parse_response: usage 字段缺失
        let body = json!({
            "content": [{ "type": "text", "text": "hi" }],
            "stop_reason": "end_turn",
            // no "usage"
        });
        let resp = ClaudeProvider::parse_response(&body).expect("ok");
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.cache_creation_input_tokens, None);
        assert_eq!(resp.usage.cache_read_input_tokens, None);

        // parse_response: usage.cache_* = null / 浮点 / 负数 → None
        let body2 = json!({
            "content": [{ "type": "text", "text": "hi" }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 1,
                "output_tokens": 2,
                "cache_creation_input_tokens": null,
                "cache_read_input_tokens": -1,
            }
        });
        let resp2 = ClaudeProvider::parse_response(&body2).expect("ok");
        assert_eq!(resp2.usage.cache_creation_input_tokens, None);
        // -1 is not a valid u64 → as_u64() returns None
        assert_eq!(resp2.usage.cache_read_input_tokens, None);

        // SSE message_start with non-numeric cache fields
        let mut state = SseState::new();
        let start = json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 5,
                    "cache_creation_input_tokens": "oops",
                    "cache_read_input_tokens": 1.5,
                }
            }
        });
        process_sse_data(&start.to_string(), &mut state);
        assert_eq!(state.cache_creation_input_tokens, None);
        assert_eq!(state.cache_read_input_tokens, None);
        assert_eq!(state.input_tokens, 5);
    }

    /// redacted_thinking 块必须原样透传所有字段（不仅是 type/data），
    /// 防止未来 Anthropic 加新字段时丢失。
    #[test]
    fn test_redacted_thinking_block_preserves_all_fields() {
        let mut state = SseState::new();
        let evt = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "redacted_thinking",
                "data": "ENC...payload",
                "future_field": "should-survive",
            }
        });
        let events = process_sse_data(&evt.to_string(), &mut state).expect("ThinkingBlock");
        match &events[0] {
            StreamEvent::ThinkingBlock { block } => {
                assert_eq!(block["type"], "redacted_thinking");
                assert_eq!(block["data"], "ENC...payload");
                assert_eq!(block["future_field"], "should-survive");
            }
            _ => panic!("Expected ThinkingBlock"),
        }
    }

    #[test]
    fn test_build_request_body_with_system_segments_blocks() {
        // Anthropic-capable model so supports_prompt_caching() == true
        let provider = ClaudeProvider::new(
            "key".to_string(),
            Some("claude-3-5-sonnet-20241022".to_string()),
        );
        let request = LlmRequest {
            messages: vec![
                ChatMessage::text("system", "static\n\ndynamic\n\nvolatile"),
                ChatMessage::text("user", "hello"),
            ],
            tools: vec![],
            max_tokens: 1024,
            temperature: 1.0,
            stream: false,
            thinking_config: None,
            system_segments: Some(vec![
                crate::llm::streaming::SystemPromptSegment {
                    text: "static".to_string(),
                    cache: true,
                },
                crate::llm::streaming::SystemPromptSegment {
                    text: "dynamic".to_string(),
                    cache: true,
                },
                crate::llm::streaming::SystemPromptSegment {
                    text: "volatile".to_string(),
                    cache: false,
                },
            ]),
            anthropic_multimodal_turn: None,
        };

        let body = provider.build_request_body(&request);
        let system_arr = body["system"].as_array().expect("system is array");
        assert_eq!(system_arr.len(), 3);
        assert_eq!(system_arr[0]["text"], "static");
        assert!(system_arr[0].get("cache_control").is_some());
        assert_eq!(system_arr[1]["text"], "dynamic");
        assert!(system_arr[1].get("cache_control").is_some());
        assert_eq!(system_arr[2]["text"], "volatile");
        assert!(system_arr[2].get("cache_control").is_none());
    }

    #[test]
    fn test_build_request_body_segments_cap_warns_beyond_three() {
        let provider = ClaudeProvider::new(
            "key".to_string(),
            Some("claude-3-5-sonnet-20241022".to_string()),
        );
        let segs: Vec<_> = (0..5)
            .map(|i| crate::llm::streaming::SystemPromptSegment {
                text: format!("seg{}", i),
                cache: true,
            })
            .collect();
        let request = LlmRequest {
            messages: vec![ChatMessage::text(
                "system",
                "seg0\n\nseg1\n\nseg2\n\nseg3\n\nseg4",
            )],
            tools: vec![],
            max_tokens: 1024,
            temperature: 1.0,
            stream: false,
            thinking_config: None,
            system_segments: Some(segs),
            anthropic_multimodal_turn: None,
        };

        let body = provider.build_request_body(&request);
        let system_arr = body["system"].as_array().expect("system is array");
        assert_eq!(system_arr.len(), 5);
        // first 3 keep cache_control; last 2 get dropped flags
        let kept: usize = system_arr
            .iter()
            .filter(|b| b.get("cache_control").is_some())
            .count();
        assert_eq!(kept, 3);
    }

    #[test]
    fn test_build_request_body_no_segments_falls_back_to_single_block() {
        let provider = ClaudeProvider::new(
            "key".to_string(),
            Some("claude-3-5-sonnet-20241022".to_string()),
        );
        let request = LlmRequest {
            messages: vec![ChatMessage::text("system", "you are helpful")],
            tools: vec![],
            max_tokens: 1024,
            temperature: 1.0,
            stream: false,
            thinking_config: None,
            system_segments: None,
            anthropic_multimodal_turn: None,
        };

        let body = provider.build_request_body(&request);
        let system_arr = body["system"].as_array().expect("system is array");
        assert_eq!(system_arr.len(), 1);
        assert_eq!(system_arr[0]["text"], "you are helpful");
        assert!(system_arr[0].get("cache_control").is_some());
    }
}
