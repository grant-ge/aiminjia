use anyhow::{anyhow, Result};
use futures::{stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::collections::VecDeque;

use crate::llm::canonical::{
    AijiaResponseRequest, CanonicalContext, CanonicalMessage, ClientInfo, ContentBlock,
    GenerationOptions, ModelPolicy, SystemSegment, ToolDefinition,
};
use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{
    AnthropicContentBlock, AnthropicImageSource, ChatMessage, LlmRequest, LlmResponse, StopReason,
    StreamBox, StreamEvent, ThinkingConfig, TokenUsage, ToolCall,
};

/// Path of the v2 responses route. The origin is resolved per-request via
/// [`crate::environment::tenant_host`] so the dev environment switch takes effect
/// (production host in release builds).
const AIJIA_GATEWAY_V2_RESPONSES_PATH: &str = "/aijia/v2/ai/responses";

pub struct AijiaGatewayV2Provider {
    client: Client,
    session_key: String,
    model_type: String,
    use_tools: bool,
}

impl AijiaGatewayV2Provider {
    pub fn new(session_key: String) -> Self {
        Self::with_route(session_key, "chat".to_string(), true)
    }

    pub fn with_route(session_key: String, model_type: String, use_tools: bool) -> Self {
        Self {
            client: super::build_http_client(),
            session_key,
            model_type,
            use_tools,
        }
    }
}

impl LlmProviderTrait for AijiaGatewayV2Provider {
    fn name(&self) -> &str {
        "aijia-v2"
    }

    fn supports_tools(&self) -> bool {
        self.use_tools
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn send(&self, _request: LlmRequest) -> Result<LlmResponse> {
        Err(anyhow!(
            "AIjia v2 non-streaming send is not enabled in desktop MVP"
        ))
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        let url = format!(
            "{}{}",
            crate::environment::tenant_host(),
            AIJIA_GATEWAY_V2_RESPONSES_PATH
        );
        let body = build_aijia_request_for_route(request, &self.model_type, self.use_tools);
        let gate_log_id = crate::llm::gate_log::next_request_id();
        crate::llm::gate_log::record_request(&gate_log_id, self.name(), &url, &body);
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.session_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let gateway_request_id = response
            .headers()
            .get("x-lotus-request-id")
            .or_else(|| response.headers().get("x-request-id"))
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        crate::llm::gate_log::record_response_status(
            &gate_log_id,
            status.as_u16(),
            gateway_request_id.as_deref(),
        );
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            crate::llm::gate_log::record_response_body(&gate_log_id, status.as_u16(), &body);
            return Err(anyhow!("AIjia v2 stream error ({}): {}", status, body));
        }
        crate::llm::gate_log::record_stream_started(&gate_log_id);

        Ok(Box::pin(sse_bytes_to_events(
            response.bytes_stream(),
            Some(gate_log_id),
        )))
    }

    async fn validate_key(&self) -> Result<bool> {
        Ok(!self.session_key.trim().is_empty())
    }
}

pub(crate) fn build_aijia_request(request: LlmRequest) -> AijiaResponseRequest {
    build_aijia_request_for_route(request, "chat", true)
}

pub(crate) fn build_aijia_request_for_route(
    request: LlmRequest,
    model_type: &str,
    use_tools: bool,
) -> AijiaResponseRequest {
    let plan = resolve_v2_model_plan(&request, model_type, use_tools);

    let mut system: Vec<SystemSegment> = request
        .system_segments
        .map(|segments| {
            segments
                .into_iter()
                .map(|segment| SystemSegment {
                    kind: "text".to_string(),
                    text: segment.text,
                    cache: segment.cache.then(|| "ephemeral".to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let mut messages = Vec::new();
    for message in request.messages {
        if message.role == "system" {
            if system.is_empty() && !message.content.is_empty() {
                system.push(SystemSegment {
                    kind: "text".to_string(),
                    text: message.content,
                    cache: None,
                });
            }
            continue;
        }
        if let Some(message) = to_canonical_message(message) {
            messages.push(message);
        }
    }

    let tools = if plan.use_tools {
        request
            .tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect()
    } else {
        Vec::new()
    };

    AijiaResponseRequest {
        schema_version: "aijia.ai.response.v1".to_string(),
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        trace_id: request.trace_id,
        intent: plan.intent,
        stream: true,
        model_policy: ModelPolicy {
            mode: "auto".to_string(),
            logical_model: plan.logical_model,
            allowed_capabilities: plan.allowed_capabilities,
            reasoning: Some(plan.reasoning),
            provider_affinity: Some("conversation".to_string()),
        },
        context: CanonicalContext { system, messages },
        tools,
        options: GenerationOptions {
            max_output_tokens: request.max_tokens,
            temperature: request.temperature,
        },
        client: ClientInfo {
            name: "aijia-desktop".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: client_platform(),
        },
    }
}

fn client_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct V2ModelPlan {
    intent: String,
    logical_model: String,
    allowed_capabilities: Vec<String>,
    reasoning: String,
    use_tools: bool,
}

fn resolve_v2_model_plan(request: &LlmRequest, model_type: &str, use_tools: bool) -> V2ModelPlan {
    let is_reasoner = model_type == "reasoner";
    let intent = if is_reasoner {
        "reasoning".to_string()
    } else {
        "chat".to_string()
    };
    let logical_model = if is_reasoner {
        "default-reasoner".to_string()
    } else {
        "default-chat".to_string()
    };
    let requires_opaque_replay = request_requires_opaque_replay(request);
    let has_image_input = request_has_image_input(request);
    let tools_enabled = use_tools && !is_reasoner && !request.tools.is_empty();
    let reasoning = v2_reasoning_level(
        request.thinking_config.as_ref(),
        is_reasoner,
        requires_opaque_replay,
    );

    let mut allowed_capabilities = vec!["text".to_string()];
    if tools_enabled {
        allowed_capabilities.push("tool_calling".to_string());
    }
    if reasoning != "off" {
        allowed_capabilities.push("reasoning".to_string());
    }
    if has_image_input {
        allowed_capabilities.push("image_input".to_string());
    }
    if requires_opaque_replay {
        allowed_capabilities.push("opaque_state_replay".to_string());
        if !allowed_capabilities.iter().any(|cap| cap == "reasoning") {
            allowed_capabilities.push("reasoning".to_string());
        }
    }

    V2ModelPlan {
        intent,
        logical_model,
        allowed_capabilities,
        reasoning,
        use_tools: tools_enabled,
    }
}

fn v2_reasoning_level(
    thinking_config: Option<&ThinkingConfig>,
    is_reasoner: bool,
    requires_opaque_replay: bool,
) -> String {
    if is_reasoner {
        return "high".to_string();
    }
    if requires_opaque_replay {
        return "medium".to_string();
    }
    match thinking_config {
        Some(ThinkingConfig::Adaptive) => "medium".to_string(),
        Some(ThinkingConfig::Enabled { budget_tokens }) => {
            reasoning_level_for_budget(*budget_tokens).to_string()
        }
        Some(ThinkingConfig::Disabled) | None => "off".to_string(),
    }
}

fn reasoning_level_for_budget(budget_tokens: u32) -> &'static str {
    match budget_tokens {
        0..=1023 => "minimal",
        1024..=2047 => "minimal",
        2048..=4095 => "low",
        4096..=8191 => "medium",
        8192..=16383 => "high",
        _ => "xhigh",
    }
}

fn request_requires_opaque_replay(request: &LlmRequest) -> bool {
    request.messages.iter().any(|message| {
        message
            .thinking_blocks
            .as_ref()
            .is_some_and(|blocks| !blocks.is_empty())
    })
}

fn request_has_image_input(request: &LlmRequest) -> bool {
    request
        .anthropic_multimodal_turn
        .as_ref()
        .is_some_and(|turn| !turn.image_blocks.is_empty())
        || request.messages.iter().any(|message| {
            message
                .anthropic_multimodal_turn
                .as_ref()
                .is_some_and(|turn| !turn.image_blocks.is_empty())
        })
}

fn to_canonical_message(message: ChatMessage) -> Option<CanonicalMessage> {
    if message.role == "tool" {
        let valid_id = message
            .tool_call_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let valid_name = message
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if valid_id.is_none() || valid_name.is_none() {
            log::warn!(
                "[aijia-v2] dropping invalid tool_result message: tool_call_id={:?} name={:?}",
                message.tool_call_id,
                message.name
            );
            return None;
        }
    }

    let role = if message.role == "tool" {
        "tool_result".to_string()
    } else {
        message.role.clone()
    };

    let mut content = Vec::new();
    if !message.content.is_empty() {
        content.push(ContentBlock {
            kind: "text".to_string(),
            text: Some(message.content),
            mime_type: None,
            data: None,
            url: None,
            id: None,
            name: None,
            arguments: None,
            signature: None,
            opaque: None,
            source: None,
        });
    }
    if let Some(thinking) = message.thinking {
        content.push(ContentBlock {
            kind: "thinking".to_string(),
            text: Some(thinking),
            mime_type: None,
            data: None,
            url: None,
            id: None,
            name: None,
            arguments: None,
            signature: None,
            opaque: None,
            source: None,
        });
    }
    if let Some(thinking_blocks) = message.thinking_blocks {
        for block in thinking_blocks {
            if block.is_object() {
                content.push(thinking_block_to_content(block));
            } else {
                log::warn!("[aijia-v2] dropping invalid non-object thinking block");
            }
        }
    }
    if let Some(turn) = message.anthropic_multimodal_turn {
        for block in turn.image_blocks {
            match block {
                AnthropicContentBlock::Text { text } => {
                    content.push(ContentBlock {
                        kind: "text".to_string(),
                        text: Some(text),
                        mime_type: None,
                        data: None,
                        url: None,
                        id: None,
                        name: None,
                        arguments: None,
                        signature: None,
                        opaque: None,
                        source: None,
                    });
                }
                AnthropicContentBlock::Image {
                    source: AnthropicImageSource::Base64 { media_type, data },
                } => {
                    content.push(ContentBlock {
                        kind: "image".to_string(),
                        text: None,
                        mime_type: Some(media_type),
                        data: Some(data),
                        url: None,
                        id: None,
                        name: None,
                        arguments: None,
                        signature: None,
                        opaque: None,
                        source: None,
                    });
                }
            }
        }
    }
    if let Some(tool_calls) = message.tool_calls {
        for tool_call in tool_calls {
            let tool_call = match tool_call.into_valid() {
                Ok(tool_call) => tool_call,
                Err(err) => {
                    log::warn!("[aijia-v2] dropping invalid assistant tool_call: {err}");
                    continue;
                }
            };
            content.push(ContentBlock {
                kind: "tool_call".to_string(),
                text: None,
                mime_type: None,
                data: None,
                url: None,
                id: Some(tool_call.id),
                name: Some(tool_call.name),
                arguments: Some(tool_call.arguments),
                signature: None,
                opaque: None,
                source: None,
            });
        }
    }
    if content.is_empty() {
        content.push(ContentBlock {
            kind: "text".to_string(),
            text: Some(String::new()),
            mime_type: None,
            data: None,
            url: None,
            id: None,
            name: None,
            arguments: None,
            signature: None,
            opaque: None,
            source: None,
        });
    }

    Some(CanonicalMessage {
        role,
        content,
        tool_call_id: message.tool_call_id.map(|id| id.trim().to_string()),
        tool_name: message.name.map(|name| name.trim().to_string()),
        is_error: message.is_error,
        provider: None,
        usage: None,
        stop_reason: None,
        created_at: None,
    })
}

fn thinking_block_to_content(block: Value) -> ContentBlock {
    let text = block
        .get("thinking")
        .or_else(|| block.get("text"))
        .or_else(|| block.get("data"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let signature = block
        .get("signature")
        .and_then(Value::as_str)
        .map(str::to_string);

    ContentBlock {
        kind: "thinking".to_string(),
        text,
        mime_type: None,
        data: None,
        url: None,
        id: None,
        name: None,
        arguments: None,
        signature,
        opaque: Some(true),
        source: Some(block),
    }
}

fn sse_bytes_to_events(
    byte_stream: impl futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>>
        + Send
        + 'static,
    gate_log_id: Option<String>,
) -> impl futures::Stream<Item = StreamEvent> + Send {
    let lifecycle = GatewayStreamLifecycle::new(gate_log_id);
    stream::unfold(
        (
            Box::pin(byte_stream),
            String::new(),
            VecDeque::new(),
            lifecycle,
        ),
        |(mut byte_stream, mut buffer, mut pending, mut lifecycle)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    return Some((event, (byte_stream, buffer, pending, lifecycle)));
                }

                match byte_stream.as_mut().next().await {
                    Some(Ok(bytes)) => {
                        if let Some(request_id) = lifecycle.request_id() {
                            crate::llm::gate_log::record_response_chunk(request_id, &bytes);
                        }
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        drain_sse_frames(&mut buffer, &mut pending, &mut lifecycle);
                    }
                    Some(Err(err)) => {
                        lifecycle.close_error(&err.to_string());
                        return Some((
                            StreamEvent::Error {
                                error: err.to_string(),
                            },
                            (byte_stream, buffer, pending, lifecycle),
                        ));
                    }
                    None => {
                        if buffer.trim().is_empty() {
                            if lifecycle.closed {
                                return None;
                            }
                            if let Some(request_id) = lifecycle.request_id() {
                                crate::llm::gate_log::record_stream_end(request_id);
                            }
                            let completed = lifecycle.response_completed;
                            lifecycle.close_eof();
                            if completed {
                                return None;
                            }
                            return Some((
                                StreamEvent::Error {
                                    error: "AIjia v2 stream ended without response.completed"
                                        .to_string(),
                                },
                                (byte_stream, buffer, pending, lifecycle),
                            ));
                        }
                        let frame = std::mem::take(&mut buffer);
                        record_gateway_route_from_frame(&frame, lifecycle.request_id());
                        lifecycle.record_frame(&frame);
                        return Some((
                            chunk_to_stream_event(&frame),
                            (byte_stream, buffer, pending, lifecycle),
                        ));
                    }
                }
            }
        },
    )
}

struct GatewayStreamLifecycle {
    request_id: Option<String>,
    first_event_recorded: bool,
    response_completed: bool,
    closed: bool,
}

impl GatewayStreamLifecycle {
    fn new(request_id: Option<String>) -> Self {
        Self {
            request_id,
            first_event_recorded: false,
            response_completed: false,
            closed: false,
        }
    }

    fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    fn record_frame(&mut self, frame: &str) {
        if let Some(request_id) = self.request_id() {
            if !self.first_event_recorded {
                let event_name = sse_event_name(frame);
                crate::llm::gate_log::record_first_event(request_id, event_name.as_deref());
            }
            if !self.response_completed && frame_has_event(frame, "response.completed") {
                let stop_reason = response_completed_stop_reason(frame);
                crate::llm::gate_log::record_response_completed(request_id, stop_reason.as_deref());
            }
        }
        self.first_event_recorded = true;
        if frame_has_event(frame, "response.completed") {
            self.response_completed = true;
        }
    }

    fn close_eof(&mut self) {
        let reason = if self.response_completed {
            "response_completed"
        } else {
            "eof"
        };
        self.close(reason, None);
    }

    fn close_error(&mut self, error: &str) {
        if let Some(request_id) = self.request_id() {
            crate::llm::gate_log::record_stream_error(request_id, error);
        }
        self.close("error", Some(error));
    }

    fn close_dropped(&mut self) {
        let reason = if self.response_completed {
            "response_completed"
        } else {
            "dropped"
        };
        self.close(reason, None);
    }

    fn close(&mut self, reason: &str, error: Option<&str>) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(request_id) = self.request_id() {
            crate::llm::gate_log::record_stream_closed(request_id, reason, error);
        }
    }
}

impl Drop for GatewayStreamLifecycle {
    fn drop(&mut self) {
        self.close_dropped();
    }
}

fn drain_sse_frames(
    buffer: &mut String,
    pending: &mut VecDeque<StreamEvent>,
    lifecycle: &mut GatewayStreamLifecycle,
) {
    while let Some((idx, len)) = next_sse_frame_boundary(buffer) {
        let frame = buffer[..idx].to_string();
        buffer.drain(..idx + len);
        if !frame.trim().is_empty() {
            record_gateway_route_from_frame(&frame, lifecycle.request_id());
            lifecycle.record_frame(&frame);
            pending.push_back(chunk_to_stream_event(&frame));
        }
    }
}

fn record_gateway_route_from_frame(frame: &str, gate_log_id: Option<&str>) {
    if !frame_has_event(frame, "response.started") {
        return;
    }
    let Some(request_id) = gate_log_id else {
        return;
    };
    let Some(data) = extract_sse_json(frame) else {
        return;
    };
    let Some(route) = data.get("route") else {
        return;
    };
    crate::llm::gate_log::record_route(
        request_id,
        data.get("response_id").and_then(Value::as_str),
        route,
    );
}

fn next_sse_frame_boundary(buffer: &str) -> Option<(usize, usize)> {
    let lf = buffer.find("\n\n").map(|idx| (idx, 2));
    let crlf = buffer.find("\r\n\r\n").map(|idx| (idx, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn chunk_to_stream_event(frame: &str) -> StreamEvent {
    if frame_has_event(frame, "content.delta") {
        StreamEvent::ContentDelta {
            delta: extract_sse_data_field(frame, "delta").unwrap_or_default(),
        }
    } else if frame_has_event(frame, "thinking.delta") {
        StreamEvent::ThinkingDelta {
            delta: extract_sse_data_field(frame, "delta").unwrap_or_default(),
        }
    } else if frame_has_event(frame, "thinking.block") {
        match extract_sse_json(frame).and_then(|v| v.get("block").cloned()) {
            Some(block) if block.is_object() => StreamEvent::ThinkingBlock { block },
            _ => StreamEvent::Keepalive,
        }
    } else if frame_has_event(frame, "tool_call.completed") {
        let Some(mut raw_tool_call) =
            extract_sse_json(frame).and_then(|v| v.get("tool_call").cloned())
        else {
            return StreamEvent::Error {
                error: "malformed tool_call.completed: missing tool_call".to_string(),
            };
        };
        if let Some(tool_call) = raw_tool_call.as_object_mut() {
            tool_call
                .entry("arguments".to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
        }
        let tool_call = match serde_json::from_value::<ToolCall>(raw_tool_call) {
            Ok(tool_call) => tool_call,
            Err(err) => {
                return StreamEvent::Error {
                    error: format!("malformed tool_call.completed: {err}"),
                };
            }
        };
        match tool_call.into_valid() {
            Ok(tool_call) => StreamEvent::ToolCallStart { tool_call },
            Err(err) => StreamEvent::Error {
                error: format!("malformed tool_call.completed: {err}"),
            },
        }
    } else if frame_has_event(frame, "response.completed") {
        StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: extract_token_usage(frame).unwrap_or_default(),
        }
    } else if frame_has_event(frame, "response.error") {
        StreamEvent::Error {
            error: extract_sse_data(frame).unwrap_or_else(|| frame.to_string()),
        }
    } else {
        StreamEvent::Keepalive
    }
}

fn frame_has_event(frame: &str, event_name: &str) -> bool {
    sse_event_name(frame).as_deref() == Some(event_name)
}

fn sse_event_name(frame: &str) -> Option<String> {
    frame.lines().find_map(|line| {
        line.trim_end_matches('\r')
            .strip_prefix("event:")
            .map(str::trim)
            .filter(|event| !event.is_empty())
            .map(str::to_string)
    })
}

fn extract_sse_data(chunk: &str) -> Option<String> {
    chunk.lines().find_map(|line| {
        line.trim()
            .strip_prefix("data: ")
            .map(std::string::ToString::to_string)
    })
}

fn extract_sse_json(chunk: &str) -> Option<Value> {
    extract_sse_data(chunk).and_then(|data| serde_json::from_str(&data).ok())
}

fn extract_sse_data_field(chunk: &str, field: &str) -> Option<String> {
    extract_sse_json(chunk)?
        .get(field)?
        .as_str()
        .map(str::to_string)
}

fn response_completed_stop_reason(chunk: &str) -> Option<String> {
    let data = extract_sse_json(chunk)?;
    data.get("stop_reason")
        .or_else(|| data.get("stopReason"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn extract_token_usage(chunk: &str) -> Option<TokenUsage> {
    let data = extract_sse_json(chunk)?;
    let usage = data.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("input")?.as_u64()? as u32,
        output_tokens: usage.get("output")?.as_u64()? as u32,
        cache_read_input_tokens: usage
            .get("cache_read")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
        cache_creation_input_tokens: usage
            .get("cache_write")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::llm::streaming::{ChatMessage, LlmRequest, SystemPromptSegment, ToolCall};

    #[test]
    fn build_request_populates_basic_client_metadata() {
        let req = LlmRequest {
            messages: vec![ChatMessage::text("user", "hello")],
            tools: vec![],
            max_tokens: 1000,
            temperature: 0.7,
            stream: true,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
            conversation_id: Some("conv".to_string()),
            trace_id: Some("trace".to_string()),
            run_id: Some("run".to_string()),
        };

        let canonical = build_aijia_request(req);

        let client = serde_json::to_value(&canonical.client).expect("serialize client metadata");
        assert!(client.get("os").is_none());
        assert!(client.get("arch").is_none());
        assert_eq!(canonical.client.name, "aijia-desktop");
        assert_eq!(canonical.client.platform, client_platform());
        assert!(canonical.client.platform.contains('-'));
    }

    #[test]
    fn build_request_promotes_system_messages_and_excludes_them_from_messages() {
        let req = LlmRequest {
            messages: vec![
                ChatMessage::text("system", "system prompt"),
                ChatMessage::text("user", "hello"),
            ],
            ..Default::default()
        };

        let canonical = build_aijia_request(req);

        assert_eq!(canonical.context.system.len(), 1);
        assert_eq!(canonical.context.system[0].text, "system prompt");
        assert_eq!(canonical.context.messages.len(), 1);
        assert_eq!(canonical.context.messages[0].role, "user");
    }

    #[test]
    fn build_request_prefers_system_segments_over_system_messages() {
        let req = LlmRequest {
            messages: vec![
                ChatMessage::text("system", "flattened system"),
                ChatMessage::text("user", "hello"),
            ],
            system_segments: Some(vec![SystemPromptSegment {
                text: "segmented system".to_string(),
                cache: true,
            }]),
            ..Default::default()
        };

        let canonical = build_aijia_request(req);

        assert_eq!(canonical.context.system.len(), 1);
        assert_eq!(canonical.context.system[0].text, "segmented system");
        assert_eq!(
            canonical.context.system[0].cache.as_deref(),
            Some("ephemeral")
        );
        assert_eq!(canonical.context.messages.len(), 1);
        assert_eq!(canonical.context.messages[0].role, "user");
    }

    #[test]
    fn build_request_preserves_assistant_tool_calls_and_tool_results() {
        let tool_call = ToolCall {
            id: "call_1".to_string(),
            name: "lookup".to_string(),
            arguments: json!({"query":"hi"}),
        };
        let req = LlmRequest {
            messages: vec![
                ChatMessage::assistant_with_tool_calls(
                    "checking".to_string(),
                    vec![tool_call],
                    None,
                    None,
                ),
                ChatMessage::tool_result("call_1", "lookup", "result".to_string()),
            ],
            ..Default::default()
        };

        let canonical = build_aijia_request(req);

        let assistant = &canonical.context.messages[0];
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content.len(), 2);
        assert_eq!(assistant.content[0].kind, "text");
        assert_eq!(assistant.content[1].kind, "tool_call");
        assert_eq!(assistant.content[1].id.as_deref(), Some("call_1"));
        assert_eq!(assistant.content[1].name.as_deref(), Some("lookup"));
        assert_eq!(
            assistant.content[1].arguments.as_ref(),
            Some(&json!({"query":"hi"}))
        );

        let tool_result = &canonical.context.messages[1];
        assert_eq!(tool_result.role, "tool_result");
        assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_result.tool_name.as_deref(), Some("lookup"));
    }

    #[test]
    fn build_request_preserves_tool_result_error_status() {
        let req = LlmRequest {
            messages: vec![ChatMessage::tool_result_with_status(
                "call_1",
                "Bash",
                "permission denied".to_string(),
                true,
            )],
            tools: vec![],
            max_tokens: 1000,
            temperature: 0.7,
            stream: true,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
            conversation_id: Some("conv".to_string()),
            trace_id: Some("trace".to_string()),
            run_id: Some("run".to_string()),
        };

        let canonical = build_aijia_request(req);
        let tool_result = &canonical.context.messages[0];

        assert_eq!(tool_result.role, "tool_result");
        assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_result.tool_name.as_deref(), Some("Bash"));
        assert!(tool_result.is_error);
    }

    #[test]
    fn build_request_uses_reasoner_route_without_tools() {
        let req = LlmRequest {
            messages: vec![ChatMessage::text("user", "think")],
            tools: vec![crate::llm::streaming::ToolDefinition {
                name: "lookup".to_string(),
                description: "Lookup".to_string(),
                parameters: json!({"type":"object"}),
            }],
            ..Default::default()
        };

        let canonical = build_aijia_request_for_route(req, "reasoner", true);

        assert_eq!(canonical.intent, "reasoning");
        assert_eq!(canonical.model_policy.logical_model, "default-reasoner");
        assert_eq!(
            canonical.model_policy.allowed_capabilities,
            vec!["text", "reasoning"]
        );
        assert_eq!(canonical.model_policy.reasoning.as_deref(), Some("high"));
        assert!(canonical.tools.is_empty());
    }

    #[test]
    fn build_request_defaults_chat_reasoning_off_without_tool_capability_when_no_tools() {
        let canonical = build_aijia_request_for_route(
            LlmRequest {
                messages: vec![ChatMessage::text("user", "hello")],
                ..Default::default()
            },
            "chat",
            true,
        );

        assert_eq!(canonical.intent, "chat");
        assert_eq!(canonical.model_policy.logical_model, "default-chat");
        assert_eq!(canonical.model_policy.reasoning.as_deref(), Some("off"));
        assert_eq!(canonical.model_policy.allowed_capabilities, vec!["text"]);
        assert!(canonical.tools.is_empty());
    }

    #[test]
    fn build_request_maps_v2_thinking_budget_to_reasoning_level() {
        let canonical = build_aijia_request_for_route(
            LlmRequest {
                messages: vec![ChatMessage::text("user", "think carefully")],
                thinking_config: Some(crate::llm::streaming::ThinkingConfig::Enabled {
                    budget_tokens: 8192,
                }),
                ..Default::default()
            },
            "chat",
            true,
        );

        assert_eq!(canonical.model_policy.reasoning.as_deref(), Some("high"));
        assert_eq!(
            canonical.model_policy.allowed_capabilities,
            vec!["text", "reasoning"]
        );
    }

    #[test]
    fn build_request_declares_tool_capability_only_when_tools_are_sent() {
        let canonical = build_aijia_request_for_route(
            LlmRequest {
                messages: vec![ChatMessage::text("user", "call a tool")],
                tools: vec![crate::llm::streaming::ToolDefinition {
                    name: "lookup".to_string(),
                    description: "Lookup".to_string(),
                    parameters: json!({"type":"object"}),
                }],
                ..Default::default()
            },
            "chat",
            true,
        );

        assert_eq!(
            canonical.model_policy.allowed_capabilities,
            vec!["text", "tool_calling"]
        );
        assert_eq!(canonical.tools.len(), 1);
    }

    #[test]
    fn build_request_preserves_thinking_and_multimodal_blocks() {
        let mut message = ChatMessage::text("assistant", "answer");
        message.thinking = Some("private chain".to_string());
        message.thinking_blocks = Some(vec![json!({
            "type": "thinking",
            "thinking": "signed thought",
            "signature": "sig_1"
        })]);
        message.anthropic_multimodal_turn = Some(crate::llm::streaming::AnthropicMultimodalTurn {
            image_blocks: vec![AnthropicContentBlock::Image {
                source: AnthropicImageSource::Base64 {
                    media_type: "image/png".to_string(),
                    data: "aGVsbG8=".to_string(),
                },
            }],
            image_count: 1,
            image_bytes_total: 5,
            degraded_count: 0,
        });

        let canonical = build_aijia_request(LlmRequest {
            messages: vec![message],
            ..Default::default()
        });

        let blocks = &canonical.context.messages[0].content;
        assert!(blocks.iter().any(
            |block| block.kind == "thinking" && block.text.as_deref() == Some("private chain")
        ));
        assert!(blocks.iter().any(|block| block.kind == "thinking"
            && block.signature.as_deref() == Some("sig_1")
            && block.opaque == Some(true)));
        assert!(blocks.iter().any(|block| block.kind == "image"
            && block.mime_type.as_deref() == Some("image/png")
            && block.data.as_deref() == Some("aGVsbG8=")));
        assert!(canonical
            .model_policy
            .allowed_capabilities
            .contains(&"image_input".to_string()));
        assert!(canonical
            .model_policy
            .allowed_capabilities
            .contains(&"opaque_state_replay".to_string()));
    }

    #[test]
    fn maps_response_error_chunks_to_stream_errors() {
        let event = chunk_to_stream_event(
            "event: response.error\ndata: {\"code\":\"provider_stream_error\",\"message\":\"bad\"}\n\n",
        );

        match event {
            StreamEvent::Error { error } => {
                assert!(error.contains("provider_stream_error"));
            }
            other => panic!("expected error event, got {other:?}"),
        }
    }

    #[test]
    fn maps_thinking_block_chunks_to_stream_events() {
        let event = chunk_to_stream_event(
            "event: thinking.block\ndata: {\"index\":0,\"block\":{\"type\":\"thinking\",\"thinking\":\"hidden\",\"signature\":\"sig-1\",\"opaque\":true}}\n\n",
        );

        match event {
            StreamEvent::ThinkingBlock { block } => {
                assert_eq!(block["type"], "thinking");
                assert_eq!(block["thinking"], "hidden");
                assert_eq!(block["signature"], "sig-1");
                assert_eq!(block["opaque"], true);
            }
            other => panic!("expected thinking block event, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_completed_without_arguments_defaults_to_empty_object() {
        let event = chunk_to_stream_event(
            "event: tool_call.completed\ndata: {\"index\":0,\"tool_call\":{\"id\":\"toolu_refresh\",\"name\":\"RefreshSkills\"}}\n\n",
        );

        match event {
            StreamEvent::ToolCallStart { tool_call } => {
                assert_eq!(tool_call.id, "toolu_refresh");
                assert_eq!(tool_call.name, "RefreshSkills");
                assert_eq!(tool_call.arguments, json!({}));
            }
            other => panic!("expected tool call event, got {other:?}"),
        }
    }

    #[test]
    fn malformed_tool_call_completed_maps_to_stream_error() {
        let event = chunk_to_stream_event(
            "event: tool_call.completed\ndata: {\"index\":0,\"tool_call\":{\"id\":\"\",\"name\":\"\",\"arguments\":null}}\n\n",
        );

        match event {
            StreamEvent::Error { error } => {
                assert!(error.contains("malformed tool_call.completed"));
                assert!(error.contains("id"));
            }
            other => panic!("expected stream error, got {other:?}"),
        }
    }

    #[test]
    fn build_request_drops_invalid_tool_call_and_tool_result_blocks() {
        let req = LlmRequest {
            messages: vec![
                ChatMessage::assistant_with_tool_calls(
                    "checking".to_string(),
                    vec![ToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: Value::Null,
                    }],
                    None,
                    None,
                ),
                ChatMessage::tool_result("", "", "bad result".to_string()),
            ],
            ..Default::default()
        };

        let canonical = build_aijia_request(req);

        assert_eq!(canonical.context.messages.len(), 1);
        let assistant = &canonical.context.messages[0];
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content.len(), 1);
        assert_eq!(assistant.content[0].kind, "text");
        assert!(!assistant
            .content
            .iter()
            .any(|block| block.kind == "tool_call"));
    }

    #[test]
    fn drains_complete_sse_frames_without_chunk_boundaries() {
        let mut buffer = concat!(
            "event: content.delta\n",
            "data: {\"delta\":\"hi\"}\n\n",
            "event: response.completed\n",
            "data: {\"usage\":{\"input\":1,\"output\":2,\"cache_read\":0,\"cache_write\":0}}\n\n"
        )
        .to_string();
        let mut pending = VecDeque::new();
        let mut lifecycle = GatewayStreamLifecycle::new(None);

        drain_sse_frames(&mut buffer, &mut pending, &mut lifecycle);

        assert!(buffer.is_empty());
        match pending.pop_front() {
            Some(StreamEvent::ContentDelta { delta }) => assert_eq!(delta, "hi"),
            other => panic!("expected content delta, got {other:?}"),
        }
        match pending.pop_front() {
            Some(StreamEvent::Done { usage, .. }) => {
                assert_eq!(usage.input_tokens, 1);
                assert_eq!(usage.output_tokens, 2);
            }
            other => panic!("expected done, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn eof_without_response_completed_maps_to_stream_error() {
        let chunks = stream::iter(vec![Ok(bytes::Bytes::from_static(
            b"event: content.delta\ndata: {\"delta\":\"partial\"}\n\n",
        ))]);
        let mut events = Box::pin(sse_bytes_to_events(chunks, None));

        match events.next().await {
            Some(StreamEvent::ContentDelta { delta }) => assert_eq!(delta, "partial"),
            other => panic!("expected content delta, got {other:?}"),
        }
        match events.next().await {
            Some(StreamEvent::Error { error }) => {
                assert!(error.contains("without response.completed"))
            }
            other => panic!("expected terminal error for incomplete stream, got {other:?}"),
        }
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn eof_after_response_completed_ends_cleanly() {
        let chunks = stream::iter(vec![Ok(bytes::Bytes::from_static(
            b"event: response.completed\ndata: {\"usage\":{\"input\":1,\"output\":2,\"cache_read\":0,\"cache_write\":0}}\n\n",
        ))]);
        let mut events = Box::pin(sse_bytes_to_events(chunks, None));

        match events.next().await {
            Some(StreamEvent::Done { usage, .. }) => {
                assert_eq!(usage.input_tokens, 1);
                assert_eq!(usage.output_tokens, 2);
            }
            other => panic!("expected done, got {other:?}"),
        }
        assert!(events.next().await.is_none());
    }

    #[test]
    fn frame_lifecycle_detects_response_completed() {
        let frame = concat!(
            "event: response.completed\r\n",
            "data: {\"stop_reason\":\"end_turn\"}\r\n\r\n"
        );

        assert_eq!(sse_event_name(frame).as_deref(), Some("response.completed"));
        assert!(frame_has_event(frame, "response.completed"));
        assert!(!frame_has_event(frame, "response.error"));
        assert_eq!(
            response_completed_stop_reason(frame).as_deref(),
            Some("end_turn")
        );
    }
}
