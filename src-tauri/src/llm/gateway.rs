//! LLM Gateway — orchestrates provider selection, request building,
//! streaming, and tool dispatch.
//!
//! The gateway is the single entry point for all LLM interactions. It:
//! 1. Uses the [`router`] to select the optimal provider for the task.
//! 2. Applies data masking via [`MaskingContext`] before sending to the LLM.
//! 3. Attaches tool definitions from [`tools`] when the provider supports them.
//! 4. Manages streaming with cancellation support.
//! 5. Unmasks the response content before returning to the caller.
//! 6. Retries retryable errors (429/5xx/timeout) with exponential backoff.
#![allow(dead_code)]

use anyhow::Result;
use std::sync::Arc;

use crate::auth::AuthManager;
use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::providers::aijia_gateway_v2;
use crate::llm::providers::custom;
use crate::llm::providers::openai;
use crate::llm::providers::LlmProviderTrait;
use crate::llm::router::{self, RouteResult};
use crate::llm::streaming::*;
use crate::llm::tools;
use crate::models::settings::AppSettings;
use crate::runtime::ids::RunId;
use crate::runtime::run_registry::RuntimeRunRegistry;
use crate::storage::file_store::AppStorage;

/// Maximum number of concurrent agent loops.
pub const MAX_CONCURRENT_AGENTS: usize = 99;

/// Maximum number of retry attempts for retryable errors (429, 5xx, timeout).
const MAX_RETRIES: u32 = 3;

/// Initial backoff delay in milliseconds (doubles each retry: 1s → 2s → 4s).
const INITIAL_BACKOFF_MS: u64 = 1000;

pub fn thinking_config_for_route(
    route: &RouteResult,
    settings: &AppSettings,
) -> Option<ThinkingConfig> {
    if route.provider != "aijia-v2" {
        return None;
    }

    match settings.thinking_type.as_str() {
        "adaptive" => Some(ThinkingConfig::Adaptive),
        "enabled" => Some(ThinkingConfig::Enabled {
            budget_tokens: settings.thinking_budget_tokens,
        }),
        "disabled" => Some(ThinkingConfig::Disabled),
        _ => Some(ThinkingConfig::Disabled),
    }
}

pub(crate) fn format_llm_error_diagnostics(err: &anyhow::Error) -> String {
    let mut parts = vec![err.to_string()];

    for (idx, cause) in err.chain().enumerate().skip(1) {
        parts.push(format!("caused_by[{idx}]: {cause}"));
    }

    for cause in err.chain() {
        if let Some(reqwest_err) = cause.downcast_ref::<reqwest::Error>() {
            let status = reqwest_err
                .status()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string());
            let url = reqwest_err
                .url()
                .map(|u| u.as_str().to_string())
                .unwrap_or_else(|| "-".to_string());
            parts.push(format!(
                "reqwest: timeout={} connect={} request={} body={} decode={} status={} url={}",
                reqwest_err.is_timeout(),
                reqwest_err.is_connect(),
                reqwest_err.is_request(),
                reqwest_err.is_body(),
                reqwest_err.is_decode(),
                status,
                url
            ));
            break;
        }
    }

    parts.join("; ")
}

fn cloud_session_error_message(err: &anyhow::Error) -> String {
    let message = err.to_string();
    if message.contains("登录已过期") || message.contains("未登录") {
        message
    } else {
        format!("登录状态异常：{message}")
    }
}

fn attach_anthropic_multimodal_turn(
    messages: &mut [ChatMessage],
    anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
) {
    let Some(turn) = anthropic_multimodal_turn else {
        return;
    };
    if turn.image_blocks.is_empty() {
        return;
    }
    if let Some(message) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == "user")
    {
        message.anthropic_multimodal_turn = Some(turn);
    }
}

/// Check if an error is retryable (rate limit, server error, or network timeout).
///
/// Parses the error message for HTTP status codes and known error patterns.
/// Non-retryable errors (401 auth, 400 bad request, etc.) return false.
/// Whether the error indicates the cloud session key was revoked / rejected
/// by the server, regardless of what the local `expires_at` says. Used by
/// `stream_message` to trigger a one-shot session refresh + retry instead of
/// surfacing "API key invalid" to the user.
fn is_auth_revoked_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    let lower = msg.to_lowercase();
    let is_401 = extract_status_code(&msg) == Some(401)
        || lower.contains("401 unauthorized")
        || lower.contains("(401)");
    if !is_401 {
        return false;
    }
    // Match by the gateway's structured error *type/code*, not the
    // human-readable message text. The cloud gateway tags session-key auth
    // failures with `authentication_error` / `auth_error` — this covers
    // "Session key expired",
    // "Session key revoked", "Invalid session key", "Missing Authorization
    // header", "Invalid key format", etc. A fresh session_key fixes all of
    // them, so a refresh + retry is always the right response. Other
    // 401-shaped reasons a refresh can't fix (tenant disabled, budget) come
    // back as 402/403 and never reach here.
    //
    // Previously this matched fixed substrings ("session expired" /
    // "invalid session" / ...), which silently missed "Session key expired"
    // (the "key" in the middle breaks the "session expired" substring) and
    // "Missing Authorization header" — so those 401s surfaced to the UI as
    // "API 密钥无效或已过期" with no auto-recovery. The trailing keyword checks
    // are defensive fallbacks for older gateway builds or non-JSON bodies.
    lower.contains("authentication_error")
        || lower.contains("auth_error")
        || lower.contains("session key")
        || lower.contains("invalid session")
        || lower.contains("authorization")
}

fn provider_uses_cloud_gateway(provider: &str) -> bool {
    provider == "aijia-v2"
}

fn gate_log_expected_for_route(route: &RouteResult) -> bool {
    route.provider == "aijia-v2"
}

fn route_log_summary(
    task_type: &router::TaskType,
    route: &RouteResult,
    settings: &AppSettings,
    conversation_id: Option<&str>,
    run_id: Option<&str>,
) -> String {
    format!(
        "[llm-route] task={:?} provider={} model_type={} model_hint={} tools={} cloud_gateway_mode={:?} gate_log_expected={} conv={} run={}",
        task_type,
        route.provider,
        route.model_type,
        route.model_hint,
        route.use_tools,
        settings.cloud_gateway_mode,
        gate_log_expected_for_route(route),
        conversation_id.filter(|s| !s.is_empty()).unwrap_or("-"),
        run_id.filter(|s| !s.is_empty()).unwrap_or("-"),
    )
}

fn is_retryable_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string();

    // Extract HTTP status code from error messages like "API error (429): ..."
    // or "Streaming API error (503): ..."
    if let Some(code) = extract_status_code(&msg) {
        return matches!(code, 429 | 500 | 502 | 503 | 504);
    }

    // Network-level errors from reqwest
    let lower = msg.to_lowercase();
    lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("error sending request")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
}

/// Extract HTTP status code from error message strings like "API error (429): ...".
fn extract_status_code(msg: &str) -> Option<u16> {
    // Match patterns: "error (NNN)" or "error(NNN)"
    let patterns = ["error (", "error("];
    for pat in &patterns {
        if let Some(pos) = msg.find(pat) {
            let after = &msg[pos + pat.len()..];
            let code_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = code_str.parse::<u16>() {
                return Some(code);
            }
        }
    }
    None
}

/// Compute backoff delay with jitter for retry attempt N (0-indexed).
///
/// Base delay doubles each attempt: 1s, 2s, 4s.
/// Jitter adds 0–25% random variation to prevent thundering herd.
fn backoff_with_jitter(attempt: u32) -> std::time::Duration {
    let base_ms = INITIAL_BACKOFF_MS * 2u64.pow(attempt);
    // Simple jitter: add 0–25% of base delay
    let jitter_ms = (base_ms / 4).max(1);
    let jitter = rand_jitter(jitter_ms);
    std::time::Duration::from_millis(base_ms + jitter)
}

/// Simple pseudo-random jitter (0..max_ms) without pulling in the rand crate.
/// Uses current time nanoseconds as entropy source — sufficient for backoff jitter.
fn rand_jitter(max_ms: u64) -> u64 {
    if max_ms == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos % max_ms
}

fn request_message_log_preview(message: &ChatMessage) -> String {
    if message.role == "system" {
        format!(
            "[system prompt redacted; chars={}]",
            message.content.chars().count()
        )
    } else {
        message.content.chars().take(120).collect()
    }
}

/// The central LLM gateway.
///
/// Owns a reference to the database (for future audit logging) and tracks
/// currently active streaming tasks so they can be cancelled on demand.
pub struct LlmGateway {
    #[allow(dead_code)]
    db: Arc<AppStorage>,
    // Stream-level bridge only. Runtime-owned cancellation stays in
    // SessionRuntime; gateway/run_registry must not become a second owner.
    run_registry: Arc<RuntimeRunRegistry>,
    /// Cloud auth manager — used to fetch a fresh session_key for AIjia Gateway requests.
    /// None in tests / non-cloud builds.
    auth_manager: Option<Arc<AuthManager>>,
}

impl LlmGateway {
    /// Create a new gateway backed by the given database.
    pub fn new(db: Arc<AppStorage>) -> Self {
        Self::new_with_registry(db, Arc::new(RuntimeRunRegistry::new()))
    }

    pub fn new_with_registry(db: Arc<AppStorage>, run_registry: Arc<RuntimeRunRegistry>) -> Self {
        Self {
            db,
            run_registry,
            auth_manager: None,
        }
    }

    /// Attach an [`AuthManager`] so cloud requests can retrieve a fresh session_key.
    pub fn with_auth_manager(mut self, auth_manager: Arc<AuthManager>) -> Self {
        self.auth_manager = Some(auth_manager);
        self
    }

    /// Build an [`LlmRequest`] from messages, route, and settings.
    ///
    /// If `system_prompt` is provided, it is prepended as a system message.
    /// If `context_message` is provided, it is inserted after the system prompt
    /// as a user message (for dynamic context that changes each iteration).
    /// If `tool_defs_override` is provided, those tools are used instead of
    /// the full tool registry (for step-filtered analysis).
    /// `max_tokens` controls the output budget — use lower values for
    /// tool-call iterations and higher for final responses.
    fn build_request(
        mut masked_messages: Vec<ChatMessage>,
        route: &RouteResult,
        stream: bool,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        settings: &AppSettings,
        system_segments: Option<Vec<crate::llm::streaming::SystemPromptSegment>>,
        conversation_id: Option<&str>,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> LlmRequest {
        // Prepend system prompt if provided (stable prefix for KV cache)
        if let Some(prompt) = system_prompt {
            masked_messages.insert(0, ChatMessage::text("system", prompt));
        }

        // Insert dynamic context message after system prompt, before conversation messages.
        // Uses role "user" for maximum provider compatibility (some providers
        // don't support multiple system messages).
        if let Some(ctx) = context_message {
            let has_system_prefix = system_prompt.is_some()
                || masked_messages.first().map(|message| message.role.as_str()) == Some("system");
            let insert_pos = if has_system_prefix { 1 } else { 0 };
            masked_messages.insert(insert_pos, ChatMessage::text("user", ctx));
        }

        let tool_defs = if route.use_tools {
            tool_defs_override.unwrap_or_else(tools::get_tool_definitions)
        } else {
            Vec::new()
        };

        LlmRequest {
            messages: masked_messages,
            tools: tool_defs,
            max_tokens,
            temperature: 0.7,
            stream,
            thinking_config: thinking_config_for_route(route, settings),
            anthropic_multimodal_turn: None,
            system_segments,
            conversation_id: conversation_id
                .filter(|id| !id.is_empty())
                .map(|id| id.to_string()),
            trace_id: trace_id
                .filter(|id| !id.is_empty())
                .map(|id| id.to_string()),
            run_id: run_id.filter(|id| !id.is_empty()).map(|id| id.to_string()),
        }
    }

    /// Send a message and stream the response.
    ///
    /// Returns a `(task_id, StreamBox)` tuple. The task ID can be passed to
    /// [`cancel_conversation`] to abort the stream early.
    ///
    /// # Parameters
    /// - `system_prompt`: Optional system prompt to prepend to messages (stable prefix).
    /// - `context_message`: Optional dynamic context message inserted after system prompt.
    ///   Used for file context, analysis notes, etc. that change each iteration.
    ///   Kept separate from system_prompt to preserve KV cache prefix stability.
    /// - `tool_defs_override`: Optional tool definitions to use instead of
    ///   the full registry (for step-filtered analysis).
    /// - `max_tokens`: Output token budget. Use lower values (4096) for
    ///   tool-call iterations to reduce latency, higher (8192) for final output.
    ///
    /// # Flow
    /// 1. Infer task type from messages.
    /// 2. Route to the best provider via [`router::select_route`].
    /// 3. Mask sensitive data in messages.
    /// 4. Attach tool definitions (if the provider supports tools).
    /// 5. Open a streaming connection to the provider.
    /// 6. Wrap the stream with cancellation support.
    pub async fn stream_message(
        &self,
        settings: &AppSettings,
        messages: Vec<ChatMessage>,
        masking_level: MaskingLevel,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        conversation_id: Option<&str>,
        anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
    ) -> Result<(
        String,
        StreamBox,
        MaskingContext,
        tokio::sync::watch::Receiver<bool>,
    )> {
        self.stream_message_inner(
            settings,
            messages,
            masking_level,
            system_prompt,
            context_message,
            tool_defs_override,
            max_tokens,
            conversation_id,
            None,
            anthropic_multimodal_turn,
            None,
            None,
        )
        .await
    }

    /// Like [`stream_message`] but accepts structured per-block cache
    /// segments. AIjia Gateway V2 can carry these hints to upstream models
    /// that support block-level prompt caching; other providers fall back to
    /// the flat `system_prompt`.
    pub async fn stream_message_with_segments(
        &self,
        settings: &AppSettings,
        messages: Vec<ChatMessage>,
        masking_level: MaskingLevel,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        conversation_id: Option<&str>,
        anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
        system_segments: Vec<crate::llm::streaming::SystemPromptSegment>,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<(
        String,
        StreamBox,
        MaskingContext,
        tokio::sync::watch::Receiver<bool>,
    )> {
        let segments = if system_segments.is_empty() {
            None
        } else {
            Some(system_segments)
        };
        self.stream_message_inner(
            settings,
            messages,
            masking_level,
            system_prompt,
            context_message,
            tool_defs_override,
            max_tokens,
            conversation_id,
            segments,
            anthropic_multimodal_turn,
            trace_id,
            run_id,
        )
        .await
    }

    async fn stream_message_inner(
        &self,
        settings: &AppSettings,
        messages: Vec<ChatMessage>,
        masking_level: MaskingLevel,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        conversation_id: Option<&str>,
        system_segments: Option<Vec<crate::llm::streaming::SystemPromptSegment>>,
        anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<(
        String,
        StreamBox,
        MaskingContext,
        tokio::sync::watch::Receiver<bool>,
    )> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let conv_id = conversation_id.unwrap_or("").to_string();

        // 1. Route to best provider
        let task_type = router::infer_task_type(&messages);
        let mut route = router::select_route(&task_type, settings);

        // Cloud mode: replace api_key with a fresh session_key from AuthManager.
        // primary_api_key in settings is for non-cloud providers; cloud gateway needs the
        // login-issued session_key which may have been refreshed since last save.
        // Keyed on the cloud-route predicate so that an old provider name
        // carried in settings still gets the session_key before being forced to V2.
        if provider_uses_cloud_gateway(&route.provider) {
            if let Some(auth) = &self.auth_manager {
                match auth.get_session_key().await {
                    Ok(sk) => route.api_key = sk,
                    Err(e) => return Err(anyhow::anyhow!(cloud_session_error_message(&e))),
                }
            }
        }
        log::info!(
            "{}",
            route_log_summary(&task_type, &route, settings, conversation_id, run_id)
        );

        // 2. Apply data masking
        let mut mask_ctx = MaskingContext::new(masking_level.clone());
        let mut masked_messages = mask_ctx.mask_messages(&messages);
        if provider_uses_cloud_gateway(&route.provider) {
            attach_anthropic_multimodal_turn(
                &mut masked_messages,
                anthropic_multimodal_turn.clone(),
            );
        }

        // 3. Build request
        let request = Self::build_request(
            masked_messages,
            &route,
            true,
            system_prompt,
            context_message,
            tool_defs_override.clone(),
            max_tokens,
            settings,
            system_segments.clone(),
            Some(conv_id.as_str()),
            trace_id,
            run_id,
        );

        // Log request summary for debugging LLM quality
        log::info!(
            "[LLM-REQ] messages={}, tools={}, system_prompt_chars={}, max_tokens={}, temp={}",
            request.messages.len(),
            request.tools.len(),
            system_prompt.map_or(0, |s| s.len()),
            request.max_tokens,
            request.temperature,
        );
        for (i, m) in request.messages.iter().enumerate() {
            let content_preview = request_message_log_preview(m);
            let has_tc = m.tool_calls.as_ref().map_or(0, |v| v.len());
            let tc_id = m.tool_call_id.as_deref().unwrap_or("-");
            log::debug!(
                "[LLM-REQ] msg[{}] role={} tc_id={} tool_calls={} content='{}'…",
                i,
                m.role,
                tc_id,
                has_tc,
                content_preview,
            );
        }

        // 4. Attach the provider stream to the runtime registry slot.
        let cancel_rx = self.run_registry.attach_stream(&conv_id, task_id.clone())?;

        // 5. Get stream from provider with retry on transient errors
        log::info!(
            "dispatch_stream: provider={}, api_key_len={}, model_hint='{}'",
            route.provider,
            route.api_key.len(),
            route.model_hint
        );
        let stream = match retry_dispatch_stream(&route, request.clone()).await {
            Ok(s) => s,
            Err(e) if provider_uses_cloud_gateway(&route.provider) && is_auth_revoked_error(&e) => {
                // 401 + "Session key revoked" from the gateway → the local cache
                // wallclock can't be trusted (clock skew, server-side revoke,
                // tz mismatch — see auth/mod.rs invalidate_session_key).
                // Force a renewal and retry once. If still 401, surface to UI.
                if let Some(auth) = &self.auth_manager {
                    log::warn!(
                        "[stream_message] cloud gateway returned 401 Session key revoked — forcing session refresh"
                    );
                    auth.invalidate_session_key().await;
                    match auth.get_session_key().await {
                        Ok(sk) => {
                            route.api_key = sk;
                            // Rebuild request so the new api_key reaches the
                            // Rebuild request so the new api_key reaches the
                            // provider.
                            let mut mask_ctx_retry = MaskingContext::new(masking_level);
                            let mut masked_retry = mask_ctx_retry.mask_messages(&messages);
                            if provider_uses_cloud_gateway(&route.provider) {
                                attach_anthropic_multimodal_turn(
                                    &mut masked_retry,
                                    anthropic_multimodal_turn,
                                );
                            }
                            let request_retry = Self::build_request(
                                masked_retry,
                                &route,
                                true,
                                system_prompt,
                                context_message,
                                tool_defs_override,
                                max_tokens,
                                settings,
                                system_segments,
                                Some(conv_id.as_str()),
                                trace_id,
                                run_id,
                            );
                            log::info!(
                                "[stream_message] retrying with refreshed session_key (len={})",
                                route.api_key.len()
                            );
                            mask_ctx = mask_ctx_retry;
                            retry_dispatch_stream(&route, request_retry).await?
                        }
                        Err(refresh_err) => {
                            log::warn!(
                                "[stream_message] session refresh failed after 401: {}",
                                refresh_err
                            );
                            return Err(e);
                        }
                    }
                } else {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        };

        // 6. Return raw stream + cancel_rx — caller uses tokio::select! for cancellation
        Ok((task_id, stream, mask_ctx, cancel_rx))
    }

    /// Cancel the streaming task for a specific conversation.
    ///
    /// Sends the cancel signal but does NOT remove the task from the map.
    /// The AgentGuard is responsible for cleanup after the agent loop exits.
    pub fn cancel_conversation(&self, conversation_id: &str) -> Result<()> {
        self.run_registry.cancel(conversation_id);
        log::info!(
            "Cancelled streaming task for conversation: {}",
            conversation_id
        );
        Ok(())
    }

    /// Send a non-streaming message (for simple queries).
    ///
    /// The response content is unmasked before being returned.
    pub async fn send_message(
        &self,
        settings: &AppSettings,
        messages: Vec<ChatMessage>,
        masking_level: MaskingLevel,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
    ) -> Result<LlmResponse> {
        // 1. Route to best provider
        let task_type = router::infer_task_type(&messages);
        let mut route = router::select_route(&task_type, settings);

        // Cloud mode: same session_key injection as stream_message.
        if provider_uses_cloud_gateway(&route.provider) {
            if let Some(auth) = &self.auth_manager {
                match auth.get_session_key().await {
                    Ok(sk) => route.api_key = sk,
                    Err(e) => return Err(anyhow::anyhow!(cloud_session_error_message(&e))),
                }
            }
        }
        log::info!(
            "{}",
            route_log_summary(&task_type, &route, settings, None, None)
        );

        // 2. Apply data masking
        let mut mask_ctx = MaskingContext::new(masking_level);
        let masked_messages = mask_ctx.mask_messages(&messages);

        // 3. Build request
        let request = Self::build_request(
            masked_messages,
            &route,
            false,
            system_prompt,
            context_message,
            tool_defs_override,
            4096,
            settings,
            None,
            None, // send_message is non-streaming, conversation-less by signature
            None,
            None,
        );

        // 4. Dispatch to provider with retry on transient errors
        let response = retry_dispatch_send(&route, request).await?;

        // 5. Unmask the response content
        let unmasked_content = mask_ctx.unmask(&response.content);
        Ok(LlmResponse {
            content: unmasked_content,
            ..response
        })
    }

    /// 非流式版本的 [`stream_message_with_segments`]，用于 PR3 流式失败兜底。
    ///
    /// 签名与 stream_message_with_segments 完全对齐：复用 max_tokens /
    /// conversation_id / system_segments (block-level cache_control) /
    /// anthropic_multimodal_turn / trace_id / run_id，保证 fallback 与
    /// stream 走完全相同的 request 上下文（不丢图 / cache 不失效 / 追踪不断链）。
    ///
    /// 内部仍走 `retry_dispatch_send`，但云端路由会强制进入
    /// `/aijia/v2/ai/responses` 的非流式分支；不再触碰 legacy Anthropic
    /// ingress。
    ///
    /// Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §五.9.2
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_with_segments(
        &self,
        settings: &AppSettings,
        messages: Vec<ChatMessage>,
        masking_level: MaskingLevel,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        conversation_id: Option<&str>,
        anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
        system_segments: Vec<crate::llm::streaming::SystemPromptSegment>,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<LlmResponse> {
        let task_type = router::infer_task_type(&messages);
        let mut route = router::select_route(&task_type, settings);

        if provider_uses_cloud_gateway(&route.provider) {
            if let Some(auth) = &self.auth_manager {
                match auth.get_session_key().await {
                    Ok(sk) => route.api_key = sk,
                    Err(e) => return Err(anyhow::anyhow!(cloud_session_error_message(&e))),
                }
            }
        }
        log::info!(
            "Sending (non-stream fallback) task {:?} to provider '{}' conv={:?}",
            task_type,
            route.provider,
            conversation_id
        );

        let mut mask_ctx = MaskingContext::new(masking_level);
        let mut masked_messages = mask_ctx.mask_messages(&messages);
        if provider_uses_cloud_gateway(&route.provider) {
            attach_anthropic_multimodal_turn(
                &mut masked_messages,
                anthropic_multimodal_turn.clone(),
            );
        }

        let segments = if system_segments.is_empty() {
            None
        } else {
            Some(system_segments)
        };

        let request = Self::build_request(
            masked_messages,
            &route,
            false, // stream = false（关键：走非流式）
            system_prompt,
            context_message,
            tool_defs_override,
            max_tokens,
            settings,
            segments,
            conversation_id,
            trace_id,
            run_id,
        );

        let response = retry_dispatch_send(&route, request).await?;
        let unmasked_content = mask_ctx.unmask(&response.content);
        Ok(LlmResponse {
            content: unmasked_content,
            ..response
        })
    }

    /// Returns true if there is at least one active task.
    pub fn is_busy(&self) -> bool {
        self.run_registry.is_busy()
    }

    /// Returns true if a specific conversation has an active task.
    pub fn is_conversation_busy(&self, conversation_id: &str) -> bool {
        self.run_registry.is_session_busy(conversation_id)
    }

    /// Get all conversation IDs that currently have active tasks.
    pub fn get_busy_conversations(&self) -> Vec<String> {
        self.run_registry.busy_sessions()
    }

    /// Clear the active task for a specific conversation.
    /// Called when the agent loop finishes.
    pub fn clear_task(&self, conversation_id: &str) {
        if let Some(run_id) = self.run_registry.clear(conversation_id) {
            log::info!(
                "Cleared active task: conversation_id={}, run_id={}",
                conversation_id,
                run_id.as_str()
            );
        }
    }

    pub fn clear_task_for_run(&self, conversation_id: &str, run_id: &RunId) {
        if self
            .run_registry
            .clear_for_run(conversation_id, run_id)
            .is_some()
        {
            log::info!(
                "Cleared active task: conversation_id={}, run_id={}",
                conversation_id,
                run_id.as_str()
            );
        }
    }

    /// Mark the gateway as busy for a given conversation.
    /// Used to reserve the agent before spawning the agent loop.
    /// Returns an error string if the conversation is already busy or max concurrency reached.
    pub fn set_busy(&self, conversation_id: &str) -> Result<(), String> {
        self.set_busy_for_run(
            conversation_id,
            RunId::new(format!("pre-{}", uuid::Uuid::new_v4())),
        )
    }

    pub fn set_busy_for_run(&self, conversation_id: &str, run_id: RunId) -> Result<(), String> {
        self.run_registry.reserve(conversation_id, run_id)?;
        log::info!(
            "Gateway marked busy for conversation {} (active={})",
            conversation_id,
            self.run_registry.busy_sessions().len()
        );
        Ok(())
    }

    pub fn active_run_id(&self, conversation_id: &str) -> Option<RunId> {
        self.run_registry.run_id_for_session(conversation_id)
    }
}

/// Dispatch a streaming request with exponential backoff retry.
///
/// Retries up to [`MAX_RETRIES`] times on retryable errors (429, 5xx, timeout).
/// Non-retryable errors (401, 400, etc.) are returned immediately.
/// The request is cloned for each retry attempt.
async fn retry_dispatch_stream(route: &RouteResult, request: LlmRequest) -> Result<StreamBox> {
    let mut last_err = None;

    for attempt in 0..=MAX_RETRIES {
        match dispatch_stream(route, request.clone()).await {
            Ok(stream) => {
                if attempt > 0 {
                    log::info!(
                        "[retry] dispatch_stream succeeded on attempt {} for provider '{}'",
                        attempt + 1,
                        route.provider
                    );
                }
                return Ok(stream);
            }
            Err(e) => {
                if attempt < MAX_RETRIES && is_retryable_error(&e) {
                    let delay = backoff_with_jitter(attempt);
                    log::warn!(
                        "[retry] dispatch_stream failed (attempt {}/{}, retrying in {:?}): {}",
                        attempt + 1,
                        MAX_RETRIES + 1,
                        delay,
                        format_llm_error_diagnostics(&e)
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    if attempt > 0 {
                        log::error!(
                            "[retry] dispatch_stream failed after {} attempts: {}",
                            attempt + 1,
                            format_llm_error_diagnostics(&e)
                        );
                    }
                    return Err(e);
                }
            }
        }
    }

    // All retries exhausted — return the last error
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("All retry attempts exhausted")))
}

/// Dispatch a non-streaming request with exponential backoff retry.
///
/// Same retry policy as [`retry_dispatch_stream`].
async fn retry_dispatch_send(route: &RouteResult, request: LlmRequest) -> Result<LlmResponse> {
    let mut last_err = None;

    for attempt in 0..=MAX_RETRIES {
        match dispatch_send(route, request.clone()).await {
            Ok(response) => {
                if attempt > 0 {
                    log::info!(
                        "[retry] dispatch_send succeeded on attempt {} for provider '{}'",
                        attempt + 1,
                        route.provider
                    );
                }
                return Ok(response);
            }
            Err(e) => {
                if attempt < MAX_RETRIES && is_retryable_error(&e) {
                    let delay = backoff_with_jitter(attempt);
                    log::warn!(
                        "[retry] dispatch_send failed (attempt {}/{}, retrying in {:?}): {}",
                        attempt + 1,
                        MAX_RETRIES + 1,
                        delay,
                        format_llm_error_diagnostics(&e)
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    if attempt > 0 {
                        log::error!(
                            "[retry] dispatch_send failed after {} attempts: {}",
                            attempt + 1,
                            format_llm_error_diagnostics(&e)
                        );
                    }
                    return Err(e);
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("All retry attempts exhausted")))
}

/// Dispatch a streaming request to the correct provider based on route.
///
/// We use match-based dispatch instead of `Box<dyn LlmProviderTrait>`
/// because the trait uses RPITIT (return-position `impl Trait`), which
/// is not object-safe.
async fn dispatch_stream(route: &RouteResult, request: LlmRequest) -> Result<StreamBox> {
    match route.provider.as_str() {
        "openai" => {
            let p = openai::OpenAiProvider::new(route.api_key.clone());
            p.stream(request).await
        }
        "custom" => {
            let p = custom::CustomProvider::new(
                route.api_key.clone(),
                route.endpoint_url.clone(),
                route.model_hint.clone(),
            );
            p.stream(request).await
        }
        "aijia-v2" => {
            let p = aijia_gateway_v2::AijiaGatewayV2Provider::with_route(
                route.api_key.clone(),
                route.model_type.clone(),
                route.use_tools,
            );
            p.stream(request).await
        }
        other => Err(anyhow::anyhow!("Unknown LLM provider: {}", other)),
    }
}

/// Dispatch a non-streaming request to the correct provider based on route.
async fn dispatch_send(route: &RouteResult, request: LlmRequest) -> Result<LlmResponse> {
    match route.provider.as_str() {
        "openai" => {
            let p = openai::OpenAiProvider::new(route.api_key.clone());
            p.send(request).await
        }
        "custom" => {
            let p = custom::CustomProvider::new(
                route.api_key.clone(),
                route.endpoint_url.clone(),
                route.model_hint.clone(),
            );
            p.send(request).await
        }
        "aijia-v2" => {
            let p = aijia_gateway_v2::AijiaGatewayV2Provider::with_route(
                route.api_key.clone(),
                route.model_type.clone(),
                route.use_tools,
            );
            p.send(request).await
        }
        other => Err(anyhow::anyhow!("Unknown LLM provider: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::settings::CloudGatewayMode;

    #[test]
    fn test_extract_status_code_standard() {
        assert_eq!(
            extract_status_code("API error (429): rate limited"),
            Some(429)
        );
        assert_eq!(
            extract_status_code("Streaming API error (503): service unavailable"),
            Some(503)
        );
        assert_eq!(
            extract_status_code("AIjia v2 stream error (401 Unauthorized): unauthorized"),
            Some(401)
        );
    }

    #[test]
    fn test_extract_status_code_no_space() {
        assert_eq!(extract_status_code("error(500): internal"), Some(500));
    }

    #[test]
    fn test_extract_status_code_no_match() {
        assert_eq!(extract_status_code("some random error"), None);
        assert_eq!(extract_status_code("connection refused"), None);
    }

    #[test]
    fn test_retryable_429() {
        let err = anyhow::anyhow!("API error (429): rate limit exceeded");
        assert!(is_retryable_error(&err));
    }

    #[test]
    fn test_retryable_5xx() {
        assert!(is_retryable_error(&anyhow::anyhow!(
            "API error (500): internal server error"
        )));
        assert!(is_retryable_error(&anyhow::anyhow!(
            "Streaming API error (502): bad gateway"
        )));
        assert!(is_retryable_error(&anyhow::anyhow!(
            "API error (503): service unavailable"
        )));
        assert!(is_retryable_error(&anyhow::anyhow!(
            "API error (504): gateway timeout"
        )));
    }

    #[test]
    fn test_not_retryable_4xx() {
        assert!(!is_retryable_error(&anyhow::anyhow!(
            "API error (401): unauthorized"
        )));
        assert!(!is_retryable_error(&anyhow::anyhow!(
            "API error (400): bad request"
        )));
        assert!(!is_retryable_error(&anyhow::anyhow!(
            "API error (403): forbidden"
        )));
    }

    #[test]
    fn test_retryable_network_errors() {
        assert!(is_retryable_error(&anyhow::anyhow!("request timed out")));
        assert!(is_retryable_error(&anyhow::anyhow!(
            "Connection reset by peer"
        )));
        assert!(is_retryable_error(&anyhow::anyhow!("connection refused")));
        assert!(is_retryable_error(&anyhow::anyhow!("Broken pipe")));
        assert!(is_retryable_error(&anyhow::anyhow!(
            "error sending request for url (https://ai-tenant.renlijia.com/aijia/v2/ai/responses)"
        )));
    }

    #[test]
    fn test_not_retryable_unknown_error() {
        assert!(!is_retryable_error(&anyhow::anyhow!(
            "invalid JSON in response"
        )));
        assert!(!is_retryable_error(&anyhow::anyhow!("unknown error")));
    }

    #[test]
    fn request_message_log_preview_redacts_system_content() {
        let message = ChatMessage::text("system", "SECRET_SYSTEM_PROMPT_CONTENT");

        let preview = request_message_log_preview(&message);

        assert!(!preview.contains("SECRET_SYSTEM_PROMPT_CONTENT"));
        assert!(preview.contains("redacted"));
        assert!(preview.contains("chars="));
    }

    #[test]
    fn request_message_log_preview_keeps_user_content_preview() {
        let content = format!("{}{}", "a".repeat(120), "SECRET_AFTER_LIMIT");
        let message = ChatMessage::text("user", &content);

        let preview = request_message_log_preview(&message);

        assert_eq!(preview, "a".repeat(120));
        assert!(!preview.contains("SECRET_AFTER_LIMIT"));
    }

    #[test]
    fn format_llm_error_diagnostics_includes_error_chain() {
        let err = anyhow::anyhow!(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset by peer"
        ))
        .context(
            "error sending request for url (https://ai-tenant.renlijia.com/aijia/v2/ai/responses)",
        );

        let formatted = format_llm_error_diagnostics(&err);

        assert!(formatted.contains("error sending request for url"));
        assert!(formatted.contains("caused_by[1]: connection reset by peer"));
    }

    #[test]
    fn build_request_inserts_dynamic_context_after_existing_system_message() {
        let route = RouteResult {
            provider: "openai".to_string(),
            api_key: String::new(),
            model_hint: "gpt-4o".to_string(),
            use_tools: false,
            endpoint_url: String::new(),
            model_type: "chat".to_string(),
        };
        let settings = AppSettings {
            primary_model: "openai".to_string(),
            auto_model_routing: false,
            ..AppSettings::default()
        };

        let request = LlmGateway::build_request(
            vec![
                ChatMessage::text("system", "existing system"),
                ChatMessage::text("user", "original user"),
            ],
            &route,
            true,
            None,
            Some("dynamic context"),
            None,
            4096,
            &settings,
            None,
            None,
            None,
            None,
        );

        let roles_and_content: Vec<(&str, &str)> = request
            .messages
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect();
        assert_eq!(
            roles_and_content,
            vec![
                ("system", "existing system"),
                ("user", "dynamic context"),
                ("user", "original user"),
            ]
        );
    }

    #[test]
    fn route_log_summary_marks_gate_log_expectation() {
        let settings = AppSettings {
            cloud_gateway_mode: CloudGatewayMode::Legacy,
            ..AppSettings::default()
        };
        let v2_route = RouteResult {
            provider: "aijia-v2".to_string(),
            api_key: "session".to_string(),
            model_hint: "claude-sonnet".to_string(),
            use_tools: true,
            endpoint_url: String::new(),
            model_type: "chat".to_string(),
        };
        let custom_route = RouteResult {
            provider: "custom".to_string(),
            api_key: "session".to_string(),
            model_hint: "local-model".to_string(),
            use_tools: true,
            endpoint_url: "http://localhost".to_string(),
            model_type: "chat".to_string(),
        };

        let v2 = route_log_summary(
            &router::TaskType::General,
            &v2_route,
            &settings,
            Some("conv-1"),
            Some("run-1"),
        );
        let custom = route_log_summary(
            &router::TaskType::General,
            &custom_route,
            &settings,
            Some("conv-1"),
            Some("run-1"),
        );

        assert!(v2.contains("provider=aijia-v2"));
        assert!(v2.contains("gate_log_expected=true"));
        assert!(v2.contains("conv=conv-1"));
        assert!(v2.contains("run=run-1"));
        assert!(custom.contains("provider=custom"));
        assert!(custom.contains("gate_log_expected=false"));
    }

    #[test]
    fn thinking_config_remains_v2_only_for_cloud_gateway() {
        let settings = AppSettings {
            thinking_type: "enabled".to_string(),
            thinking_budget_tokens: 8192,
            ..AppSettings::default()
        };
        let custom_route = RouteResult {
            provider: "custom".to_string(),
            api_key: "session".to_string(),
            model_hint: String::new(),
            use_tools: true,
            endpoint_url: "http://localhost".to_string(),
            model_type: "chat".to_string(),
        };
        let v2_route = RouteResult {
            provider: "aijia-v2".to_string(),
            endpoint_url: String::new(),
            ..custom_route.clone()
        };

        assert_eq!(thinking_config_for_route(&custom_route, &settings), None);
        assert_eq!(
            thinking_config_for_route(&v2_route, &settings),
            Some(ThinkingConfig::Enabled {
                budget_tokens: 8192
            })
        );
    }

    #[test]
    fn test_backoff_increases() {
        let d0 = backoff_with_jitter(0);
        let d1 = backoff_with_jitter(1);
        let d2 = backoff_with_jitter(2);
        // Base delays: 1s, 2s, 4s (plus up to 25% jitter)
        assert!(d0.as_millis() >= 1000 && d0.as_millis() <= 1250);
        assert!(d1.as_millis() >= 2000 && d1.as_millis() <= 2500);
        assert!(d2.as_millis() >= 4000 && d2.as_millis() <= 5000);
    }
}

#[cfg(test)]
mod stability_tests {
    use super::*;

    #[test]
    fn detects_aijia_v2_auth_error_envelope_as_revoked_session_key() {
        let err = anyhow::anyhow!(
            "{}",
            r#"AIjia v2 stream error (401 Unauthorized): {"error":{"code":"auth_error","message":"Invalid session key"}}"#
        );

        assert!(is_auth_revoked_error(&err));
    }

    #[test]
    fn detects_aijia_v2_auth_error_code_with_neutral_message() {
        let err = anyhow::anyhow!(
            "{}",
            r#"AIjia v2 stream error (401 Unauthorized): {"error":{"code":"auth_error","message":"Token rejected"}}"#
        );

        assert!(is_auth_revoked_error(&err));
    }

    #[test]
    fn does_not_treat_gateway_402_as_refreshable_session_key() {
        let err = anyhow::anyhow!(
            "{}",
            r#"AIjia v2 stream error (402 Payment Required): {"error":{"code":"insufficient_balance","message":"Insufficient balance"}}"#
        );

        assert!(!is_auth_revoked_error(&err));
    }

    #[test]
    fn cloud_session_error_message_does_not_mention_api_key() {
        let err = anyhow::anyhow!("登录已过期，请重新登录");

        assert_eq!(cloud_session_error_message(&err), "登录已过期，请重新登录");
    }
}
