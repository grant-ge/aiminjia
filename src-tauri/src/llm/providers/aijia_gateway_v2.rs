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
    StreamBox, StreamEvent, TokenUsage,
};

const AIJIA_GATEWAY_V2_RESPONSES_URL: &str = "https://ai-tenant.renlijia.com/aijia/v2/ai/responses";

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
        let body = build_aijia_request_for_route(request, &self.model_type, self.use_tools);
        let response = self
            .client
            .post(AIJIA_GATEWAY_V2_RESPONSES_URL)
            .bearer_auth(&self.session_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("AIjia v2 stream error ({}): {}", status, body));
        }

        Ok(Box::pin(sse_bytes_to_events(response.bytes_stream())))
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
    let is_reasoner = model_type == "reasoner";
    let intent = if is_reasoner { "reasoning" } else { "chat" };
    let logical_model = if is_reasoner {
        "default-reasoner"
    } else {
        "default-chat"
    };
    let tools_enabled = use_tools && !is_reasoner;
    let mut allowed_capabilities = vec!["text".to_string()];
    if tools_enabled {
        allowed_capabilities.push("tool_calling".to_string());
    }

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
        messages.push(to_canonical_message(message));
    }

    let tools = if tools_enabled {
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
        intent: intent.to_string(),
        stream: true,
        model_policy: ModelPolicy {
            mode: "auto".to_string(),
            logical_model: logical_model.to_string(),
            allowed_capabilities,
            reasoning: Some("medium".to_string()),
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
            platform: std::env::consts::ARCH.to_string(),
        },
    }
}

fn to_canonical_message(message: ChatMessage) -> CanonicalMessage {
    let role = if message.role == "tool" {
        "tool_result".to_string()
    } else {
        message.role
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
            content.push(thinking_block_to_content(block));
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

    CanonicalMessage {
        role,
        content,
        tool_call_id: message.tool_call_id,
        tool_name: message.name,
        is_error: false,
        provider: None,
        usage: None,
        stop_reason: None,
        created_at: None,
    }
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
) -> impl futures::Stream<Item = StreamEvent> + Send {
    stream::unfold(
        (Box::pin(byte_stream), String::new(), VecDeque::new()),
        |(mut byte_stream, mut buffer, mut pending)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    return Some((event, (byte_stream, buffer, pending)));
                }

                match byte_stream.as_mut().next().await {
                    Some(Ok(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        drain_sse_frames(&mut buffer, &mut pending);
                    }
                    Some(Err(err)) => {
                        return Some((
                            StreamEvent::Error {
                                error: err.to_string(),
                            },
                            (byte_stream, buffer, pending),
                        ));
                    }
                    None => {
                        if buffer.trim().is_empty() {
                            return None;
                        }
                        let frame = std::mem::take(&mut buffer);
                        return Some((
                            chunk_to_stream_event(&frame),
                            (byte_stream, buffer, pending),
                        ));
                    }
                }
            }
        },
    )
}

fn drain_sse_frames(buffer: &mut String, pending: &mut VecDeque<StreamEvent>) {
    while let Some((idx, len)) = next_sse_frame_boundary(buffer) {
        let frame = buffer[..idx].to_string();
        buffer.drain(..idx + len);
        if !frame.trim().is_empty() {
            pending.push_back(chunk_to_stream_event(&frame));
        }
    }
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
    if frame.contains("event: content.delta") {
        StreamEvent::ContentDelta {
            delta: extract_sse_data_field(frame, "delta").unwrap_or_default(),
        }
    } else if frame.contains("event: thinking.delta") {
        StreamEvent::ThinkingDelta {
            delta: extract_sse_data_field(frame, "delta").unwrap_or_default(),
        }
    } else if frame.contains("event: tool_call.completed") {
        let tool_call = extract_sse_json(frame)
            .and_then(|v| v.get("tool_call").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| crate::llm::streaming::ToolCall {
                id: String::new(),
                name: String::new(),
                arguments: Value::Null,
            });
        StreamEvent::ToolCallStart { tool_call }
    } else if frame.contains("event: response.completed") {
        StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: extract_token_usage(frame).unwrap_or_default(),
        }
    } else if frame.contains("event: response.error") {
        StreamEvent::Error {
            error: extract_sse_data(frame).unwrap_or_else(|| frame.to_string()),
        }
    } else {
        StreamEvent::Keepalive
    }
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
        assert_eq!(canonical.model_policy.allowed_capabilities, vec!["text"]);
        assert!(canonical.tools.is_empty());
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
    fn drains_complete_sse_frames_without_chunk_boundaries() {
        let mut buffer = concat!(
            "event: content.delta\n",
            "data: {\"delta\":\"hi\"}\n\n",
            "event: response.completed\n",
            "data: {\"usage\":{\"input\":1,\"output\":2,\"cache_read\":0,\"cache_write\":0}}\n\n"
        )
        .to_string();
        let mut pending = VecDeque::new();

        drain_sse_frames(&mut buffer, &mut pending);

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
}
