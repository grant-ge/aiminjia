#![allow(dead_code)]

pub mod claude;
pub mod deepseek_r1;
pub mod deepseek_v3;
pub mod openai;
pub mod qwen;
pub mod volcano;

use anyhow::Result;

use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

/// Build a shared HTTP client with a 30-second TCP connect timeout.
///
/// Only `connect_timeout` is set — no global `timeout` — because
/// streaming responses can legitimately run for several minutes.
pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Trait that all LLM providers must implement.
///
/// Each provider handles its own API format, authentication,
/// and response parsing. Uses Rust's native RPITIT (return position
/// impl Trait in trait), stable since Rust 1.75, instead of the
/// `async_trait` macro.
pub trait LlmProviderTrait: Send + Sync {
    /// Provider display name (e.g. "DeepSeek V3", "Claude").
    fn name(&self) -> &str;

    /// Whether this provider supports tool use.
    fn supports_tools(&self) -> bool;

    /// Whether this provider supports streaming.
    fn supports_streaming(&self) -> bool {
        true
    }

    /// Send a complete (non-streaming) request.
    fn send(
        &self,
        request: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse>> + Send;

    /// Send a streaming request, returning a stream of events.
    fn stream(
        &self,
        request: LlmRequest,
    ) -> impl std::future::Future<Output = Result<StreamBox>> + Send;

    /// Validate the API key by making a minimal test request.
    fn validate_key(&self) -> impl std::future::Future<Output = Result<bool>> + Send;
}
