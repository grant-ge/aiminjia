#![allow(dead_code)]

// ============================================================================
// Provider inventory
// ============================================================================
//
// **In active use:**
//   - `lotus.rs`   Lotus gateway via anthropic-native ingress
//                  (`/anthropic/v1/messages`). Phase C (2026-05-09) shifted
//                  it from OpenAI ingress to a thin shell over
//                  `ClaudeProvider::with_url(... is_direct=false)`. The
//                  body builder, SSE state machine, and tool/thinking
//                  handling are shared with the direct anthropic.com
//                  path. Retry policy on `send` is local to lotus.rs.
//   - `claude.rs`  Anthropic protocol implementation (used by lotus.rs +
//                  by direct anthropic.com calls via `ClaudeProvider::new`).
//                  Parameterized by URL + `is_direct` so the same code
//                  drives both endpoints.
//   - `custom.rs`  User-supplied OpenAI-compatible endpoint.
//   - `openai.rs`  OpenAI direct (DEFAULT_MODEL="gpt-4o"). NOTE: the
//                  `pub(super) send_openai_compat / stream_openai_compat /
//                  validate_key_openai_compat` functions ARE actively used
//                  by `custom.rs` — do not delete this file before that.
//
// Removed in 2026-05-10 cleanup (Phase C dead code):
//   - deepseek_v3.rs / deepseek_r1.rs / qwen.rs / volcano.rs — direct
//     providers superseded by Lotus anthropic ingress. Routing / gateway
//     / settings / router capabilities references all removed in the
//     same commit. `AppSettings.cloud_model_type` retained as a no-op
//     for persisted-setting compat (anthropic ingress has one endpoint).
// ============================================================================

pub mod aijia_gateway_v2;
pub mod claude;
pub mod custom;
pub mod lotus;
pub mod openai;

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

    /// Whether this provider supports Anthropic-style prompt caching.
    fn supports_prompt_caching(&self) -> bool {
        false
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
