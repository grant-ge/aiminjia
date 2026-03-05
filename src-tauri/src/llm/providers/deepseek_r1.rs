//! DeepSeek R1 provider — deep reasoning, no Tool Use.
//!
//! Endpoint: `https://api.deepseek.com/chat/completions`
//! Uses the OpenAI-compatible chat completions format.
//! Model: `deepseek-reasoner`
//!
//! Key differences from V3:
//! - Does NOT support tool use (`supports_tools()` returns false).
//! - Responses include a `reasoning_content` field with chain-of-thought.
//! - In streaming mode, chunks may carry `choices[0].delta.reasoning_content`
//!   which is emitted as `StreamEvent::ThinkingDelta`.

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

use super::openai::{send_openai_compat, stream_openai_compat, validate_key_openai_compat};

const API_URL: &str = "https://api.deepseek.com/chat/completions";
const MODEL: &str = "deepseek-reasoner";

/// DeepSeek R1 provider (reasoning model).
pub struct DeepSeekR1Provider {
    api_key: String,
    client: Client,
}

impl DeepSeekR1Provider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: super::build_http_client(),
        }
    }
}

impl LlmProviderTrait for DeepSeekR1Provider {
    fn name(&self) -> &str {
        "deepseek-r1"
    }

    fn supports_tools(&self) -> bool {
        false
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        // Never include tools — R1 does not support function calling.
        send_openai_compat(
            &self.client,
            &self.api_key,
            API_URL,
            MODEL,
            &request,
            false,
        )
        .await
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        stream_openai_compat(
            &self.client,
            &self.api_key,
            API_URL,
            MODEL,
            &request,
            false, // include_tools
            true,  // emit_thinking — emit reasoning_content as ThinkingDelta
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        validate_key_openai_compat(&self.client, &self.api_key, API_URL, MODEL).await
    }
}
