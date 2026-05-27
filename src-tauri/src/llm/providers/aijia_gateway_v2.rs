use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest::Client;

use crate::llm::canonical::{
    AijiaResponseRequest, CanonicalContext, CanonicalMessage, ClientInfo, ContentBlock,
    GenerationOptions, ModelPolicy, SystemSegment, ToolDefinition,
};
use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{
    ChatMessage, LlmRequest, LlmResponse, StopReason, StreamBox, StreamEvent, TokenUsage,
};

const AIJIA_GATEWAY_V2_RESPONSES_URL: &str = "https://ai-tenant.renlijia.com/aijia/v2/ai/responses";

pub struct AijiaGatewayV2Provider {
    client: Client,
    session_key: String,
}

impl AijiaGatewayV2Provider {
    pub fn new(session_key: String) -> Self {
        Self {
            client: super::build_http_client(),
            session_key,
        }
    }
}

impl LlmProviderTrait for AijiaGatewayV2Provider {
    fn name(&self) -> &str {
        "aijia-v2"
    }

    fn supports_tools(&self) -> bool {
        true
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
        let body = build_aijia_request(request);
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

        let stream = response.bytes_stream().map(|chunk| match chunk {
            Ok(bytes) => chunk_to_stream_event(&String::from_utf8_lossy(&bytes)),
            Err(err) => StreamEvent::Error {
                error: err.to_string(),
            },
        });

        Ok(Box::pin(stream))
    }

    async fn validate_key(&self) -> Result<bool> {
        Ok(!self.session_key.trim().is_empty())
    }
}

pub(crate) fn build_aijia_request(request: LlmRequest) -> AijiaResponseRequest {
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

    AijiaResponseRequest {
        schema_version: "aijia.ai.response.v1".to_string(),
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        trace_id: request.trace_id,
        intent: "chat".to_string(),
        stream: true,
        model_policy: ModelPolicy {
            mode: "auto".to_string(),
            logical_model: "default-chat".to_string(),
            allowed_capabilities: vec!["text".to_string(), "tool_calling".to_string()],
            reasoning: Some("medium".to_string()),
            provider_affinity: Some("conversation".to_string()),
        },
        context: CanonicalContext { system, messages },
        tools: request
            .tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect(),
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
            id: None,
            name: None,
            arguments: None,
        });
    }
    if let Some(tool_calls) = message.tool_calls {
        for tool_call in tool_calls {
            content.push(ContentBlock {
                kind: "tool_call".to_string(),
                text: None,
                id: Some(tool_call.id),
                name: Some(tool_call.name),
                arguments: Some(tool_call.arguments),
            });
        }
    }
    if content.is_empty() {
        content.push(ContentBlock {
            kind: "text".to_string(),
            text: Some(String::new()),
            id: None,
            name: None,
            arguments: None,
        });
    }

    CanonicalMessage {
        role,
        content,
        tool_call_id: message.tool_call_id,
        tool_name: message.name,
        is_error: false,
    }
}

fn chunk_to_stream_event(chunk: &str) -> StreamEvent {
    if chunk.contains("event: response.completed") {
        StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        }
    } else if chunk.contains("event: response.error") {
        StreamEvent::Error {
            error: extract_sse_data(chunk).unwrap_or_else(|| chunk.to_string()),
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
}
