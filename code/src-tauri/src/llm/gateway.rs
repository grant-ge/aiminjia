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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use anyhow::Result;

use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::lotus;
use crate::llm::providers::claude;
use crate::llm::providers::custom;
use crate::llm::providers::deepseek_r1;
use crate::llm::providers::deepseek_v3;
use crate::llm::providers::openai;
use crate::llm::providers::qwen;
use crate::llm::providers::volcano;
use crate::llm::router::{self, RouteResult};
use crate::llm::streaming::*;
use crate::llm::tools;
use crate::models::settings::AppSettings;
use crate::storage::file_store::AppStorage;

/// Maximum number of concurrent agent loops.
pub const MAX_CONCURRENT_AGENTS: usize = 3;

/// Maximum number of retry attempts for retryable errors (429, 5xx, timeout).
const MAX_RETRIES: u32 = 3;

/// Initial backoff delay in milliseconds (doubles each retry: 1s → 2s → 4s).
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Check if an error is retryable (rate limit, server error, or network timeout).
///
/// Parses the error message for HTTP status codes and known error patterns.
/// Non-retryable errors (401 auth, 400 bad request, etc.) return false.
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
    if max_ms == 0 { return 0; }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos % max_ms
}

/// Active streaming task handle; dropping the sender cancels the stream.
struct ActiveTask {
    id: String,
    conversation_id: String,
    cancel: tokio::sync::watch::Sender<bool>,
    started_at: std::time::Instant,
}

/// The central LLM gateway.
///
/// Owns a reference to the database (for future audit logging) and tracks
/// currently active streaming tasks so they can be cancelled on demand.
pub struct LlmGateway {
    #[allow(dead_code)]
    db: Arc<AppStorage>,
    active_tasks: Arc<Mutex<HashMap<String, ActiveTask>>>,
}

impl LlmGateway {
    /// Create a new gateway backed by the given database.
    pub fn new(db: Arc<AppStorage>) -> Self {
        Self {
            db,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build an [`LlmRequest`] from messages, route, and settings.
    ///
    /// If `system_prompt` is provided, it is prepended as a system message.
    /// If `tool_defs_override` is provided, those tools are used instead of
    /// the full tool registry (for step-filtered analysis).
    /// `max_tokens` controls the output budget — use lower values for
    /// tool-call iterations and higher for final responses.
    fn build_request(
        mut masked_messages: Vec<ChatMessage>,
        route: &RouteResult,
        stream: bool,
        system_prompt: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
    ) -> LlmRequest {
        // Prepend system prompt if provided
        if let Some(prompt) = system_prompt {
            masked_messages.insert(
                0,
                ChatMessage::text("system", prompt),
            );
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
        }
    }

    /// Send a message and stream the response.
    ///
    /// Returns a `(task_id, StreamBox)` tuple. The task ID can be passed to
    /// [`cancel_conversation`] to abort the stream early.
    ///
    /// # Parameters
    /// - `system_prompt`: Optional system prompt to prepend to messages.
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
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        conversation_id: Option<&str>,
    ) -> Result<(String, StreamBox, MaskingContext, tokio::sync::watch::Receiver<bool>)> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let conv_id = conversation_id.unwrap_or("").to_string();

        // 1. Route to best provider
        let task_type = router::infer_task_type(&messages);
        let route = router::select_route(&task_type, settings);

        log::info!(
            "Routing task {:?} to provider '{}' (tools={})",
            task_type,
            route.provider,
            route.use_tools
        );

        // 2. Apply data masking
        let mut mask_ctx = MaskingContext::new(masking_level);
        let masked_messages = mask_ctx.mask_messages(&messages);

        // 3. Build request
        let request = Self::build_request(masked_messages, &route, true, system_prompt, tool_defs_override, max_tokens);

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
            let content_preview: String = m.content.chars().take(120).collect();
            let has_tc = m.tool_calls.as_ref().map_or(0, |v| v.len());
            let tc_id = m.tool_call_id.as_deref().unwrap_or("-");
            log::debug!(
                "[LLM-REQ] msg[{}] role={} tc_id={} tool_calls={} content='{}'…",
                i, m.role, tc_id, has_tc, content_preview,
            );
        }

        // 4. Create cancellation channel and update active task map.
        // Check if the previous task (from set_busy()) was already cancelled
        // during the pre-stream window (between set_busy and first stream_message).
        // If so, bail out immediately instead of starting a new stream.
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        {
            let mut tasks = self.active_tasks.lock().unwrap();
            if let Some(existing) = tasks.get(&conv_id) {
                if *existing.cancel.borrow() {
                    log::info!(
                        "stream_message: conversation {} was cancelled before stream started, aborting",
                        conv_id
                    );
                    return Err(anyhow::anyhow!("Conversation cancelled before stream started"));
                }
            }
            tasks.insert(conv_id.clone(), ActiveTask {
                id: task_id.clone(),
                conversation_id: conv_id,
                cancel: cancel_tx,
                started_at: std::time::Instant::now(),
            });
        }

        // 5. Get stream from provider with retry on transient errors
        log::info!("dispatch_stream: provider={}, api_key_len={}, model_hint='{}'",
            route.provider, route.api_key.len(), route.model_hint);
        let stream = retry_dispatch_stream(&route, request).await?;

        // 6. Return raw stream + cancel_rx — caller uses tokio::select! for cancellation
        Ok((task_id, stream, mask_ctx, cancel_rx))
    }

    /// Cancel the streaming task for a specific conversation.
    ///
    /// Sends the cancel signal but does NOT remove the task from the map.
    /// The AgentGuard is responsible for cleanup after the agent loop exits.
    pub fn cancel_conversation(&self, conversation_id: &str) -> Result<()> {
        let tasks = self.active_tasks.lock().unwrap();
        if let Some(task) = tasks.get(conversation_id) {
            let _ = task.cancel.send(true);
            log::info!("Cancelled streaming task for conversation: {} (task_id={})", conversation_id, task.id);
        }
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
        tool_defs_override: Option<Vec<ToolDefinition>>,
    ) -> Result<LlmResponse> {
        // 1. Route to best provider
        let task_type = router::infer_task_type(&messages);
        let route = router::select_route(&task_type, settings);

        log::info!(
            "Sending (non-stream) task {:?} to provider '{}'",
            task_type,
            route.provider
        );

        // 2. Apply data masking
        let mut mask_ctx = MaskingContext::new(masking_level);
        let masked_messages = mask_ctx.mask_messages(&messages);

        // 3. Build request
        let request = Self::build_request(masked_messages, &route, false, system_prompt, tool_defs_override, 4096);

        // 4. Dispatch to provider with retry on transient errors
        let response = retry_dispatch_send(&route, request).await?;

        // 5. Unmask the response content
        let unmasked_content = mask_ctx.unmask(&response.content);
        Ok(LlmResponse {
            content: unmasked_content,
            ..response
        })
    }

    /// Returns true if there is at least one active task.
    pub fn is_busy(&self) -> bool {
        let tasks = self.active_tasks.lock().unwrap();
        !tasks.is_empty()
    }

    /// Returns true if a specific conversation has an active task.
    pub fn is_conversation_busy(&self, conversation_id: &str) -> bool {
        let tasks = self.active_tasks.lock().unwrap();
        tasks.contains_key(conversation_id)
    }

    /// Get all conversation IDs that currently have active tasks.
    pub fn get_busy_conversations(&self) -> Vec<String> {
        let tasks = self.active_tasks.lock().unwrap();
        tasks.keys().cloned().collect()
    }

    /// Clear the active task for a specific conversation.
    /// Called when the agent loop finishes.
    pub fn clear_task(&self, conversation_id: &str) {
        let mut tasks = self.active_tasks.lock().unwrap();
        if let Some(task) = tasks.remove(conversation_id) {
            log::info!("Cleared active task: id={}, conversation_id={}", task.id, task.conversation_id);
        }
    }

    /// Mark the gateway as busy for a given conversation.
    /// Used to reserve the agent before spawning the agent loop.
    /// Returns an error string if the conversation is already busy or max concurrency reached.
    pub fn set_busy(&self, conversation_id: &str) -> Result<(), String> {
        let mut tasks = self.active_tasks.lock().unwrap();
        if tasks.contains_key(conversation_id) {
            return Err("This conversation is already processing.".to_string());
        }
        if tasks.len() >= MAX_CONCURRENT_AGENTS {
            return Err(format!("Maximum concurrent conversations reached ({}). Please wait.", MAX_CONCURRENT_AGENTS));
        }
        let (cancel_tx, _) = tokio::sync::watch::channel(false);
        tasks.insert(conversation_id.to_string(), ActiveTask {
            id: format!("pre-{}", uuid::Uuid::new_v4()),
            conversation_id: conversation_id.to_string(),
            cancel: cancel_tx,
            started_at: std::time::Instant::now(),
        });
        log::info!("Gateway marked busy for conversation {} (active={})", conversation_id, tasks.len());
        Ok(())
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
                        attempt + 1, route.provider
                    );
                }
                return Ok(stream);
            }
            Err(e) => {
                if attempt < MAX_RETRIES && is_retryable_error(&e) {
                    let delay = backoff_with_jitter(attempt);
                    log::warn!(
                        "[retry] dispatch_stream failed (attempt {}/{}, retrying in {:?}): {}",
                        attempt + 1, MAX_RETRIES + 1, delay, e
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    if attempt > 0 {
                        log::error!(
                            "[retry] dispatch_stream failed after {} attempts: {}",
                            attempt + 1, e
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
                        attempt + 1, route.provider
                    );
                }
                return Ok(response);
            }
            Err(e) => {
                if attempt < MAX_RETRIES && is_retryable_error(&e) {
                    let delay = backoff_with_jitter(attempt);
                    log::warn!(
                        "[retry] dispatch_send failed (attempt {}/{}, retrying in {:?}): {}",
                        attempt + 1, MAX_RETRIES + 1, delay, e
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    if attempt > 0 {
                        log::error!(
                            "[retry] dispatch_send failed after {} attempts: {}",
                            attempt + 1, e
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
        "deepseek-v3" => {
            let p = deepseek_v3::DeepSeekV3Provider::new(route.api_key.clone());
            p.stream(request).await
        }
        "openai" => {
            let p = openai::OpenAiProvider::new(route.api_key.clone());
            p.stream(request).await
        }
        "claude" => {
            let p = claude::ClaudeProvider::new(route.api_key.clone(), None);
            p.stream(request).await
        }
        "deepseek-r1" => {
            let p = deepseek_r1::DeepSeekR1Provider::new(route.api_key.clone());
            p.stream(request).await
        }
        "volcano" => {
            let p = volcano::VolcanoProvider::new(
                route.api_key.clone(),
                route.model_hint.clone(),
            );
            p.stream(request).await
        }
        "qwen-plus" => {
            let p = qwen::QwenProvider::new(route.api_key.clone());
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
        "lotus" => {
            let p = lotus::LotusProvider::new(
                route.api_key.clone(),
                route.model_hint.clone(),
            );
            p.stream(request).await
        }
        other => {
            log::warn!(
                "Unknown provider '{}', falling back to deepseek-v3",
                other
            );
            let p = deepseek_v3::DeepSeekV3Provider::new(route.api_key.clone());
            p.stream(request).await
        }
    }
}

/// Dispatch a non-streaming request to the correct provider based on route.
async fn dispatch_send(route: &RouteResult, request: LlmRequest) -> Result<LlmResponse> {
    match route.provider.as_str() {
        "deepseek-v3" => {
            let p = deepseek_v3::DeepSeekV3Provider::new(route.api_key.clone());
            p.send(request).await
        }
        "openai" => {
            let p = openai::OpenAiProvider::new(route.api_key.clone());
            p.send(request).await
        }
        "claude" => {
            let p = claude::ClaudeProvider::new(route.api_key.clone(), None);
            p.send(request).await
        }
        "deepseek-r1" => {
            let p = deepseek_r1::DeepSeekR1Provider::new(route.api_key.clone());
            p.send(request).await
        }
        "volcano" => {
            let p = volcano::VolcanoProvider::new(
                route.api_key.clone(),
                route.model_hint.clone(),
            );
            p.send(request).await
        }
        "qwen-plus" => {
            let p = qwen::QwenProvider::new(route.api_key.clone());
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
        "lotus" => {
            let p = lotus::LotusProvider::new(
                route.api_key.clone(),
                route.model_hint.clone(),
            );
            p.send(request).await
        }
        other => {
            log::warn!(
                "Unknown provider '{}', falling back to deepseek-v3",
                other
            );
            let p = deepseek_v3::DeepSeekV3Provider::new(route.api_key.clone());
            p.send(request).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_status_code_standard() {
        assert_eq!(extract_status_code("API error (429): rate limited"), Some(429));
        assert_eq!(extract_status_code("Streaming API error (503): service unavailable"), Some(503));
        assert_eq!(extract_status_code("Anthropic API error (401): unauthorized"), Some(401));
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
        assert!(is_retryable_error(&anyhow::anyhow!("API error (500): internal server error")));
        assert!(is_retryable_error(&anyhow::anyhow!("Streaming API error (502): bad gateway")));
        assert!(is_retryable_error(&anyhow::anyhow!("API error (503): service unavailable")));
        assert!(is_retryable_error(&anyhow::anyhow!("API error (504): gateway timeout")));
    }

    #[test]
    fn test_not_retryable_4xx() {
        assert!(!is_retryable_error(&anyhow::anyhow!("API error (401): unauthorized")));
        assert!(!is_retryable_error(&anyhow::anyhow!("API error (400): bad request")));
        assert!(!is_retryable_error(&anyhow::anyhow!("API error (403): forbidden")));
    }

    #[test]
    fn test_retryable_network_errors() {
        assert!(is_retryable_error(&anyhow::anyhow!("request timed out")));
        assert!(is_retryable_error(&anyhow::anyhow!("Connection reset by peer")));
        assert!(is_retryable_error(&anyhow::anyhow!("connection refused")));
        assert!(is_retryable_error(&anyhow::anyhow!("Broken pipe")));
    }

    #[test]
    fn test_not_retryable_unknown_error() {
        assert!(!is_retryable_error(&anyhow::anyhow!("invalid JSON in response")));
        assert!(!is_retryable_error(&anyhow::anyhow!("unknown error")));
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
