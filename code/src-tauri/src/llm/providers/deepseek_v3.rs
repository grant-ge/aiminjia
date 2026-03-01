//! DeepSeek V3 provider — fast model, supports Tool Use.
//!
//! Endpoint: `https://api.deepseek.com/chat/completions`
//! Uses the OpenAI-compatible chat completions format.
//! Model: `deepseek-chat`

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

use super::openai::{send_openai_compat, stream_openai_compat, validate_key_openai_compat};

const API_URL: &str = "https://api.deepseek.com/chat/completions";
const MODEL: &str = "deepseek-chat";

/// DeepSeek V3 provider.
pub struct DeepSeekV3Provider {
    api_key: String,
    client: Client,
}

impl DeepSeekV3Provider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: super::build_http_client(),
        }
    }
}

impl LlmProviderTrait for DeepSeekV3Provider {
    fn name(&self) -> &str {
        "deepseek-v3"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        send_openai_compat(&self.client, &self.api_key, API_URL, MODEL, &request, true).await
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        stream_openai_compat(
            &self.client,
            &self.api_key,
            API_URL,
            MODEL,
            &request,
            true,  // include_tools
            false, // emit_thinking
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        validate_key_openai_compat(&self.client, &self.api_key, API_URL, MODEL).await
    }
}
