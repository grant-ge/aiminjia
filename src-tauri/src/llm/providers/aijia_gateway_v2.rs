use anyhow::{anyhow, Result};
use futures::{stream, StreamExt};
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
    client: reqwest_middleware::ClientWithMiddleware,
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
            client: crate::tracing_setup::traced_client(super::build_http_client()),
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

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        let body = build_aijia_request_for_route_with_stream(
            request,
            &self.model_type,
            self.use_tools,
            false,
        );
        let url = format!(
            "{}{}",
            crate::environment::tenant_host(),
            AIJIA_GATEWAY_V2_RESPONSES_PATH
        );
        let gate_log_id = crate::llm::gate_log::next_request_id();
        crate::llm::gate_log::record_request(&gate_log_id, self.name(), &url, &body);
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.session_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                crate::llm::gate_log::record_request_error(
                    &gate_log_id,
                    &error_chain_diagnostics(&err),
                );
                err
            })?;

        let status = response.status();
        let gateway_request_id = response
            .headers()
            .get("x-lotus-request-id")
            .or_else(|| response.headers().get("x-request-id"))
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        crate::llm::gate_log::record_response_status(
            &gate_log_id,
            status.as_u16(),
            gateway_request_id.as_deref(),
        );
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            crate::llm::gate_log::record_response_body(&gate_log_id, status.as_u16(), &body);
            return Err(anyhow!("AIjia v2 non-stream error ({}): {}", status, body));
        }

        if content_type.contains("text/event-stream") {
            return collect_stream_response(Box::pin(sse_bytes_to_events(
                response.bytes_stream(),
                Some(gate_log_id),
            )))
            .await;
        }

        let response_body = response.text().await?;
        crate::llm::gate_log::record_response_body(&gate_log_id, status.as_u16(), &response_body);
        let payload: Value = serde_json::from_str(&response_body)
            .map_err(|err| anyhow!("AIjia v2 non-stream response is not JSON: {err}"))?;
        parse_non_stream_response(&payload)
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
            .await
            .map_err(|err| {
                crate::llm::gate_log::record_request_error(
                    &gate_log_id,
                    &error_chain_diagnostics(&err),
                );
                err
            })?;

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

fn error_chain_diagnostics(error: &(dyn std::error::Error + 'static)) -> String {
    let mut parts = vec![error.to_string()];
    append_reqwest_flags(error, &mut parts);

    let mut idx = 1;
    let mut source = error.source();
    while let Some(cause) = source {
        parts.push(format!("caused_by[{idx}]: {cause}"));
        append_reqwest_flags(cause, &mut parts);
        source = cause.source();
        idx += 1;
    }

    parts.join("; ")
}

fn append_reqwest_flags(error: &(dyn std::error::Error + 'static), parts: &mut Vec<String>) {
    if let Some(reqwest_err) = error.downcast_ref::<reqwest::Error>() {
        let status = reqwest_err
            .status()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".to_string());
        let url = reqwest_err
            .url()
            .map(|u| u.as_str().to_string())
            .unwrap_or_else(|| "-".to_string());
        parts.push(format!(
            "reqwest: timeout={} connect={} request={} body={} decode={} status={} url={}",
            reqwest_err.is_timeout(),
            reqwest_err.is_connect(),
            reqwest_err.is_request(),
            reqwest_err.is_body(),
            reqwest_err.is_decode(),
            status,
            url
        ));
    }
}

async fn collect_stream_response(mut stream: StreamBox) -> Result<LlmResponse> {
    let mut content = String::new();
    let mut stop_reason = StopReason::EndTurn;
    let mut usage = TokenUsage::default();
    let mut tool_calls = Vec::new();
    let mut thinking_blocks = Vec::new();

    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ContentDelta { delta } => content.push_str(&delta),
            StreamEvent::ToolCallStart { tool_call } => tool_calls.push(tool_call),
            StreamEvent::ThinkingBlock { block } => thinking_blocks.push(block),
            StreamEvent::Done {
                stop_reason: reason,
                usage: done_usage,
            } => {
                stop_reason = reason;
                usage = done_usage;
                break;
            }
            StreamEvent::Error { error } => return Err(anyhow!(error)),
            StreamEvent::ThinkingDelta { .. }
            | StreamEvent::Keepalive
            | StreamEvent::Notice { .. } => {}
        }
    }

    Ok(LlmResponse {
        content,
        stop_reason,
        usage,
        tool_calls,
        thinking_blocks,
    })
}

fn content_block_texts(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Some(text) = value.as_str() {
        return vec![text.to_string()];
    }
    let Some(blocks) = value.as_array() else {
        return Vec::new();
    };

    blocks
        .iter()
        .filter_map(|block| {
            let block_type = block
                .get("type")
                .or_else(|| block.get("kind"))
                .and_then(Value::as_str);
            let is_text = match block_type {
                Some("text" | "output_text") => true,
                Some("thinking" | "reasoning" | "tool_call" | "tool_use") => false,
                Some(_) => false,
                None => true,
            };
            if !is_text {
                return None;
            }
            block
                .get("text")
                .or_else(|| block.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn response_text_parts(payload: &Value) -> Vec<String> {
    let mut parts = Vec::new();

    parts.extend(content_block_texts(payload.get("content")));
    if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
        parts.push(text.to_string());
    }
    parts.extend(content_block_texts(
        payload
            .get("message")
            .and_then(|message| message.get("content")),
    ));

    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            parts.extend(content_block_texts(item.get("content")));
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
    }

    if let Some(choices) = payload.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let message = choice.get("message");
            if let Some(text) = message
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
            {
                parts.push(text.to_string());
            }
            parts.extend(content_block_texts(
                message.and_then(|message| message.get("content")),
            ));
        }
    }

    parts
}

fn parse_tool_call_value(call: &Value) -> ToolCall {
    ToolCall {
        id: call
            .get("id")
            .or_else(|| call.get("tool_call_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: call
            .get("name")
            .or_else(|| {
                call.get("function")
                    .and_then(|function| function.get("name"))
            })
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        arguments: call
            .get("arguments")
            .or_else(|| call.get("input"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    }
}

fn content_block_tool_calls(value: Option<&Value>) -> Vec<ToolCall> {
    let Some(blocks) = value.and_then(Value::as_array) else {
        return Vec::new();
    };

    blocks
        .iter()
        .filter(|block| {
            matches!(
                block.get("type").and_then(Value::as_str),
                Some("tool_call" | "tool_use")
            )
        })
        .map(parse_tool_call_value)
        .collect()
}

fn response_tool_calls(payload: &Value) -> Vec<ToolCall> {
    let mut calls = payload
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().map(parse_tool_call_value).collect::<Vec<_>>())
        .unwrap_or_default();

    calls.extend(content_block_tool_calls(
        payload
            .get("message")
            .and_then(|message| message.get("content")),
    ));
    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            calls.extend(content_block_tool_calls(item.get("content")));
        }
    }
    calls
}

fn thinking_blocks_from_content(value: Option<&Value>) -> Vec<Value> {
    let Some(blocks) = value.and_then(Value::as_array) else {
        return Vec::new();
    };

    blocks
        .iter()
        .filter(|block| {
            matches!(
                block.get("type").and_then(Value::as_str),
                Some("thinking" | "reasoning")
            )
        })
        .cloned()
        .collect()
}

fn response_thinking_blocks(payload: &Value) -> Vec<Value> {
    let mut blocks = payload
        .get("thinking_blocks")
        .or_else(|| payload.get("thinkingBlocks"))
        .and_then(Value::as_array)
        .map(|blocks| blocks.to_vec())
        .unwrap_or_default();

    blocks.extend(thinking_blocks_from_content(
        payload
            .get("message")
            .and_then(|message| message.get("content")),
    ));
    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            blocks.extend(thinking_blocks_from_content(item.get("content")));
        }
    }
    blocks
}

fn parse_non_stream_response(payload: &Value) -> Result<LlmResponse> {
    let content = response_text_parts(payload)
        .into_iter()
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let stop_reason = parse_aijia_v2_stop_reason(
        payload
            .get("stop_reason")
            .and_then(Value::as_str)
            .unwrap_or("stop"),
    );
    let usage_value = payload.get("usage").unwrap_or(&Value::Null);
    let usage = TokenUsage {
        input_tokens: usage_value
            .get("input")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        output_tokens: usage_value
            .get("output")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        cache_creation_input_tokens: usage_value
            .get("cache_write")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
        cache_read_input_tokens: usage_value
            .get("cache_read")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
    };
    let tool_calls = response_tool_calls(payload);
    let thinking_blocks = response_thinking_blocks(payload);

    Ok(LlmResponse {
        content,
        stop_reason,
        usage,
        tool_calls,
        thinking_blocks,
    })
}

fn parse_aijia_v2_stop_reason(reason: &str) -> StopReason {
    match reason {
        "toolUse" | "tool_use" => StopReason::ToolUse,
        "length" | "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        _ => StopReason::EndTurn,
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
    build_aijia_request_for_route_with_stream(request, model_type, use_tools, true)
}

fn build_aijia_request_for_route_with_stream(
    request: LlmRequest,
    model_type: &str,
    use_tools: bool,
    stream: bool,
) -> AijiaResponseRequest {
    let plan = resolve_v2_model_plan(&request, model_type, use_tools);
    let visible_reply_language = infer_visible_reply_language(&request.messages);

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
    if let Some(segment) = visible_reply_language_system_segment(visible_reply_language) {
        system.push(segment);
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
        stream,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisibleReplyLanguage {
    Chinese,
    English,
}

fn infer_visible_reply_language(messages: &[ChatMessage]) -> Option<VisibleReplyLanguage> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == "user")
        .filter_map(|message| infer_visible_reply_language_from_text(&message.content))
        .next()
}

fn infer_visible_reply_language_from_text(text: &str) -> Option<VisibleReplyLanguage> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("[动态上下文")
        || trimmed.starts_with("<system-reminder>")
        || trimmed.starts_with("# agentsMd")
        || is_link_or_code_only(trimmed)
    {
        return None;
    }

    let cjk_count = trimmed.chars().filter(|ch| is_cjk(*ch)).count();
    if cjk_count > 0 {
        return Some(VisibleReplyLanguage::Chinese);
    }

    let ascii_alpha_count = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .count();
    if ascii_alpha_count >= 3 {
        return Some(VisibleReplyLanguage::English);
    }

    None
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0x20000..=0x2a6df
            | 0x2a700..=0x2b73f
            | 0x2b740..=0x2b81f
            | 0x2b820..=0x2ceaf
    )
}

fn is_link_or_code_only(text: &str) -> bool {
    text.starts_with("```")
        || text.starts_with("`")
        || text.starts_with("http://")
        || text.starts_with("https://")
}

fn visible_reply_language_system_segment(
    language: Option<VisibleReplyLanguage>,
) -> Option<SystemSegment> {
    let text = match language? {
        VisibleReplyLanguage::Chinese => concat!(
            "<system-reminder>\n",
            "Visible Reply Language: Chinese (zh-CN, 中文).\n",
            "The latest natural-language user request in this turn is Chinese. ",
            "All user-visible assistant prose must stay in Chinese, including brief status/narration before tool calls. ",
            "Keep code, file paths, commands, API fields, proper nouns, and requested foreign-language content unchanged.\n",
            "</system-reminder>"
        ),
        VisibleReplyLanguage::English => concat!(
            "<system-reminder>\n",
            "Visible Reply Language: English.\n",
            "The latest natural-language user request in this turn is English. ",
            "All user-visible assistant prose must stay in English, including brief status/narration before tool calls. ",
            "Keep code, file paths, commands, API fields, proper nouns, and requested foreign-language content unchanged.\n",
            "</system-reminder>"
        ),
    };

    Some(SystemSegment {
        kind: "text".to_string(),
        text: text.to_string(),
        cache: None,
    })
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
    let tools_enabled = use_tools && !request.tools.is_empty();
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
        source: extract_provider_source(&block),
    }
}

fn extract_provider_source(block: &Value) -> Option<Value> {
    if let Some(source) = block.get("source") {
        if source.is_object() {
            if let Some(nested_source) = source.get("source") {
                if nested_source.is_object() {
                    if let Some(meta) = provider_meta_from_object(nested_source) {
                        return Some(meta);
                    }
                }
            }
            if let Some(meta) = provider_meta_from_object(source) {
                return Some(meta);
            }
        }
    }

    provider_meta_from_object(block)
}

fn provider_meta_from_object(value: &Value) -> Option<Value> {
    let mut provider_source = serde_json::Map::new();
    for key in ["api", "provider", "model", "response_id", "response_model"] {
        if let Some(field_value) = value.get(key) {
            provider_source.insert(key.to_string(), field_value.clone());
        }
    }
    if provider_source.is_empty() {
        None
    } else {
        Some(Value::Object(provider_source))
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
    } else if frame_has_event(frame, "response.notice") {
        StreamEvent::Notice {
            notice: extract_sse_json(frame).unwrap_or(Value::Null),
        }
    } else if frame_has_event(frame, "response.completed") {
        StreamEvent::Done {
            stop_reason: extract_stop_reason(frame).unwrap_or(StopReason::EndTurn),
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

fn extract_stop_reason(chunk: &str) -> Option<StopReason> {
    let data = extract_sse_json(chunk)?;
    let reason = data
        .get("stop_reason")
        .or_else(|| data.get("stopReason"))?
        .as_str()?;
    Some(match reason {
        "toolUse" | "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "aborted" => StopReason::Aborted,
        _ => StopReason::EndTurn,
    })
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

        assert_eq!(canonical.context.system.len(), 2);
        assert_eq!(canonical.context.system[0].text, "system prompt");
        assert!(canonical.context.system[1]
            .text
            .contains("Visible Reply Language"));
        assert_eq!(canonical.context.messages.len(), 1);
        assert_eq!(canonical.context.messages[0].role, "user");
    }

    #[test]
    fn build_request_with_stream_flag_preserves_non_streaming_mode() {
        let req = LlmRequest {
            messages: vec![ChatMessage::text("user", "hello")],
            ..Default::default()
        };

        let canonical = build_aijia_request_for_route_with_stream(req, "chat", true, false);

        assert!(!canonical.stream);
    }

    #[test]
    fn build_request_keeps_streaming_mode_by_default() {
        let req = LlmRequest {
            messages: vec![ChatMessage::text("user", "hello")],
            ..Default::default()
        };

        let canonical = build_aijia_request(req);

        assert!(canonical.stream);
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

        assert_eq!(canonical.context.system.len(), 2);
        assert_eq!(canonical.context.system[0].text, "segmented system");
        assert_eq!(
            canonical.context.system[0].cache.as_deref(),
            Some("ephemeral")
        );
        assert_eq!(canonical.context.system[1].cache, None);
        assert!(canonical.context.system[1]
            .text
            .contains("Visible Reply Language"));
        assert_eq!(canonical.context.messages.len(), 1);
        assert_eq!(canonical.context.messages[0].role, "user");
    }

    #[test]
    fn build_request_anchors_visible_reply_language_from_latest_user_message() {
        let tool_call = ToolCall {
            id: "call_1".to_string(),
            name: "Bash".to_string(),
            arguments: json!({"command":"ls templates"}),
        };
        let req = LlmRequest {
            messages: vec![
                ChatMessage::text("user", "请用 html-ppt 生成一份产品发布会 PPT。"),
                ChatMessage::assistant_with_tool_calls(
                    "".to_string(),
                    vec![tool_call],
                    Some("Let me inspect the templates.".to_string()),
                    None,
                ),
                ChatMessage::tool_result(
                    "call_1",
                    "Bash",
                    "product-launch\nsingle-page layouts".to_string(),
                ),
            ],
            system_segments: Some(vec![SystemPromptSegment {
                text: "base system".to_string(),
                cache: true,
            }]),
            ..Default::default()
        };

        let canonical = build_aijia_request(req);

        let language_segment = canonical
            .context
            .system
            .iter()
            .find(|segment| segment.text.contains("Visible Reply Language"))
            .expect("visible reply language segment");
        assert_eq!(language_segment.cache, None);
        assert!(language_segment.text.contains("Chinese"));
        assert!(language_segment.text.contains("中文"));
    }

    #[test]
    fn visible_reply_language_uses_latest_real_user_language() {
        let messages = vec![
            ChatMessage::text("user", "先用中文聊一下。"),
            ChatMessage::text("assistant", "好的。"),
            ChatMessage::text("user", "Please summarize this file."),
        ];

        assert_eq!(
            infer_visible_reply_language(&messages),
            Some(VisibleReplyLanguage::English)
        );
    }

    #[test]
    fn visible_reply_language_ignores_user_system_reminders() {
        let messages = vec![
            ChatMessage::text("user", "帮我看一下这个文件。"),
            ChatMessage::text(
                "user",
                "<system-reminder>\nCurrent time is 2026-06-15.\n</system-reminder>",
            ),
        ];

        assert_eq!(
            infer_visible_reply_language(&messages),
            Some(VisibleReplyLanguage::Chinese)
        );
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
    fn build_request_uses_reasoner_route_with_tools_when_available() {
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
            vec!["text", "tool_calling", "reasoning"]
        );
        assert_eq!(canonical.model_policy.reasoning.as_deref(), Some("high"));
        assert_eq!(canonical.tools.len(), 1);
        assert_eq!(canonical.tools[0].name, "lookup");
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
    fn thinking_block_content_keeps_only_provider_source_metadata() {
        let content = thinking_block_to_content(json!({
            "type": "thinking",
            "thinking": "hidden",
            "signature": "sig-1",
            "opaque": true,
            "source": {
                "api": "anthropic-messages",
                "provider": "anthropic",
                "model": "claude-3-7-sonnet",
                "response_id": "resp_123"
            }
        }));

        assert_eq!(content.kind, "thinking");
        assert_eq!(content.signature.as_deref(), Some("sig-1"));
        assert_eq!(content.opaque, Some(true));
        assert_eq!(
            content.source,
            Some(json!({
                "api": "anthropic-messages",
                "provider": "anthropic",
                "model": "claude-3-7-sonnet",
                "response_id": "resp_123"
            }))
        );
    }

    #[test]
    fn thinking_block_content_filters_legacy_noisy_source() {
        let content = thinking_block_to_content(json!({
            "type": "thinking",
            "thinking": "hidden",
            "signature": "sig-1",
            "opaque": true,
            "source": {
                "type": "thinking",
                "thinking": "must-not-leak",
                "signature": "sig-old",
                "source": {
                    "api": "anthropic-messages",
                    "provider": "anthropic",
                    "model": "claude-3-7-sonnet",
                    "response_id": "resp_123"
                }
            }
        }));

        assert_eq!(
            content.source,
            Some(json!({
                "api": "anthropic-messages",
                "provider": "anthropic",
                "model": "claude-3-7-sonnet",
                "response_id": "resp_123"
            }))
        );
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
    fn maps_response_notice_chunks_to_notice_events() {
        let event = chunk_to_stream_event(
            "event: response.notice\ndata: {\"level\":\"info\",\"code\":\"auto_failed_over\",\"message\":\"switched to backup\",\"from_route\":{\"provider\":\"anthropic\"},\"to_route\":{\"provider\":\"openai\"}}\n\n",
        );

        match event {
            StreamEvent::Notice { notice } => {
                assert_eq!(notice["code"], "auto_failed_over");
                assert_eq!(notice["message"], "switched to backup");
                assert_eq!(notice["from_route"]["provider"], "anthropic");
                assert_eq!(notice["to_route"]["provider"], "openai");
            }
            other => panic!("expected notice event, got {other:?}"),
        }
    }

    #[test]
    fn maps_response_completed_stop_reason_from_gateway() {
        let event = chunk_to_stream_event(
            "event: response.completed\ndata: {\"stop_reason\":\"aborted\",\"usage\":{\"input\":1,\"output\":2,\"cache_read\":0,\"cache_write\":0}}\n\n",
        );

        match event {
            StreamEvent::Done { stop_reason, usage } => {
                assert_eq!(stop_reason, StopReason::Aborted);
                assert_eq!(usage.input_tokens, 1);
                assert_eq!(usage.output_tokens, 2);
            }
            other => panic!("expected done event, got {other:?}"),
        }
    }

    #[test]
    fn maps_response_completed_tool_use_stop_reason_from_gateway() {
        let event = chunk_to_stream_event(
            "event: response.completed\ndata: {\"stop_reason\":\"toolUse\",\"usage\":{\"input\":3,\"output\":5,\"cache_read\":0,\"cache_write\":0}}\n\n",
        );

        match event {
            StreamEvent::Done { stop_reason, usage } => {
                assert_eq!(stop_reason, StopReason::ToolUse);
                assert_eq!(usage.input_tokens, 3);
                assert_eq!(usage.output_tokens, 5);
            }
            other => panic!("expected done event, got {other:?}"),
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
    async fn collect_stream_response_concatenates_content_and_usage() {
        let stream: StreamBox = Box::pin(stream::iter(vec![
            StreamEvent::ContentDelta {
                delta: "hello ".to_string(),
            },
            StreamEvent::ThinkingDelta {
                delta: "internal".to_string(),
            },
            StreamEvent::ContentDelta {
                delta: "summary".to_string(),
            },
            StreamEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage {
                    input_tokens: 12,
                    output_tokens: 3,
                    cache_creation_input_tokens: Some(1),
                    cache_read_input_tokens: Some(2),
                },
            },
        ]));

        let response = collect_stream_response(stream).await.unwrap();

        assert_eq!(response.content, "hello summary");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 3);
        assert!(response.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn collect_stream_response_returns_error_events() {
        let stream: StreamBox = Box::pin(stream::iter(vec![StreamEvent::Error {
            error: "provider failed".to_string(),
        }]));

        let err = collect_stream_response(stream).await.unwrap_err();

        assert!(err.to_string().contains("provider failed"));
    }

    #[test]
    fn parse_non_stream_response_preserves_tool_calls_and_thinking_blocks() {
        let payload = serde_json::json!({
            "content": "I will inspect the file.",
            "stop_reason": "toolUse",
            "usage": {
                "input": 17,
                "output": 7,
                "cache_read": 3,
                "cache_write": 2
            },
            "tool_calls": [{
                "type": "tool_call",
                "id": "toolu_1",
                "name": "read_file",
                "arguments": {"path": "README.md"}
            }],
            "thinking_blocks": [{
                "type": "thinking",
                "text": "hidden",
                "signature": "sig-1",
                "opaque": true
            }]
        });

        let response = parse_non_stream_response(&payload).unwrap();

        assert_eq!(response.content, "I will inspect the file.");
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 17);
        assert_eq!(response.usage.output_tokens, 7);
        assert_eq!(response.usage.cache_read_input_tokens, Some(3));
        assert_eq!(response.usage.cache_creation_input_tokens, Some(2));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "toolu_1");
        assert_eq!(response.tool_calls[0].name, "read_file");
        assert_eq!(response.tool_calls[0].arguments["path"], "README.md");
        assert_eq!(response.thinking_blocks.len(), 1);
        assert_eq!(response.thinking_blocks[0]["signature"], "sig-1");
    }

    #[test]
    fn parse_non_stream_response_reads_v2_message_content_blocks() {
        let payload = serde_json::json!({
            "id": "lreq_test",
            "object": "aijia.response",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "thinking",
                        "text": "internal reasoning",
                        "signature": "sig-v2",
                        "opaque": true
                    },
                    {
                        "type": "text",
                        "text": "compact summary text"
                    }
                ]
            },
            "stop_reason": "stop",
            "usage": {
                "input": 101,
                "output": 9,
                "cache_read": 7,
                "cache_write": 0
            }
        });

        let response = parse_non_stream_response(&payload).unwrap();

        assert_eq!(response.content, "compact summary text");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.usage.input_tokens, 101);
        assert_eq!(response.usage.output_tokens, 9);
        assert_eq!(response.usage.cache_read_input_tokens, Some(7));
        assert_eq!(response.thinking_blocks.len(), 1);
        assert_eq!(response.thinking_blocks[0]["signature"], "sig-v2");
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
