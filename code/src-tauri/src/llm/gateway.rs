//! LLM Gateway — orchestrates provider selection, request building,
//! streaming, and tool dispatch.
//!
//! The gateway is the single entry point for all LLM interactions. It:
//! 1. Uses the [`router`] to select the optimal provider for the task.
//! 2. Applies data masking via [`MaskingContext`] before sending to the LLM.
//! 3. Attaches tool definitions from [`tools`] when the provider supports them.
//! 4. Manages streaming with cancellation support.
//! 5. Unmasks the response content before returning to the caller.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use tokio::sync::Mutex;

use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::claude;
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

        // 4. Create cancellation channel
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        // Store active task in per-conversation map
        {
            let mut tasks = self.active_tasks.lock().await;
            tasks.insert(conv_id.clone(), ActiveTask {
                id: task_id.clone(),
                conversation_id: conv_id,
                cancel: cancel_tx,
                started_at: std::time::Instant::now(),
            });
        }

        // 5. Get stream from provider (enum dispatch — trait uses RPITIT)
        log::info!("dispatch_stream: provider={}, api_key_len={}, model_hint='{}'",
            route.provider, route.api_key.len(), route.model_hint);
        let stream = dispatch_stream(&route, request).await?;

        // 6. Return raw stream + cancel_rx — caller uses tokio::select! for cancellation
        Ok((task_id, stream, mask_ctx, cancel_rx))
    }

    /// Cancel the streaming task for a specific conversation.
    ///
    /// If there is no active task for this conversation, this is a no-op.
    pub async fn cancel_conversation(&self, conversation_id: &str) -> Result<()> {
        let mut tasks = self.active_tasks.lock().await;
        if let Some(task) = tasks.remove(conversation_id) {
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

        // 4. Dispatch to provider
        let response = dispatch_send(&route, request).await?;

        // 5. Unmask the response content
        let unmasked_content = mask_ctx.unmask(&response.content);
        Ok(LlmResponse {
            content: unmasked_content,
            ..response
        })
    }

    /// Returns true if there is at least one active task.
    pub async fn is_busy(&self) -> bool {
        let tasks = self.active_tasks.lock().await;
        !tasks.is_empty()
    }

    /// Returns true if a specific conversation has an active task.
    pub async fn is_conversation_busy(&self, conversation_id: &str) -> bool {
        let tasks = self.active_tasks.lock().await;
        tasks.contains_key(conversation_id)
    }

    /// Get all conversation IDs that currently have active tasks.
    pub async fn get_busy_conversations(&self) -> Vec<String> {
        let tasks = self.active_tasks.lock().await;
        tasks.keys().cloned().collect()
    }

    /// Clear the active task for a specific conversation.
    /// Called when the agent loop finishes.
    pub async fn clear_task(&self, conversation_id: &str) {
        let mut tasks = self.active_tasks.lock().await;
        if let Some(task) = tasks.remove(conversation_id) {
            log::info!("Cleared active task: id={}, conversation_id={}", task.id, task.conversation_id);
        }
    }

    /// Mark the gateway as busy for a given conversation.
    /// Used to reserve the agent before spawning the agent loop.
    /// Returns an error string if the conversation is already busy or max concurrency reached.
    pub async fn set_busy(&self, conversation_id: &str) -> Result<(), String> {
        let mut tasks = self.active_tasks.lock().await;
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
