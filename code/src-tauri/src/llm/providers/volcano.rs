//! Volcano Engine (ByteDance) provider — OpenAI-compatible.
//!
//! Endpoint: `https://ark.cn-beijing.volces.com/api/v3/chat/completions`
//! Model: configurable endpoint ID (e.g. `ep-xxxx`).
//! Supports tool use.

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

use super::openai::{send_openai_compat, stream_openai_compat, validate_key_openai_compat};

const API_URL: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";

/// Volcano Engine (ByteDance) provider.
pub struct VolcanoProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl VolcanoProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: super::build_http_client(),
        }
    }
}

impl LlmProviderTrait for VolcanoProvider {
    fn name(&self) -> &str {
        "volcano"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        send_openai_compat(
            &self.client,
            &self.api_key,
            API_URL,
            &self.model,
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
            &self.model,
            &request,
            true,  // include_tools
            false, // emit_thinking
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        validate_key_openai_compat(&self.client, &self.api_key, API_URL, &self.model).await
    }
}
