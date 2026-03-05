//! Custom OpenAI-compatible provider.
//!
//! Supports any endpoint that implements the OpenAI chat completions API,
//! including Ollama, LM Studio, and other OpenAI-compatible services.
//! Reuses the shared helpers from `openai.rs`.

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};
use super::openai::{send_openai_compat, stream_openai_compat, validate_key_openai_compat};

pub struct CustomProvider {
    api_key: String,
    endpoint: String,
    model: String,
    client: Client,
}

impl CustomProvider {
    pub fn new(api_key: String, endpoint: String, model: String) -> Self {
        Self {
            api_key,
            endpoint,
            model,
            client: super::build_http_client(),
        }
    }

    fn api_url(&self) -> String {
        let base = self.endpoint.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }
}

impl LlmProviderTrait for CustomProvider {
    fn name(&self) -> &str {
        "custom"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = self.api_url();
        send_openai_compat(
            &self.client,
            &self.api_key,
            &url,
            &self.model,
            &request,
            true,
        )
        .await
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        let url = self.api_url();
        stream_openai_compat(
            &self.client,
            &self.api_key,
            &url,
            &self.model,
            &request,
            true,
            false,
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        let url = self.api_url();
        validate_key_openai_compat(&self.client, &self.api_key, &url, &self.model).await
    }
}
