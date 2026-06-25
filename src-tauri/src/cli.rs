use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::AuthManager;
use crate::llm::gateway::LlmGateway;
use crate::llm::prompts;
use crate::models::settings::AppSettings;
use crate::plugin::registry::ToolRegistry;
use crate::runtime::agent::async_task_store::AsyncAgentTaskStore;
use crate::runtime::agent::task_notification::TaskNotificationQueue;
use crate::runtime::chat::{ChatTurnOutcome, ChatTurnRequest};
use crate::runtime::event_bus::{RuntimeEventBus, RuntimeEventSubscriber};
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind, TurnStage};
use crate::runtime::ids::SessionId;
use crate::runtime::path_auth::{RuleSource, ToolPermissionContext};
use crate::runtime::store::{
    AuthorizedWorkspace, AuthorizedWorkspaceRef, AuthorizedWorkspaceStore,
    ConvJsonAuthorizedWorkspaceStore,
};
use crate::runtime::tools::permission::PermissionMode;
use crate::runtime::RuntimeRunRegistry;
use crate::storage::crypto::SecureStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::types::{ErrorKind, MessageError, StoredMessage};
use crate::storage::file_store::AppStorage;
use crate::storage::{AiJiaHome, CurrentUserStorage, GlobalConfigStore, UserScope};
use crate::storage::{UserScopedPathResolver, UserScopedPaths};
use crate::transport::tauri_commands::chat::{
    build_headless_chat_runtime, HeadlessChatRuntime, HeadlessChatRuntimeConfig,
};

pub type HeadlessStreamEventSink = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

#[derive(Clone)]
pub struct HeadlessBuildOptions {
    pub add_dirs: Vec<PathBuf>,
    pub session_id: Option<SessionId>,
    pub continue_latest: bool,
    pub system_prompt: Option<String>,
    pub max_turns: Option<usize>,
    pub permission_mode: PermissionMode,
    pub model: Option<String>,
    pub verbose: bool,
    pub stream_event_sink: Option<HeadlessStreamEventSink>,
}

#[derive(Debug, Clone)]
pub struct HeadlessAgentRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessRunOutput {
    pub session_id: String,
    pub answer: String,
    pub tool_calls: Vec<HeadlessToolCall>,
    pub iterations: usize,
    pub tokens: HeadlessTokenUsage,
    pub duration_ms: u64,
    pub duration_api_ms: u64,
    pub stop_reason: Option<String>,
    pub total_cost_usd: f64,
    pub model: String,
    pub requested_model: String,
    pub execution_model: String,
    pub permission_mode: String,
    pub workspace: String,
    pub uuid: String,
    pub is_error: bool,
    pub error_subtype: Option<String>,
    pub permission_denials: Vec<serde_json::Value>,
    #[serde(skip)]
    pub streamed_model: Option<String>,
    #[serde(skip)]
    pub execution_error: Option<String>,
    #[serde(skip)]
    pub tools: Vec<String>,
    #[serde(skip)]
    pub stream_messages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessTokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_creation_input: u64,
    pub cache_read_input: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessToolCall {
    pub id: String,
    pub name: String,
    pub input: Option<serde_json::Value>,
    pub is_error: Option<bool>,
    pub content: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Default)]
struct CollectorInner {
    stream_buffer: String,
    last_assistant_text: Option<String>,
    tool_calls: Vec<HeadlessToolCall>,
    max_iteration_seen: usize,
    tokens: HeadlessTokenUsage,
    outcome: Option<ChatTurnOutcome>,
    total_cost_usd: f64,
    permission_denial_count: usize,
    streamed_model: Option<String>,
    execution_error: Option<String>,
    stream_messages: Vec<serde_json::Value>,
}

fn extract_model_from_thinking_blocks(content: &serde_json::Value) -> Option<String> {
    let thinking_blocks = content
        .get("_thinking_blocks")
        .or_else(|| content.get("thinkingBlocks"))
        .and_then(|value| value.as_array())
        .or_else(|| content.get("thinking").and_then(|value| value.as_array()))?;

    for block in thinking_blocks {
        let model = block
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .or_else(|| {
                block
                    .get("modelName")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
            })
            .or_else(|| {
                block
                    .get("source")
                    .and_then(|source| source.get("model"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
            })
            .or_else(|| {
                block
                    .get("source")
                    .and_then(|source| source.get("modelName"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
            });
        if let Some(model) = model {
            return Some(model.to_string());
        }
    }

    None
}

#[derive(Default)]
struct HeadlessEventCollector {
    inner: Mutex<CollectorInner>,
    stream_event_sink: Option<HeadlessStreamEventSink>,
}

impl HeadlessEventCollector {
    fn reset(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        *guard = CollectorInner::default();
    }

    fn output(&self) -> HeadlessRunOutput {
        let guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let answer = guard
            .last_assistant_text
            .clone()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| guard.stream_buffer.clone())
            .trim()
            .to_string();
        HeadlessRunOutput {
            answer,
            tool_calls: guard.tool_calls.clone(),
            iterations: guard.max_iteration_seen,
            tokens: guard.tokens.clone(),
            total_cost_usd: guard.total_cost_usd,
            streamed_model: guard.streamed_model.clone(),
            execution_error: guard.execution_error.clone(),
            is_error: guard
                .outcome
                .as_ref()
                .map(|outcome| outcome.is_error())
                .unwrap_or(false),
            error_subtype: guard.outcome.as_ref().and_then(outcome_to_error_subtype),
            stream_messages: guard.stream_messages.clone(),
            uuid: uuid::Uuid::new_v4().to_string(),
            ..HeadlessRunOutput::default()
        }
    }

    fn upsert_tool_start(&self, id: &str, name: &str, input: serde_json::Value) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = guard.tool_calls.iter_mut().find(|call| call.id == id) {
            existing.name = name.to_string();
            existing.input = Some(input);
            return;
        }
        guard.tool_calls.push(HeadlessToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input: Some(input),
            is_error: None,
            content: None,
            duration_ms: None,
        });
    }

    fn upsert_tool_done(
        &self,
        id: &str,
        name: &str,
        is_error: bool,
        content: String,
        duration_ms: Option<u64>,
    ) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = guard.tool_calls.iter_mut().find(|call| call.id == id) {
            existing.name = name.to_string();
            existing.is_error = Some(is_error);
            existing.content = Some(content);
            existing.duration_ms = duration_ms;
            return;
        }
        guard.tool_calls.push(HeadlessToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input: None,
            is_error: Some(is_error),
            content: Some(content),
            duration_ms,
        });
    }

    fn note_iteration(&self, iteration: u32) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.max_iteration_seen = guard.max_iteration_seen.max(iteration as usize + 1);
    }

    fn emit_stream_message(&self, message: serde_json::Value) {
        if let Some(sink) = &self.stream_event_sink {
            sink(message);
        }
    }

    fn update_streamed_model_from_content(&self, content: &serde_json::Value) {
        if let Some(model) = extract_model_from_thinking_blocks(content) {
            let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            if guard.streamed_model.is_none() {
                guard.streamed_model = Some(model);
            }
        }
    }

    fn push_assistant_message(
        &self,
        session_id: &str,
        message_id: &str,
        content: &serde_json::Value,
        tool_calls: Option<&[serde_json::Value]>,
    ) {
        self.update_streamed_model_from_content(content);
        let text = content
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(serde_json::json!({ "type": "text", "text": text }));
        }
        for call in tool_calls.unwrap_or(&[]) {
            let id = call
                .get("id")
                .or_else(|| call.get("toolCallId"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let name = call
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if id.is_empty() || name.is_empty() {
                continue;
            }
            let input = call
                .get("input")
                .or_else(|| call.get("arguments"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }));
        }
        if blocks.is_empty() {
            return;
        }
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.stream_messages.push(serde_json::json!({
            "type": "assistant",
            "message": { "role": "assistant", "content": blocks },
            "parent_tool_use_id": null,
            "uuid": message_id,
            "session_id": session_id,
        }));
        self.emit_stream_message(guard.stream_messages.last().cloned().unwrap_or_else(|| {
            serde_json::json!({
                "type": "assistant",
                "message": { "role": "assistant", "content": blocks },
                "parent_tool_use_id": null,
                "uuid": message_id,
                "session_id": session_id,
            })
        }));
    }

    fn push_tool_result_message(
        &self,
        session_id: &str,
        msg_id: &str,
        tool_call_id: &str,
        content: &str,
        is_error: bool,
    ) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.stream_messages.push(serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": content,
                    "is_error": is_error,
                }],
            },
            "parent_tool_use_id": null,
            "uuid": msg_id,
            "session_id": session_id,
        }));
        self.emit_stream_message(guard.stream_messages.last().cloned().unwrap_or_else(|| {
            serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                        "is_error": is_error,
                    }],
                },
                "parent_tool_use_id": null,
                "uuid": msg_id,
                "session_id": session_id,
            })
        }));
    }
}

#[async_trait]
impl RuntimeEventSubscriber for HeadlessEventCollector {
    async fn on_event(&self, event: &RuntimeEvent) -> anyhow::Result<()> {
        match &event.kind {
            RuntimeEventKind::StreamDone => {
                let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                guard.execution_error = None;
            }
            RuntimeEventKind::StreamDelta { content } => {
                let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                guard.stream_buffer.push_str(content);
            }
            RuntimeEventKind::MessagePersisted {
                message_id,
                role,
                content,
                tool_calls,
                error,
                ..
            } if role == "assistant" => {
                if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
                    let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                    guard.last_assistant_text = Some(text.to_string());
                }
                if let Some(error) = error {
                    let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                    guard.execution_error = Some(readable_message_error(error));
                    guard.outcome = Some(outcome_from_message_error(error));
                }
                self.push_assistant_message(
                    event.session_id.as_str(),
                    message_id,
                    content,
                    tool_calls.as_deref(),
                );
            }
            RuntimeEventKind::ToolCallExecuting {
                tool_call_id,
                tool_name,
                input,
            } => {
                self.upsert_tool_start(tool_call_id.as_str(), tool_name, input.clone());
            }
            RuntimeEventKind::ToolCallCompleted {
                tool_call_id,
                tool_name,
                is_error,
                content,
                msg_id,
                duration_ms,
                ..
            } => {
                self.upsert_tool_done(
                    tool_call_id.as_str(),
                    tool_name,
                    *is_error,
                    content.clone(),
                    *duration_ms,
                );
                self.push_tool_result_message(
                    event.session_id.as_str(),
                    msg_id,
                    tool_call_id.as_str(),
                    content,
                    *is_error,
                );
            }
            RuntimeEventKind::TurnStageChanged { stage, .. } => match stage {
                TurnStage::WaitingLlm { iteration }
                | TurnStage::Streaming { iteration }
                | TurnStage::Tools { iteration, .. } => self.note_iteration(*iteration),
                _ => {}
            },
            RuntimeEventKind::TurnCompleted {
                outcome,
                total_input_tokens,
                total_output_tokens,
                total_cache_creation_input_tokens,
                total_cache_read_input_tokens,
                total_cost_usd,
                permission_denial_count,
                ..
            } => {
                let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                guard.outcome = Some(outcome.clone());
                guard.tokens = HeadlessTokenUsage {
                    input: *total_input_tokens,
                    output: *total_output_tokens,
                    cache_creation_input: *total_cache_creation_input_tokens,
                    cache_read_input: *total_cache_read_input_tokens,
                };
                guard.total_cost_usd = total_cost_usd.unwrap_or(0.0);
                guard.permission_denial_count = *permission_denial_count;
                guard.execution_error = None;
            }
            RuntimeEventKind::StreamError { error, .. } => {
                let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                guard.execution_error = Some(error.clone());
                guard.outcome = Some(ChatTurnOutcome::ExecutionError {
                    message: error.clone(),
                });
            }
            _ => {}
        }
        Ok(())
    }
}

pub struct HeadlessDriver {
    chat_runtime: HeadlessChatRuntime,
    collector: Arc<HeadlessEventCollector>,
    session_id: SessionId,
    db: Arc<AppStorage>,
    authorized_workspace_store: Arc<dyn AuthorizedWorkspaceStore>,
    authorized_workspace: AuthorizedWorkspace,
    permission_mode: PermissionMode,
    requested_model: String,
    model: String,
    workspace: PathBuf,
    tools: Vec<String>,
}

impl HeadlessDriver {
    pub async fn run_agent_prompt(
        &self,
        request: HeadlessAgentRequest,
    ) -> Result<HeadlessRunOutput> {
        let started_at = Instant::now();
        self.ensure_conversation()?;
        self.authorized_workspace_store
            .replace_for_session(self.session_id.as_str(), &self.authorized_workspace)
            .with_context(|| {
                format!(
                    "failed to authorize workspace {}",
                    self.authorized_workspace.root_path.display()
                )
            })?;

        let mut chat_request =
            ChatTurnRequest::new(self.session_id.clone(), request.prompt, Vec::new());
        chat_request.permission_mode = self.permission_mode;

        self.collector.reset();
        self.chat_runtime
            .run_chat_request(chat_request)
            .await
            .map_err(anyhow::Error::msg)?;

        let mut output = self.decorate_output(self.collector.output());
        output = self.enrich_output_from_db(output);
        output.duration_ms = started_at.elapsed().as_millis() as u64;
        Ok(output)
    }

    pub fn collect_output(&self) -> HeadlessRunOutput {
        let output = self.decorate_output(self.collector.output());
        self.enrich_output_from_db(output)
    }

    fn decorate_output(&self, mut output: HeadlessRunOutput) -> HeadlessRunOutput {
        let execution_model = output
            .streamed_model
            .as_ref()
            .filter(|model| !model.trim().is_empty())
            .cloned()
            .or_else(|| {
                if self.model.trim().is_empty() {
                    None
                } else {
                    Some(self.model.clone())
                }
            })
            .unwrap_or_default();

        if output.model.trim().is_empty() {
            output.model = execution_model.clone();
        }
        output.requested_model = if output.requested_model.trim().is_empty() {
            self.requested_model.clone()
        } else {
            output.requested_model.clone()
        };
        output.execution_model = if output.execution_model.trim().is_empty() {
            output.model.clone()
        } else {
            output.execution_model.clone()
        };
        output.session_id = self.session_id.as_str().to_string();
        output.permission_mode = permission_mode_to_external(self.permission_mode).to_string();
        output.workspace = self.workspace.to_string_lossy().into_owned();
        output.tools = self.tools.clone();
        if let Some(error) = output.execution_error.clone() {
            output.execution_error = Some(error);
            output.is_error = true;
        }
        output
    }

    fn enrich_output_from_db(&self, mut output: HeadlessRunOutput) -> HeadlessRunOutput {
        let Ok(messages) = self.db.get_messages_v2(self.session_id.as_str()) else {
            return output;
        };

        let hit_max_iterations = output.error_subtype.as_deref() == Some("error_max_turns")
            || latest_assistant_message(&messages)
                .and_then(|message| message.error.as_ref())
                .map(is_max_iterations_error)
                .unwrap_or(false);

        if let Some(message) = answer_assistant_message(&messages, hit_max_iterations) {
            if output.answer.trim().is_empty() || hit_max_iterations {
                let text = message.text().trim();
                if !text.is_empty() {
                    output.answer = text.to_string();
                }
            }
        }

        if let Some(message) = latest_assistant_message(&messages) {
            let model = extract_model_from_thinking_blocks(&message.content)
                .or_else(|| latest_assistant_model(&messages));

            if let Some(model) = model {
                output.execution_model = model.clone();
                output.model = model;
            }

            if let Some(error) = message.error.as_ref() {
                output.is_error = true;
                output.error_subtype = Some(error_subtype_from_message_error(error));
                output.execution_error = Some(readable_message_error(error));
            }
        }

        if output.answer.trim().is_empty() {
            if let Some(last_message) = messages.iter().rev().find(|message| {
                matches!(message.role.as_str(), "assistant" | "tool")
                    && !message.text().trim().is_empty()
            }) {
                output.answer = last_message.text().trim().to_string();
                if last_message.role == "tool" && !output.is_error {
                    output.is_error = true;
                    output.error_subtype = Some("error_incomplete_turn".to_string());
                    output.execution_error = Some(
                        "turn ended before a final assistant message was produced".to_string(),
                    );
                }
            }
        }

        output
    }

    fn ensure_conversation(&self) -> Result<()> {
        match self.db.get_conversation(self.session_id.as_str()) {
            Ok(_) => Ok(()),
            Err(_) => self
                .db
                .create_conversation(self.session_id.as_str(), "CLI")
                .with_context(|| {
                    format!(
                        "failed to create CLI conversation {}",
                        self.session_id.as_str()
                    )
                }),
        }
    }
}

fn latest_assistant_message(messages: &[StoredMessage]) -> Option<&StoredMessage> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
}

fn answer_assistant_message(
    messages: &[StoredMessage],
    hit_max_iterations: bool,
) -> Option<&StoredMessage> {
    let latest = latest_assistant_message(messages)?;
    if hit_max_iterations {
        return messages
            .iter()
            .rev()
            .find(|message| {
                let text = message.text();
                let text = text.trim();
                message.role == "assistant"
                    && !text.is_empty()
                    && !is_max_iterations_notice_text(text)
                    && !message
                        .error
                        .as_ref()
                        .map(is_max_iterations_error)
                        .unwrap_or(false)
            })
            .or(Some(latest));
    }
    Some(latest)
}

fn latest_assistant_model(messages: &[StoredMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == "assistant")
        .find_map(|message| extract_model_from_thinking_blocks(&message.content))
}

fn is_max_iterations_error(error: &MessageError) -> bool {
    matches!(&error.kind, ErrorKind::MaxIterations)
}

fn is_max_iterations_notice_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("处理上限")
        || lower.contains("max_iterations")
        || lower.contains("max iterations")
}

fn readable_message_error(error: &MessageError) -> String {
    error
        .raw
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or(error.message.as_str())
        .to_string()
}

fn outcome_from_message_error(error: &MessageError) -> ChatTurnOutcome {
    match &error.kind {
        ErrorKind::MaxIterations => ChatTurnOutcome::MaxIterationsReached { iterations: 0 },
        ErrorKind::BudgetExceeded => ChatTurnOutcome::BudgetExceeded {
            reason: readable_message_error(error),
            total_cost_usd: 0.0,
        },
        _ => ChatTurnOutcome::ExecutionError {
            message: readable_message_error(error),
        },
    }
}

fn error_subtype_from_message_error(error: &MessageError) -> String {
    match &error.kind {
        ErrorKind::MaxIterations => "error_max_turns",
        ErrorKind::BudgetExceeded => "error_max_budget_usd",
        ErrorKind::PromptTooLong => "error_prompt_too_long",
        ErrorKind::AuthFailed => "error_auth_failed",
        ErrorKind::RateLimited => "error_rate_limited",
        ErrorKind::ChunkTimeout | ErrorKind::Network => "error_network",
        ErrorKind::ExecutionError | ErrorKind::Unknown => "error_during_execution",
    }
    .to_string()
}

#[cfg(test)]
mod headless_output_tests {
    use super::*;

    fn stored_message(role: &str, text: &str, error: Option<MessageError>) -> StoredMessage {
        StoredMessage {
            seq: None,
            rev: None,
            id: format!("msg-{}", uuid::Uuid::new_v4()),
            conversation_id: "cli-test".to_string(),
            role: role.to_string(),
            content: serde_json::json!({ "text": text }),
            created_at: "2026-06-08T00:00:00Z".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            subtype: None,
            compact_metadata: None,
            is_compact_summary: None,
            run_id: None,
            schema_version: Some(2),
            sequence: None,
            error,
        }
    }

    #[test]
    fn max_iterations_prefers_last_real_assistant_answer() {
        let messages = vec![
            stored_message("user", "question", None),
            stored_message("assistant", "FINAL ANSWER: 17000", None),
            stored_message(
                "assistant",
                "analysis reached max iterations template",
                Some(MessageError {
                    kind: ErrorKind::MaxIterations,
                    message: "max iterations reached".to_string(),
                    raw: None,
                }),
            ),
        ];

        let answer = answer_assistant_message(&messages, true)
            .expect("answer message")
            .text()
            .to_string();

        assert_eq!(answer, "FINAL ANSWER: 17000");
    }

    #[test]
    fn max_iterations_falls_back_to_template_when_no_real_answer_exists() {
        let messages = vec![
            stored_message("user", "question", None),
            stored_message(
                "assistant",
                "analysis reached max iterations template",
                Some(MessageError {
                    kind: ErrorKind::MaxIterations,
                    message: "max iterations reached".to_string(),
                    raw: None,
                }),
            ),
        ];

        let answer = answer_assistant_message(&messages, true)
            .expect("answer message")
            .text()
            .to_string();

        assert_eq!(answer, "analysis reached max iterations template");
    }

    #[test]
    fn max_iterations_skips_notice_text_without_message_error() {
        let messages = vec![
            stored_message("user", "question", None),
            stored_message("assistant", "FINAL ANSWER: 3", None),
            stored_message(
                "assistant",
                "⚠️ 本步分析较为复杂,已达处理上限(15 次迭代)。",
                None,
            ),
        ];

        let answer = answer_assistant_message(&messages, true)
            .expect("answer message")
            .text()
            .to_string();

        assert_eq!(answer, "FINAL ANSWER: 3");
    }
}

pub async fn run_headless_agent(
    options: HeadlessBuildOptions,
    request: HeadlessAgentRequest,
) -> Result<HeadlessRunOutput> {
    let driver = build_headless_driver(options).await?;
    driver.run_agent_prompt(request).await
}

pub async fn build_headless_driver(options: HeadlessBuildOptions) -> Result<HeadlessDriver> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let add_dirs = normalize_add_dirs(&options.add_dirs)?;
    let workspace = add_dirs
        .first()
        .cloned()
        .context("failed to resolve CLI workspace")?;
    let home = Arc::new(AiJiaHome::from_home());
    home.ensure_dirs()
        .context("failed to create ~/.renlijia directories")?;
    home.ensure_global_dirs()
        .context("failed to create ~/.renlijia global directories")?;
    crate::telemetry::set_diagnostics_workspace(home.root().to_path_buf());
    prompts::init_prompts(home.root(), home.root());

    let secure_storage = SecureStorage::new(&home.crypto_dir())
        .map(Arc::new)
        .map(Some)
        .unwrap_or_else(|err| {
            log::warn!("[cli] SecureStorage unavailable: {}", err);
            None
        });

    crate::storage::data_version::ensure_compatible(home.as_ref(), secure_storage.as_deref());
    if let Err(err) = crate::storage::migration_user_scope::bootstrap_cloud_auth_if_needed(
        home.root(),
        &home.global_dir(),
    ) {
        log::warn!("[cli] cloud auth bootstrap warning: {}", err);
    }

    let global_store = Arc::new(GlobalConfigStore::new(home.global_dir()));
    #[cfg(debug_assertions)]
    {
        let persisted = global_store
            .get_setting(crate::environment::dev::CONFIG_KEY)
            .ok()
            .flatten();
        crate::environment::dev::load(persisted.as_deref());
    }
    let auth_manager = Arc::new(AuthManager::new(
        global_store,
        secure_storage.clone(),
        home.as_ref(),
    ));
    auth_manager.restore().await;
    let auth_info = auth_manager.get_auth_info().await;
    if !auth_info.logged_in {
        anyhow::bail!(
            "not logged in: run the desktop app login first so ~/.renlijia contains a valid JWT"
        );
    }
    let user_scope = auth_info
        .user
        .as_ref()
        .zip(auth_info.tenant.as_ref())
        .map(|(u, t)| UserScope::new(t.id, u.id))
        .context("restored auth is missing user or tenant information")?;

    let current_user_storage = Arc::new(CurrentUserStorage::new(home.clone()));
    run_user_scope_migrations(home.as_ref(), &user_scope)?;
    current_user_storage
        .activate_scope(user_scope)
        .context("failed to activate user-scoped storage")?;
    let user_paths = current_user_storage.resolve_paths();

    let root_db = Arc::new(AppStorage::new(home.root()).context("failed to initialize root db")?);
    let db = current_user_storage.get_or(&root_db);
    let session_id = resolve_headless_session_id(&db, options.session_id, options.continue_latest)?;
    let run_registry = Arc::new(RuntimeRunRegistry::new());
    let gateway = Arc::new(
        LlmGateway::new_with_registry(db.clone(), run_registry)
            .with_auth_manager(auth_manager.clone()),
    );
    let file_mgr = Arc::new(FileManager::new(&workspace));

    let tool_registry = Arc::new(ToolRegistry::new());
    crate::plugin::builtin::tools::register_builtin_tools(&tool_registry).await;
    let permission_store = Arc::new(crate::runtime::store::PermissionStore::with_layer_files(
        Some(workspace.join(".aijia").join("permissions.json")),
        user_paths
            .as_ref()
            .map(|paths| paths.permissions_path())
            .or_else(|| Some(home.permissions_path())),
    ));
    tool_registry
        .set_permission_store(permission_store.clone())
        .await;

    let disk_skill_registry = Arc::new(Mutex::new(load_disk_skill_registry(
        home.as_ref(),
        user_paths.clone(),
    )));
    let skill_enablement_store = Arc::new(
        crate::plugin::skill::enablement::SkillEnablementStore::new(current_user_storage.clone()),
    );
    let skill_market_install_roots = user_paths.as_ref().map(|paths| {
        crate::runtime::tools::builtin::skill_market::HeadlessSkillMarketInstallRoots {
            user_skills_dir: paths.skills_dir(),
            global_skills_dir: home.skills_dir(),
            tmp_dir: home.root().join("tmp"),
        }
    });

    let task_store = Arc::new(AsyncAgentTaskStore::new());
    let task_notification_queue = Arc::new(TaskNotificationQueue::new());
    let team_registry = crate::runtime::agent::TeamRegistry::new();
    let agent_names = crate::runtime::agent::AgentNameRegistry::new();
    let inbox_registry = crate::runtime::agent::InboxRegistry::new();
    let lead_idle = crate::runtime::agent::LeadIdleSupervisor::new();
    let cancellation_registry = crate::runtime::agent::CancellationRegistry::new();
    let user_agents_dir = user_paths.as_ref().map(|paths| paths.agents_dir());
    let agent_registry = Arc::new(
        crate::runtime::agent::registry_loader::load_registry_with_user_dir(
            user_agents_dir.as_deref(),
            None,
        ),
    );
    let (agent_store_path, subagent_transcript_store_dir) = user_paths
        .as_ref()
        .map(|paths| {
            (
                paths.agent_invocations_path(),
                #[allow(deprecated)]
                paths.subagent_transcripts_dir(),
            )
        })
        .unwrap_or_else(|| {
            (
                home.agent_invocations_path(),
                #[allow(deprecated)]
                home.subagent_transcripts_dir(),
            )
        });
    let agent_runtime = Arc::new(
        crate::runtime::agent::AgentRuntime::from_storage(
            agent_store_path,
            subagent_transcript_store_dir,
        )
        .unwrap_or_else(|err| {
            log::warn!("[cli] AgentRuntime storage unavailable: {err}");
            crate::runtime::agent::AgentRuntime::for_test()
        }),
    );

    let authorized_workspace_ref = AuthorizedWorkspaceRef {
        id: "cli-workspace".to_string(),
        root_path: workspace.clone(),
        display_name: workspace
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("workspace")
            .to_string(),
    };
    let authorized_workspace = AuthorizedWorkspace {
        id: authorized_workspace_ref.id.clone(),
        session_id: session_id.clone(),
        root_path: authorized_workspace_ref.root_path.clone(),
        display_name: authorized_workspace_ref.display_name.clone(),
        authorized_at: chrono::Utc::now().to_rfc3339(),
    };
    let authorized_workspace_store = Arc::new(ConvJsonAuthorizedWorkspaceStore {
        storage: db.clone(),
        cus: Some(current_user_storage.clone()),
    });
    if db.get_conversation(session_id.as_str()).is_err() {
        db.create_conversation(session_id.as_str(), "CLI")
            .with_context(|| {
                format!("failed to create CLI conversation {}", session_id.as_str())
            })?;
    }
    authorized_workspace_store
        .replace_for_session(session_id.as_str(), &authorized_workspace)
        .context("failed to initialize CLI authorized workspace")?;

    let mut app_settings_value = load_app_settings(&db, secure_storage.as_deref());
    let requested_model = options
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            let configured_model = effective_model_name(&app_settings_value);
            if configured_model.trim().is_empty() {
                None
            } else {
                Some(configured_model)
            }
        });
    apply_model_override(&mut app_settings_value, options.model.as_deref());
    let model = effective_model_name(&app_settings_value);
    let requested_model = requested_model.unwrap_or_else(|| model.clone());
    let permission_ctx = Arc::new(build_headless_permission_ctx(&add_dirs));
    let bus = RuntimeEventBus::new();
    let collector = Arc::new(HeadlessEventCollector {
        inner: Mutex::new(CollectorInner::default()),
        stream_event_sink: options.stream_event_sink.clone(),
    });
    bus.subscribe(collector.clone());

    let chat_runtime = build_headless_chat_runtime(HeadlessChatRuntimeConfig {
        cus: current_user_storage.clone(),
        root_db,
        gateway,
        file_mgr,
        crypto: secure_storage,
        tool_registry,
        auth_manager,
        skill_registry: disk_skill_registry,
        skill_enablement_store: Some(skill_enablement_store),
        skill_market_install_roots,
        permission_store,
        authorized_workspace_store: authorized_workspace_store.clone(),
        default_folder: workspace.clone(),
        permission_ctx,
        task_store,
        task_notification_queue,
        team_registry,
        agent_names,
        inbox_registry,
        lead_idle,
        cancellation_registry,
        agent_registry,
        agent_runtime,
        user_scoped_path_resolver: current_user_storage.clone() as Arc<dyn UserScopedPathResolver>,
        system_prompt_override: options.system_prompt,
        max_iterations_override: options.max_turns,
        model_override: options.model,
        bus,
    });
    let tools = chat_runtime.visible_tool_names(session_id.as_str()).await;

    Ok(HeadlessDriver {
        chat_runtime,
        collector,
        session_id,
        db,
        authorized_workspace_store,
        authorized_workspace,
        permission_mode: options.permission_mode,
        requested_model,
        model,
        workspace,
        tools,
    })
}

fn normalize_add_dirs(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let raw = if paths.is_empty() {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    } else {
        paths.to_vec()
    };
    raw.iter()
        .map(|path| normalize_workspace(path))
        .collect::<Result<Vec<_>>>()
}

fn build_headless_permission_ctx(add_dirs: &[PathBuf]) -> ToolPermissionContext {
    let mut ctx = ToolPermissionContext::empty();
    for dir in add_dirs.iter().skip(1) {
        ctx.additional_working_dirs
            .insert(dir.clone(), RuleSource::Session);
    }
    ctx
}

fn resolve_headless_session_id(
    db: &AppStorage,
    session_id: Option<SessionId>,
    continue_latest: bool,
) -> Result<SessionId> {
    if let Some(session_id) = session_id {
        return Ok(session_id);
    }
    if continue_latest {
        let latest = db
            .get_conversations()
            .context("failed to read conversations for --continue")?
            .into_iter()
            .filter_map(|conv| {
                let id = conv.get("id")?.as_str()?.to_string();
                let updated_at = conv
                    .get("updatedAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                Some((updated_at, id))
            })
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, id)| id);
        if let Some(id) = latest {
            return Ok(SessionId::new(id));
        }
    }
    Ok(SessionId::new(format!("cli-{}", uuid::Uuid::new_v4())))
}

fn normalize_workspace(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create workspace {}", path.display()))?;
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize workspace {}", path.display()))
}

fn run_user_scope_migrations(home: &AiJiaHome, scope: &UserScope) -> Result<()> {
    let user_dir = home.user_dir(scope);
    if let Err(err) = crate::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        home.root(),
        &user_dir,
        &scope.key(),
        &home.global_state_path(),
    ) {
        log::warn!("[cli] user-scope migration warning: {}", err);
    }
    if let Err(err) = crate::storage::migration_user_scope::migrate_legacy_config_if_needed(
        home.root(),
        &user_dir,
        &home.global_dir(),
    ) {
        log::warn!("[cli] config migration warning: {}", err);
    }
    if let Err(err) = crate::storage::migration_user_scope::migrate_legacy_turn_stages_if_needed(
        home.root(),
        &user_dir,
    ) {
        log::warn!("[cli] turn-stage migration warning: {}", err);
    }
    if let Err(err) = crate::storage::migration_root_cleanup::cleanup_legacy_root_if_claimed(
        home.root(),
        &home.global_state_path(),
    ) {
        log::warn!("[cli] legacy-root cleanup warning: {}", err);
    }
    Ok(())
}

fn load_disk_skill_registry(
    home: &AiJiaHome,
    user_paths: Option<UserScopedPaths>,
) -> crate::plugin::skill::registry::SkillRegistry {
    let global_skills_dir = home.skills_dir();
    let tagged_roots = match user_paths {
        Some(paths) => vec![
            (
                paths.skills_dir(),
                crate::plugin::skill::types::SkillSource::User,
            ),
            (
                global_skills_dir,
                crate::plugin::skill::types::SkillSource::Global,
            ),
        ],
        None => vec![(
            global_skills_dir,
            crate::plugin::skill::types::SkillSource::Global,
        )],
    };
    let loaded = crate::plugin::skill::loader::load_skill_roots_tagged(&tagged_roots)
        .unwrap_or_else(|err| {
            log::warn!("[cli] failed to load skills: {}", err);
            Default::default()
        });
    crate::plugin::skill::registry::SkillRegistry::from_skills(loaded.into_values().collect())
}

fn load_app_settings(db: &AppStorage, secure_storage: Option<&SecureStorage>) -> AppSettings {
    let global_settings_map = db.get_all_settings().unwrap_or_default();
    let global_settings = if global_settings_map.is_empty() {
        AppSettings::default()
    } else {
        AppSettings::from_string_map(&global_settings_map)
    };
    let workspace_path = global_settings.workspace_path.trim().to_string();
    let effective_settings_map = if workspace_path.is_empty() {
        global_settings_map
    } else {
        db.get_effective_settings(Some(Path::new(&workspace_path)))
            .unwrap_or(global_settings_map)
    };
    let mut settings = if effective_settings_map.is_empty() {
        AppSettings::default()
    } else {
        AppSettings::from_string_map(&effective_settings_map)
    };
    if let Some(storage) = secure_storage {
        settings.primary_api_key = decrypt_api_key(storage, &settings.primary_api_key);
    }
    settings
}

fn apply_model_override(settings: &mut AppSettings, model: Option<&str>) {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return;
    };
    settings.primary_model = model.to_string();
    settings.cloud_model = model.to_string();
    settings.custom_model_name = model.to_string();
}

fn effective_model_name(settings: &AppSettings) -> String {
    if !settings.cloud_model.trim().is_empty() {
        settings.cloud_model.clone()
    } else if !settings.custom_model_name.trim().is_empty() {
        settings.custom_model_name.clone()
    } else {
        settings.primary_model.clone()
    }
}

fn permission_mode_to_external(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Default => "default",
        PermissionMode::Plan => "plan",
        PermissionMode::DontAsk => "dontAsk",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::FullAccess => "fullAccess",
    }
}

fn outcome_to_error_subtype(outcome: &ChatTurnOutcome) -> Option<String> {
    match outcome {
        ChatTurnOutcome::MaxIterationsReached { .. } => Some("error_max_turns".to_string()),
        ChatTurnOutcome::BudgetExceeded { .. } => Some("error_max_budget_usd".to_string()),
        ChatTurnOutcome::ExecutionError { .. } => Some("error_during_execution".to_string()),
        ChatTurnOutcome::Cancelled => Some("error_during_execution".to_string()),
        ChatTurnOutcome::Success => None,
    }
}

fn decrypt_api_key(storage: &SecureStorage, value: &str) -> String {
    if value.is_empty() || !value.contains(':') {
        return value.to_string();
    }
    storage.decrypt(value).unwrap_or_default()
}
