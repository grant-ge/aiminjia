//! Qwen (通义千问) provider — fast model with tool use support.
//!
//! Endpoint: `https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions`
//! Uses the OpenAI-compatible chat completions format.
//! Model: `qwen-plus` (balanced speed + quality, ~60-80 tok/s output)

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

use super::openai::{send_openai_compat, stream_openai_compat, validate_key_openai_compat};

const API_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
const MODEL: &str = "qwen-plus";

/// Qwen-Plus provider (Alibaba Cloud DashScope).
pub struct QwenProvider {
    api_key: String,
    client: Client,
}

impl QwenProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: super::build_http_client(),
        }
    }
}

impl LlmProviderTrait for QwenProvider {
    fn name(&self) -> &str {
        "qwen-plus"
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
