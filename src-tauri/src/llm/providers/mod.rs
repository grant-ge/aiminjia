#![allow(dead_code)]

// ============================================================================
// ⚠ ALL PROVIDERS BELOW ARE DEPRECATED — to be removed in 专项 P-router-model-passthrough
// ============================================================================
//
// 当前 8 个 provider 全部为「死代码或半成品」，等待后续重构删除：
//
//   死代码（产品 UI 不会触发）：
//   - `claude.rs`        Anthropic Messages API（异类协议，非 OpenAI；产品只暴露 OpenAI）
//   - `openai.rs`        OpenAI 直连（DEFAULT_MODEL="gpt-4o"，model id 不透传）
//   - `deepseek_v3.rs`   DeepSeek 直连（DEFAULT_MODEL="deepseek-chat"）
//   - `deepseek_r1.rs`   DeepSeek R1 直连（DEFAULT_MODEL="deepseek-reasoner"）
//   - `qwen.rs`          通义千问直连（DEFAULT_MODEL="qwen-plus"）
//   - `volcano.rs`       火山方舟直连（model_hint 透传，但产品不暴露）
//
//   当前在用但仍计划重构（半成品，将被统一接入层取代）：
//   - `lotus.rs`         远端 lotus 网关（cloud_model 通过 OpenAI 协议透传）
//   - `custom.rs`        用户自填 OpenAI-兼容端点（custom_model_name 透传）
//
// 重构方向（专项 P-router-model-passthrough，不在 Mode B 范围）：
//   - 收敛为单一 OpenAI-兼容 provider 实现 + endpoint/认证配置
//   - 删除上述 8 个独立 provider 文件
//   - 同步清理 `router.rs::get_provider_capabilities`、`gateway.rs::dispatch_*`
//     的 provider match 分支，以及 `AppSettings.primary_model` /
//     `primary_api_key` / `auto_model_routing` / `cloud_model_type` 等历史字段
//   - 修复 sub-agent model_override 在所有路径上的透传
//
// **保留** `openai.rs` 中的 `pub(super)` 共享函数（`send_openai_compat` /
// `stream_openai_compat` / `validate_key_openai_compat`），它们是 OpenAI 协议
// 的核心实现，被多 provider 复用。重构时这部分会迁入新统一实现。
// ============================================================================

pub mod claude;
pub mod custom;
pub mod deepseek_r1;
pub mod deepseek_v3;
pub mod lotus;
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
