use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest::Client;

use crate::llm::canonical::{
    AijiaResponseRequest, CanonicalContext, CanonicalMessage, ClientInfo, ContentBlock,
    GenerationOptions, ModelPolicy, SystemSegment, ToolDefinition,
};
use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{
    LlmRequest, LlmResponse, StopReason, StreamBox, StreamEvent, TokenUsage,
};

const AIJIA_GATEWAY_V2_RESPONSES_URL: &str =
    "https://ai-tenant.renlijia.com/aijia/v2/ai/responses";

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
    let system = request
        .system_segments
        .unwrap_or_default()
        .into_iter()
        .map(|segment| SystemSegment {
            kind: "text".to_string(),
            text: segment.text,
            cache: segment.cache.then(|| "ephemeral".to_string()),
        })
        .collect();

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
        context: CanonicalContext {
            system,
            messages: request
                .messages
                .into_iter()
                .map(|message| CanonicalMessage {
                    role: if message.role == "tool" {
                        "tool_result".to_string()
                    } else {
                        message.role
                    },
                    content: vec![ContentBlock {
                        kind: "text".to_string(),
                        text: Some(message.content),
                        id: None,
                        name: None,
                        arguments: None,
                    }],
                    tool_call_id: message.tool_call_id,
                    tool_name: message.name,
                    is_error: false,
                })
                .collect(),
        },
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

fn chunk_to_stream_event(chunk: &str) -> StreamEvent {
    if chunk.contains("event: response.completed") {
        StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        }
    } else {
        StreamEvent::Keepalive
    }
}
