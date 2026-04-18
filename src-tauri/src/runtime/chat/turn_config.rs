// src-tauri/src/runtime/chat/turn_config.rs

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::Value as JsonValue;

use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;

/// LLM provider/routing settings resolved once per turn.
///
/// This keeps runtime code independent from the transport/database layer while
/// still giving the executor everything it needs to call `LlmGateway` without
/// re-reading settings on every iteration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedLlmSettings {
    pub primary_model: String,
    pub primary_api_key: String,
    pub auto_model_routing: bool,
    pub custom_model_endpoint: String,
    pub custom_model_name: String,
    pub use_cloud: bool,
    pub cloud_model: String,
    pub cloud_model_type: String,
}

/// Turn 级不可变配置。在 run_chat_turn 入口处构建一次，之后只读。
#[derive(Debug, Clone)]
pub struct TurnConfig {
    pub system_prompt: String,
    pub tool_defs: Vec<JsonValue>,
    pub allowed_tools: Option<HashSet<String>>,
    pub max_iterations: usize,
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub is_analysis: bool,
    pub masking_level: String,
    pub workspace_path: PathBuf,
    pub llm_settings: ResolvedLlmSettings,
    pub conversation_id: String,
    pub run_id: String,
}

/// Turn 级可变状态。Driver 是唯一修改者。
#[derive(Debug)]
pub struct TurnIterationState {
    pub messages: Vec<JsonValue>,
    pub full_content: String,
    pub generated_file_ids: Vec<String>,
    pub all_file_metas: Vec<JsonValue>,
    pub iteration_count: usize,
    pub stream_cancelled: bool,
    pub step_tokens_in: u64,
    pub step_tokens_out: u64,
    pub force_no_tools: bool,
    pub safeguard_phase1_injected: bool,
}

impl TurnIterationState {
    pub fn new(messages: Vec<JsonValue>) -> Self {
        Self {
            messages,
            full_content: String::new(),
            generated_file_ids: Vec::new(),
            all_file_metas: Vec::new(),
            iteration_count: 0,
            stream_cancelled: false,
            step_tokens_in: 0,
            step_tokens_out: 0,
            force_no_tools: false,
            safeguard_phase1_injected: false,
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
    pub dynamic_context: &'a str,
    pub messages: Vec<JsonValue>,  // decayed 副本，非原始 messages 引用
    pub tool_defs: &'a [JsonValue],
    pub token_budget: usize,
    pub chunk_timeout_secs: u64,
    pub masking_level: &'a str,
    pub force_no_tools: bool,
    pub llm_settings: &'a ResolvedLlmSettings,
    pub conversation_id: &'a str,
    pub run_id: &'a str,
}

/// Executor 的结构化返回。Executor 只产出数据，不修改外部状态。
#[derive(Debug)]
pub enum LlmStepResult {
    /// LLM 返回了工具调用
    ToolCalls {
        assistant_content: String,
        tool_calls: Vec<RuntimeToolCallRequest>,
        tokens_in: u64,
        tokens_out: u64,
    },
    /// LLM 返回纯文本，无工具调用
    ContentComplete {
        content: String,
        tokens_in: u64,
        tokens_out: u64,
    },
    /// 用户取消
    Cancelled,
}

/// 结构化错误
#[derive(Debug, thiserror::Error)]
pub enum TurnError {
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Max retries exceeded")]
    MaxRetriesExceeded,
    #[error("Persistence error: {0}")]
    PersistenceError(String),
}
