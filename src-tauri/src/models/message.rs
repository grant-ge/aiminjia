#![allow(dead_code)]

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
    pub search_sources: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_summary: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reports: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_files: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_envelope: Option<SubAgentEnvelopePayload>,
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
        aijia_home: &std::path::Path,
        conversation_id: &str,
    ) -> anyhow::Result<Vec<Self>> {
        let store = crate::runtime::task::FileTaskV2Store::new(aijia_home.to_path_buf());
        store
            .list(conversation_id)
            .map(|records| records.into_iter().map(Into::into).collect())
    }
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
