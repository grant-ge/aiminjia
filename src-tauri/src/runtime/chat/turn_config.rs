// src-tauri/src/runtime/chat/turn_config.rs

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use crate::llm::streaming::AnthropicMultimodalTurn;
use crate::models::settings::CloudGatewayMode;
use crate::runtime::chat::compaction::AutoCompactState;
use crate::runtime::chat::preprocess::PreprocessRuntimeState;
use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use crate::runtime::hooks::config::HookRegistry;
use crate::runtime::ids::{RunId, SessionId};

/// LLM provider/routing settings resolved once per turn.
///
/// This keeps runtime code independent from the transport/database layer while
/// still giving the executor everything it needs to call `LlmGateway` without
/// re-reading settings on every iteration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLlmSettings {
    pub primary_model: String,
    pub primary_api_key: String,
    pub auto_model_routing: bool,
    pub custom_model_endpoint: String,
    pub custom_model_name: String,
    pub cloud_model: String,
    pub cloud_model_type: String,
    pub cloud_gateway_mode: CloudGatewayMode,
    pub thinking_type: String,
    pub thinking_budget_tokens: u32,
    pub masking_level: String,
}

impl Default for ResolvedLlmSettings {
    fn default() -> Self {
        Self {
            primary_model: String::new(),
            primary_api_key: String::new(),
            auto_model_routing: false,
            custom_model_endpoint: String::new(),
            custom_model_name: String::new(),
            cloud_model: String::new(),
            cloud_model_type: String::new(),
            cloud_gateway_mode: CloudGatewayMode::V2,
            thinking_type: "disabled".to_string(),
            thinking_budget_tokens: 8000,
            masking_level: "strict".to_string(),
        }
    }
}

/// Turn 级不可变配置。在 run_chat_turn 入口处构建一次，之后只读。
#[derive(Debug, Clone)]
pub struct TurnConfig {
    pub system_prompt: String,
    pub prompt_snapshot: Option<crate::runtime::chat::prompt::TurnPromptSnapshot>,
    pub tool_defs: Vec<JsonValue>,
    pub allowed_tools: Option<HashSet<String>>,
    pub max_iterations: usize,
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub masking_level: String,
    pub workspace_path: PathBuf,
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    pub llm_settings: ResolvedLlmSettings,
    pub conversation_id: SessionId,
    pub run_id: RunId,
    /// Top-level user-turn trace id (UUID from send_message). Used only for
    /// observability propagation to the lotus gateway via the X-Aijia-Trace-Id
    /// HTTP header. Empty string when unknown.
    pub trace_id: String,
    pub hook_registry: Option<Arc<HookRegistry>>,
}

/// Optional per-turn overrides resolved before the driver snapshots `TurnConfig`.
#[derive(Debug, Clone, Default)]
pub struct TurnConfigOverrides {
    pub system_prompt: Option<String>,
    pub tool_defs: Option<Vec<JsonValue>>,
    pub allowed_tools: Option<HashSet<String>>,
    pub max_iterations: Option<usize>,
    pub token_budget: Option<usize>,
    /// 当前会话已授权的本地工作目录（来自 AuthorizedWorkspaceStore）。
    /// 由 executor::load_turn_config_overrides 填入，转存到 TurnConfig 供 load_agents_md 使用。
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
}

/// Turn 级可变状态。Driver 是唯一修改者。
#[derive(Debug)]
pub struct TurnIterationState {
    pub messages: Vec<JsonValue>,
    pub full_content: String,
    pub generated_file_ids: Vec<String>,
    pub all_file_metas: Vec<JsonValue>,
    pub all_tool_calls: Vec<JsonValue>,
    pub final_thinking_blocks: Vec<JsonValue>,
    pub iteration_count: usize,
    pub stream_cancelled: bool,
    pub step_tokens_in: u64,
    pub step_tokens_out: u64,
    pub step_cache_creation_input_tokens: u64,
    pub step_cache_read_input_tokens: u64,
    pub force_no_tools: bool,
    pub compact_state: AutoCompactState,
    pub stop_hook_prevent_continuation: bool,
    pub stop_hook_reason: Option<String>,
    pub stop_hook_active: bool,
    pub max_output_tokens_recovery_count: usize,
    pub orphaned_permission_count: usize,
    pub preprocess_state: PreprocessRuntimeState,
    pub last_thinking_blocks: Vec<serde_json::Value>,
    pub final_only_content: String,
}

impl TurnIterationState {
    pub fn new(messages: Vec<JsonValue>) -> Self {
        Self {
            messages,
            full_content: String::new(),
            generated_file_ids: Vec::new(),
            all_file_metas: Vec::new(),
            all_tool_calls: Vec::new(),
            final_thinking_blocks: Vec::new(),
            iteration_count: 0,
            stream_cancelled: false,
            step_tokens_in: 0,
            step_tokens_out: 0,
            step_cache_creation_input_tokens: 0,
            step_cache_read_input_tokens: 0,
            force_no_tools: false,
            compact_state: AutoCompactState::new(),
            stop_hook_prevent_continuation: false,
            stop_hook_reason: None,
            stop_hook_active: false,
            max_output_tokens_recovery_count: 0,
            orphaned_permission_count: 0,
            preprocess_state: PreprocessRuntimeState::default(),
            last_thinking_blocks: Vec::new(),
            final_only_content: String::new(),
        }
    }

    /// Append a fully-prepared message batch in one step.
    ///
    /// The driver builds assistant/tool batches off-state and calls this once
    /// so turn cancellation can only land between batches, never mid-merge.
    pub fn append_messages_batch(&mut self, batch: Vec<JsonValue>) {
        self.messages.extend(batch);
    }
}

/// Executor 的只读输入。由 driver 从 TurnConfig + TurnIterationState 构建。
#[derive(Debug)]
pub struct LlmStepInput<'a> {
    pub system_prompt: &'a str,
    pub system_message: Option<serde_json::Value>,
    pub dynamic_context: &'a str,
    pub messages: Vec<JsonValue>, // decayed 副本，非原始 messages 引用
    pub tool_defs: &'a [JsonValue],
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub masking_level: &'a str,
    pub force_no_tools: bool,
    pub llm_settings: &'a ResolvedLlmSettings,
    pub conversation_id: &'a str,
    pub run_id: &'a str,
    /// Top-level user-turn trace id (UUID generated by send_message). Pure
    /// observability — propagated to gateway via the `X-Aijia-Trace-Id`
    /// HTTP header so SLS can correlate gateway events back to the client
    /// turn that triggered them. Empty string when unknown.
    pub trace_id: &'a str,
    pub estimated_tokens: usize,
    pub anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
}

/// Executor 的结构化返回。Executor 只产出数据，不修改外部状态。
#[derive(Debug)]
pub enum LlmStepResult {
    /// LLM 返回了工具调用
    ToolCalls {
        assistant_content: String,
        thinking_blocks: Vec<serde_json::Value>,
        tool_calls: Vec<RuntimeToolCallRequest>,
        tokens_in: u64,
        tokens_out: u64,
        cache_creation_input_tokens: u64,
        cache_read_input_tokens: u64,
    },
    /// LLM 返回纯文本，无工具调用
    ContentComplete {
        content: String,
        thinking_blocks: Vec<serde_json::Value>,
        tokens_in: u64,
        tokens_out: u64,
        cache_creation_input_tokens: u64,
        cache_read_input_tokens: u64,
        stop_reason: Option<String>,
    },
    /// 用户取消。`partial_content` 是取消前本轮已收到并发给前端的流式文本。
    Cancelled { partial_content: String },
}

/// 结构化错误
#[derive(Debug, thiserror::Error)]
pub enum TurnError {
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Prompt too long: {0}")]
    PromptTooLong(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Max retries exceeded")]
    MaxRetriesExceeded,
    #[error("Persistence error: {0}")]
    PersistenceError(String),
}

pub const MAX_OUTPUT_TOKENS_RECOVERY_LIMIT: usize = 3;
