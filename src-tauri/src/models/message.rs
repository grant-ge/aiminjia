#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runtime::agent::subagent_result_envelope::SubAgentResultEnvelope;
use crate::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub created_at: String,
    pub content: MessageContent,
    /// Sender information (only present for user messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<MessageSender>,
}

/// Information about the message sender (for user messages)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSender {
    /// Display name of the sender
    pub name: String,
    /// Whether the sender was logged in when sending the message
    pub is_logged_in: bool,
}

/// Supports multiple rich content types mixed together.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_blocks: Option<Vec<CodeBlock>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_results: Option<Vec<CodeResult>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub anomalies: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub insights: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_causes: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reports: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_files: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_envelope: Option<SubAgentEnvelopePayload>,

    /// 流式输出状态。仅对 assistant 消息有意义；缺省（None）按 Final 渲染。
    /// 详见 `~/lotus/docs/superpowers/specs/2026-05-22-streaming-partial-preservation.md`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_status: Option<StreamStatus>,

    /// 前端生成的乐观 id，用于 user message 写入幂等去重。
    /// 同一 `clientMessageId` 第二次到达时复用已落库的 message id，不再 append 新行。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
}

/// 流式输出的最终状态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StreamStatus {
    /// 正常完成（缺省视为 Final，跨版本兼容老消息）
    Final,
    /// chunk_timeout / network_flap retry 时持久化的中断 partial
    Incomplete,
    /// retry 全部耗尽后 final-error 标记
    Failed,
    /// 用户主动 stop 中止
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentEnvelopePayload {
    pub schema_version: u32,
    pub output: String,
    pub iterations_used: usize,
    pub generated_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_ref: Option<String>,
}

impl From<SubAgentResultEnvelope> for SubAgentEnvelopePayload {
    fn from(value: SubAgentResultEnvelope) -> Self {
        Self {
            schema_version: value.schema_version,
            output: value.output,
            iterations_used: value.iterations_used,
            generated_files: value.generated_files,
            transcript_ref: value.transcript_ref,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentTranscriptEntryFrontend {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl From<SubagentTranscriptEntryRecord> for SubAgentTranscriptEntryFrontend {
    fn from(value: SubagentTranscriptEntryRecord) -> Self {
        Self {
            role: value.role,
            content: value.content,
            tool_name: value.tool_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecordFrontend {
    pub task_id: String,
    pub session_id: String,
    pub run_id: String,
    pub subject: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

impl From<crate::runtime::task::task_models::TaskRecord> for TaskRecordFrontend {
    fn from(r: crate::runtime::task::task_models::TaskRecord) -> Self {
        use crate::runtime::task::task_models::TaskStatus;
        let status_str = match r.status {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        };
        Self {
            task_id: r.id,
            session_id: r.session_id.as_str().to_string(),
            run_id: r.parent_run_id.as_str().to_string(),
            subject: r.subject,
            status: status_str.to_string(),
            active_form: r.active_form,
            owner: r
                .owner
                .or_else(|| r.owner_agent_id.map(|id| id.as_str().to_string())),
        }
    }
}

impl TaskRecordFrontend {
    pub fn list_from_task_v2_store(
        aijia_home: &Path,
        conversation_id: &str,
    ) -> anyhow::Result<Vec<Self>> {
        let records = list_task_records_with_legacy_root_fallback(aijia_home, conversation_id)?;
        Ok(records.into_iter().map(Into::into).collect())
    }
}

fn list_task_records_with_legacy_root_fallback(
    aijia_home: &Path,
    conversation_id: &str,
) -> anyhow::Result<Vec<crate::runtime::task::task_models::TaskRecord>> {
    // P1.5: primary store is per-conversation; legacy store is the old global tasks/ root.
    let primary_root = aijia_home
        .join("conversations")
        .join(conversation_id)
        .join("tasks");
    let primary_store = crate::runtime::task::FileTaskV2Store::new(primary_root);
    // Empty task_list_id keeps task files flat at <root>/<id>.json — matches
    // the write path in runtime::tools::builtin::task_tools.
    let primary = primary_store.list("")?;

    let Some(legacy_root) = legacy_aijia_root_for_user_scoped_base(aijia_home) else {
        return Ok(primary);
    };

    #[allow(deprecated)]
    let legacy_store = crate::runtime::task::FileTaskV2Store::from_aijia_home(legacy_root);
    let legacy = legacy_store.list(conversation_id)?;
    if legacy.is_empty() {
        return Ok(primary);
    }
    if primary.is_empty() {
        return Ok(legacy);
    }

    let mut by_id = std::collections::HashMap::new();
    for task in legacy.into_iter().chain(primary) {
        by_id.insert(task.id.clone(), task);
    }
    let mut merged = by_id.into_values().collect::<Vec<_>>();
    merged.sort_by_key(|task| task.id.parse::<u64>().unwrap_or(u64::MAX));
    Ok(merged)
}

fn legacy_aijia_root_for_user_scoped_base(base: &Path) -> Option<PathBuf> {
    let users_dir = base.parent()?;
    if users_dir.file_name()?.to_str()? != "users" {
        return None;
    }
    users_dir.parent().map(Path::to_path_buf)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeBlock {
    pub id: String,
    pub language: String,
    pub code: String,
    pub purpose: Option<String>,
    pub status: CodeBlockStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeBlockStatus {
    Pending,
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeResult {
    pub id: String,
    pub code_block_id: String,
    pub output: String,
    pub is_error: bool,
}
