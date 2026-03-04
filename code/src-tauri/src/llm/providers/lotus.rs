//! Lotus LLM provider — OpenAI-compatible gateway at ai-tenant.renlijia.com.
//!
//! Uses session key (sk-sess***) from AuthManager for authentication.
//! Supports two endpoints:
//! - /v1/chat/completions     — chat models
//! - /v1/reasoner/completions — reasoner models

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::openai::{
    send_openai_compat, stream_openai_compat, validate_key_openai_compat,
};
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

const CHAT_URL: &str = "https://ai-tenant.renlijia.com/v1/chat/completions";
const REASONER_URL: &str = "https://ai-tenant.renlijia.com/v1/reasoner/completions";

/// Lotus cloud provider — proxies through the tenant portal gateway.
pub struct LotusProvider {
    session_key: String,
    model: String,
    api_url: String,
    client: Client,
}

impl LotusProvider {
    pub fn new(session_key: String, model: String, model_type: &str) -> Self {
        let api_url = match model_type {
            "reasoner" => REASONER_URL.to_string(),
            _ => CHAT_URL.to_string(),
        };
        Self {
            session_key,
            model,
            api_url,
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
            &self.api_url,
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
            &self.api_url,
            &self.model,
            &request,
            true,
            false,
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        validate_key_openai_compat(&self.client, &self.session_key, &self.api_url, &self.model).await
    }
}
