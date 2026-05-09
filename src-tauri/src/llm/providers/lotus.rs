//! Lotus LLM provider — talks to lotus gateway via the **anthropic native
//! ingress** (`/anthropic/v1/messages`), byte-level passthrough.
//!
//! Why anthropic ingress (Phase C, 2026-05): the previous OpenAI ingress
//! path went through the gateway's `convertToAnthropicBody` /
//! `convertAnthropicResponse` translation layers, which were the root of
//! `thinking_block`-mismatch errors — any field/order drift in the
//! signature roundtrip would 4xx. Anthropic ingress is byte-for-byte
//! passthrough, so the signature link end-to-end is invariant.
//!
//! Implementation strategy: this provider is a thin shell over
//! `ClaudeProvider::with_url(...)` (see `claude.rs`) — the body builder,
//! SSE state machine, tool/thinking handling are shared with the direct
//! anthropic.com path. The differences live entirely in:
//!   - URL: `/anthropic/v1/messages` on `ai-tenant.renlijia.com`
//!   - `is_direct = false` so beta-thinking headers are suppressed
//!   - retry policy on `send` (network-only retry; trust gateway failover
//!     for 5xx; honor 429 Retry-After once)
//!
//! Streaming has no client-side retry: the gateway already pre-deducts
//! billing on stream start, and once SSE bytes have been emitted to the
//! UI an automatic reconnect would either double-deduct or corrupt the
//! visible content.

use std::time::Duration;

use anyhow::{anyhow, Result};
use log::{debug, warn};

use crate::llm::providers::claude::ClaudeProvider;
use crate::llm::providers::LlmProviderTrait;
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

const LOTUS_ANTHROPIC_URL: &str = "https://ai-tenant.renlijia.com/anthropic/v1/messages";

/// Backstop default — used only when callers pass an empty model string.
/// In normal flow the model name comes from `AppSettings.cloud_model`.
const DEFAULT_MODEL: &str = "claude-sonnet-4-5";

/// Maximum extra attempts on `send` for network-class errors. The first
/// attempt counts as 0; with `MAX_RETRY_ATTEMPTS = 1` the worst case is
/// 2 total HTTP requests.
const MAX_RETRY_ATTEMPTS: u32 = 1;

/// Lotus cloud provider — proxies through the lotus tenant portal gateway.
pub struct LotusProvider {
    inner: ClaudeProvider,
}

impl LotusProvider {
    /// Construct a provider tied to the configured session_key + model.
    ///
    /// `_model_type` is retained for caller-signature compatibility with
    /// the OpenAI-ingress era (`"chat"` vs `"reasoner"`). The anthropic
    /// ingress has only one endpoint; reasoner-vs-chat is now expressed
    /// through the model name + `LlmRequest.tools` (empty tools = reasoner-
    /// equivalent). The argument is unused but preserved to avoid
    /// rippling a signature change through `gateway.rs` callers.
    pub fn new(session_key: String, model: String, _model_type: &str) -> Self {
        let model = if model.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model
        };
        Self {
            inner: ClaudeProvider::with_url(
                session_key,
                Some(model),
                LOTUS_ANTHROPIC_URL.to_string(),
                false, // is_direct=false → suppress anthropic-beta headers
            ),
        }
    }
}

/// Classify an `anyhow::Error` from `ClaudeProvider` into one of three
/// retry buckets. The classification is deliberately string-based: the
/// error wrapping in claude.rs is `"Anthropic API error ({status}): ..."`
/// or a `reqwest` transport error rendered through `?` — there's no
/// structured status code to match on without restructuring claude.rs.
///
/// Returns:
///   - `Some(None)`: retry immediately (network-class transport error)
///   - `Some(Some(d))`: retry after duration `d` (429 with Retry-After)
///   - `None`: do NOT retry (4xx, 5xx, or anything we can't confidently
///     classify as transient)
fn classify_for_retry(err: &anyhow::Error) -> Option<Option<Duration>> {
    let s = err.to_string();
    let lower = s.to_lowercase();

    // 5xx → trust gateway-side failover; client-side retry would only
    // duplicate a request that the gateway already tried alternates for.
    if s.contains("Anthropic API error (5") || s.contains("Anthropic API stream error (5") {
        return None;
    }

    // 401/403 → token-state error, not transient. Caller flow handles
    // sign-in re-prompt elsewhere.
    if s.contains("Anthropic API error (401)") || s.contains("Anthropic API error (403)") {
        return None;
    }

    // 429 → backoff per Retry-After is the contract, but our error
    // wrapper doesn't expose response headers. Use a fixed 1s backoff
    // for one retry, matching the lower bound of typical Retry-After.
    if s.contains("Anthropic API error (429)") {
        return Some(Some(Duration::from_secs(1)));
    }

    // Other 4xx → caller-fixable (bad model, bad input, quota exceeded).
    // Don't retry; surface verbatim so the UI message is meaningful.
    if s.contains("Anthropic API error (4") {
        return None;
    }

    // Heuristic for transport errors. reqwest renders these without a
    // numeric HTTP status. We treat connection-class strings as retryable
    // and everything else as opaque (don't retry).
    let transport_markers = [
        "connection",
        "connect ",
        "timed out",
        "timeout",
        "tls",
        "handshake",
        "broken pipe",
        "reset by peer",
        "dns",
        "io error",
    ];
    if transport_markers.iter().any(|m| lower.contains(m)) {
        return Some(None);
    }

    None
}

impl LlmProviderTrait for LotusProvider {
    fn name(&self) -> &str {
        "lotus"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_prompt_caching(&self) -> bool {
        // Gateway is byte-level passthrough; cache_control fields make it
        // through to the upstream and the cache_read/cache_creation
        // tokens come back in usage. Same semantics as direct anthropic.
        true
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=MAX_RETRY_ATTEMPTS {
            match self.inner.send(request.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    match classify_for_retry(&err) {
                        Some(delay) if attempt < MAX_RETRY_ATTEMPTS => {
                            if let Some(d) = delay {
                                debug!(
                                    "lotus send retry after {:?} (attempt {}/{}): {}",
                                    d,
                                    attempt + 1,
                                    MAX_RETRY_ATTEMPTS + 1,
                                    err
                                );
                                tokio::time::sleep(d).await;
                            } else {
                                debug!(
                                    "lotus send retry immediately (attempt {}/{}): {}",
                                    attempt + 1,
                                    MAX_RETRY_ATTEMPTS + 1,
                                    err
                                );
                            }
                            last_err = Some(err);
                            continue;
                        }
                        _ => {
                            return Err(err);
                        }
                    }
                }
            }
        }
        // Loop exits only when the last classification said "retry" but
        // we've used up MAX_RETRY_ATTEMPTS; surface the last error.
        Err(last_err.unwrap_or_else(|| anyhow!("lotus send: unreachable retry loop exit")))
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        // No client-side retry on streaming: the gateway pre-deducts on
        // stream start; a client retry after partial bytes would either
        // double-deduct or corrupt the UI by replaying content. Network
        // failures during the stream propagate to the caller, which will
        // prompt the user to resend.
        match self.inner.stream(request).await {
            Ok(s) => Ok(s),
            Err(e) => {
                warn!("lotus stream failed: {}", e);
                Err(e)
            }
        }
    }

    async fn validate_key(&self) -> Result<bool> {
        self.inner.validate_key().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_5xx_does_not_retry() {
        let e = anyhow!("Anthropic API error (502): bad gateway");
        assert!(classify_for_retry(&e).is_none());
    }

    #[test]
    fn classify_4xx_does_not_retry() {
        let e = anyhow!("Anthropic API error (400): invalid model");
        assert!(classify_for_retry(&e).is_none());
    }

    #[test]
    fn classify_401_does_not_retry() {
        let e = anyhow!("Anthropic API error (401): unauthorized");
        assert!(classify_for_retry(&e).is_none());
    }

    #[test]
    fn classify_429_retries_with_backoff() {
        let e = anyhow!("Anthropic API error (429): rate limited");
        let res = classify_for_retry(&e);
        assert!(matches!(res, Some(Some(_))));
    }

    #[test]
    fn classify_transport_error_retries_immediately() {
        let e = anyhow!("error sending request: connection reset by peer");
        let res = classify_for_retry(&e);
        assert!(matches!(res, Some(None)));
    }

    #[test]
    fn classify_unknown_does_not_retry() {
        let e = anyhow!("something completely unrelated");
        assert!(classify_for_retry(&e).is_none());
    }
}
