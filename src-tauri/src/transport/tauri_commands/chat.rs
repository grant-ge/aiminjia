// Legacy Tauri command layer: constructs PluginContext to bootstrap tool execution.
// Suppress the deprecation lint here; this is the entry-point bridge between
// Tauri commands and the legacy PluginContext-based tool chain.
// Migrate to CapabilityContext when the command layer is refactored.
#![allow(deprecated)]
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tauri::{AppHandle, Emitter, Manager};

use crate::auth::AuthManager;
use crate::llm::analysis_context::AnalysisContext;
use crate::llm::content_filter::strip_hallucinated_xml;
use crate::llm::context_decay;
use crate::llm::gateway::LlmGateway;
use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::orchestrator::{self, StepConfig, StepStatus};
use crate::llm::prompt_guard;
use crate::llm::prompts;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent};
use crate::llm::taor::PhaseTracker;
use crate::models::settings::AppSettings;
use crate::plugin::skill_trait::{SkillState, StepAction, ToolFilter};
use crate::plugin::tool_trait::FileMeta;
use crate::plugin::{PluginContext, SkillRegistry, ToolRegistry};
use crate::runtime::conversation_service;
use crate::runtime::ids::{RunId, SessionId};
use crate::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, SessionRuntime,
};
use crate::storage::crypto::SecureStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

mod chat_runtime_impl;
mod chat_support;

pub(crate) use chat_runtime_impl::legacy_send_message_impl;

/// Maximum agent loop iterations for daily consultation mode.
/// 30 iterations allows multi-step browser workflows: navigate → login check →
/// filter → paginate → read data across multiple pages.
const MAX_TOOL_ITERATIONS: usize = 30;

/// Maximum wall-clock time for the entire agent loop (15 minutes for multi-step analysis).
const AGENT_TIMEOUT_SECS: u64 = 900;

/// Maximum time to wait for a single chunk from the LLM stream (90 seconds).
/// If no data arrives within this window, the stream is considered stalled.
const CHUNK_TIMEOUT_SECS: u64 = 90;

/// Maximum number of stream-level retries within the agent loop.
/// When a stream error or gateway error is retryable (5xx, timeout, connection),
/// the current iteration is retried instead of aborting the entire agent loop.
const MAX_STREAM_RETRIES: u32 = 2;

/// Delay before retrying a failed stream (seconds).
const STREAM_RETRY_DELAY_SECS: u64 = 2;

/// Character threshold for triggering context compression in daily mode.
/// When total message content exceeds this, older messages are summarized.
/// ~24K chars ≈ 8K tokens (Chinese averages ~3 chars/token).
const COMPRESS_THRESHOLD_CHARS: usize = 24_000;

/// Number of recent messages to preserve (not compress) during compression.
/// These are kept verbatim so the LLM has full context for the current exchange.
const COMPRESS_KEEP_RECENT: usize = 10;

pub(crate) use chat_support::{
    auto_capture_step_context, build_config_from_skill, build_knowledge_preamble,
    clear_analysis_notes, compress_tool_result, is_daily_question, truncate_at_char_boundary,
};

#[derive(Clone)]
struct TauriChatServices {
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    crypto: Option<Arc<SecureStorage>>,
    tool_registry: Arc<ToolRegistry>,
    skill_registry: Arc<SkillRegistry>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    auth_manager: Arc<AuthManager>,
    app: tauri::AppHandle,
}

struct TauriLegacyTurnExecutor {
    services: TauriChatServices,
}

#[async_trait]
impl RuntimeTurnExecutor for TauriLegacyTurnExecutor {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> Result<(), String> {
        legacy_send_message_impl(
            self.services.db.clone(),
            self.services.gateway.clone(),
            self.services.file_mgr.clone(),
            self.services.crypto.clone(),
            self.services.tool_registry.clone(),
            self.services.skill_registry.clone(),
            self.services.session_mgr.clone(),
            self.services.auth_manager.clone(),
            self.services.app.clone(),
            request.conversation_id,
            request.content,
            request.file_ids,
        )
        .await
    }
}

#[derive(Clone)]
pub struct TauriChatCommandAdapter {
    runtime: SessionRuntime,
    services: TauriChatServices,
}

impl TauriChatCommandAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<AppStorage>,
        gateway: Arc<LlmGateway>,
        file_mgr: Arc<FileManager>,
        crypto: Option<Arc<SecureStorage>>,
        tool_registry: Arc<ToolRegistry>,
        skill_registry: Arc<SkillRegistry>,
        session_mgr: Arc<crate::python::session::PythonSessionManager>,
        auth_manager: Arc<AuthManager>,
        app: tauri::AppHandle,
    ) -> Self {
        let services = TauriChatServices {
            db,
            gateway,
            file_mgr,
            crypto,
            tool_registry,
            skill_registry,
            session_mgr,
            auth_manager,
            app,
        };
        let runtime = SessionRuntime::with_executor(
            QueryEngine::new(),
            RuntimeEventBus::new(),
            Arc::new(TauriLegacyTurnExecutor {
                services: services.clone(),
            }),
        );
        Self { runtime, services }
    }

    pub async fn send_message(
        &self,
        conversation_id: String,
        content: String,
        file_ids: Vec<String>,
    ) -> Result<(), String> {
        self.runtime
            .run_chat_request(ChatTurnRequest::new(conversation_id, content, file_ids))
            .await
    }

    pub fn is_agent_busy(&self) -> Vec<String> {
        self.services.gateway.get_busy_conversations()
    }

    pub async fn stop_streaming(&self, conversation_id: String) -> Result<(), String> {
        conversation_service::stop_streaming(
            self.services.gateway.clone(),
            self.services.session_mgr.clone(),
            conversation_id,
        )
        .await
    }

    pub async fn get_messages(
        &self,
        conversation_id: String,
    ) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_messages(self.services.db.clone(), conversation_id).await
    }

    pub async fn create_conversation(&self) -> Result<String, String> {
        conversation_service::create_conversation(self.services.db.clone()).await
    }

    pub async fn delete_conversation(&self, conversation_id: String) -> Result<(), String> {
        let outcome = conversation_service::delete_conversation(
            self.services.db.clone(),
            self.services.gateway.clone(),
            self.services.file_mgr.clone(),
            self.services.session_mgr.clone(),
            conversation_id,
        )
        .await?;

        if outcome.cancelled_active_agent {
            let _ = self.services.app.emit(
                "streaming:done",
                serde_json::json!({
                    "conversationId": outcome.conversation_id,
                    "messageId": "",
                }),
            );
            let _ = self.services.app.emit(
                "agent:idle",
                serde_json::json!({
                    "conversationId": outcome.conversation_id,
                }),
            );
        }

        Ok(())
    }

    pub async fn rename_conversation(
        &self,
        conversation_id: String,
        new_title: String,
    ) -> Result<(), String> {
        let outcome = conversation_service::rename_conversation(
            self.services.db.clone(),
            conversation_id,
            new_title,
        )
        .await?;
        let _ = self.services.app.emit(
            "conversation:title-updated",
            serde_json::json!({
                "conversationId": outcome.conversation_id,
                "title": outcome.new_title,
            }),
        );
        Ok(())
    }

    pub async fn get_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
        conversation_service::get_conversations(self.services.db.clone()).await
    }
}
