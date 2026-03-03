//! Lotus LLM provider — OpenAI-compatible gateway at ai-tenant.renlijia.com.
//!
//! Uses session key (sk-sess***) from AuthManager for authentication.
//! All LLM models are routed through the Lotus gateway; the model name
//! is passed through directly (server-side routing).

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::openai::{
    send_openai_compat, stream_openai_compat, validate_key_openai_compat,
};
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

const API_URL: &str = "https://ai-tenant.renlijia.com/v1/chat/completions";

/// Lotus cloud provider — proxies through the tenant portal gateway.
pub struct LotusProvider {
    session_key: String,
    model: String,
    client: Client,
}

impl LotusProvider {
    pub fn new(session_key: String, model: String) -> Self {
        Self {
            session_key,
            model,
            client: super::build_http_client(),
        }
    }
}

impl LlmProviderTrait for LotusProvider {
    fn name(&self) -> &str {
        "lotus"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        send_openai_compat(
            &self.client,
            &self.session_key,
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
            &self.session_key,
            API_URL,
            &self.model,
            &request,
            true,
            false,
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        validate_key_openai_compat(&self.client, &self.session_key, API_URL, &self.model).await
    }
}
