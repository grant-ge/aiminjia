use std::sync::Arc;

use crate::llm::gateway::LlmGateway;
use crate::models::message::SubAgentTranscriptEntryFrontend;
use crate::runtime::agent::subagent_result_envelope::SubAgentResultEnvelope;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::store::conversation_store::ConversationStore;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteConversationOutcome {
    pub conversation_id: String,
    pub cancelled_active_agent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameConversationOutcome {
    pub conversation_id: String,
    pub new_title: String,
}

pub async fn stop_streaming(
    gateway: Arc<LlmGateway>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    conversation_id: String,
) -> Result<(), String> {
    if let Some(run_id) = gateway.active_run_id(&conversation_id) {
        let _ = session_mgr.interrupt_run(&run_id).await;
    } else {
        let _ = session_mgr.interrupt(&conversation_id).await;
    }
    gateway
        .cancel_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn get_messages(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let messages = db
        .get_messages(&conversation_id)
        .map_err(|e| e.to_string())?;
    Ok(messages
        .into_iter()
        .map(transform_message_json_for_frontend)
        .collect())
}

pub async fn get_subagent_transcript(
    runtime: Arc<AgentRuntime>,
    transcript_ref: String,
) -> Result<Vec<SubAgentTranscriptEntryFrontend>, String> {
    let entries = runtime
        .transcript_store_get(&transcript_ref)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("missing subagent transcript: {transcript_ref}"))?;

    Ok(entries
        .into_iter()
        .map(SubAgentTranscriptEntryFrontend::from)
        .collect())
}

pub fn transform_message_json_for_frontend(mut message: serde_json::Value) -> serde_json::Value {
    let Some(content) = message
        .get_mut("content")
        .and_then(|value| value.as_object_mut())
    else {
        return message;
    };

    let Some(raw_text) = content
        .get("text")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return message;
    };

    let Some(envelope) = SubAgentResultEnvelope::from_storage_summary(&raw_text) else {
        return message;
    };

    content.remove("text");
    content.insert(
        "subagentEnvelope".to_string(),
        serde_json::to_value(crate::models::message::SubAgentEnvelopePayload::from(
            envelope,
        ))
        .unwrap_or(serde_json::Value::Null),
    );

    message
}

pub async fn create_conversation(db: Arc<dyn ConversationStore>) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.create_conversation(&id, "New Conversation")
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn get_conversation_model_override(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<Option<String>, String> {
    db.get_conversation_model_override(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn set_conversation_model_override(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
    model_override: Option<String>,
) -> Result<(), String> {
    db.set_conversation_model_override(&conversation_id, model_override)
        .map_err(|e| e.to_string())
}

pub async fn delete_conversation(
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    conversation_id: String,
) -> Result<DeleteConversationOutcome, String> {
    session_mgr.destroy(&conversation_id).await;
    if let Some(run_id) = gateway.active_run_id(&conversation_id) {
        session_mgr.destroy_run(&run_id).await;
    }

    let was_busy = gateway.is_conversation_busy(&conversation_id);
    if was_busy {
        log::info!(
            "delete_conversation: cancelling active agent for conversation {}",
            conversation_id
        );
        gateway.cancel_conversation(&conversation_id).ok();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        gateway.clear_task(&conversation_id);
        db.remove_active_task(&conversation_id).ok();
    }

    let file_paths = db
        .get_file_paths_for_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;

    let mut deleted = 0usize;
    let mut failed = 0usize;
    for path in &file_paths {
        let full_path = file_mgr.full_path(path);
        match std::fs::remove_file(&full_path) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                log::warn!("Failed to delete file {:?}: {}", full_path, e);
                failed += 1;
            }
        }
    }
    if !file_paths.is_empty() {
        log::info!(
            "Conversation {} file cleanup: {} deleted, {} failed, {} already gone",
            conversation_id,
            deleted,
            failed,
            file_paths.len() - deleted - failed
        );
    }

    let _ = db.delete_memories_by_prefix(&format!("loaded:{}:", conversation_id));
    let _ = db.delete_memories_by_prefix(&format!("note:{}:", conversation_id));

    db.delete_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;

    Ok(DeleteConversationOutcome {
        conversation_id,
        cancelled_active_agent: was_busy,
    })
}

pub async fn rename_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
    new_title: String,
) -> Result<RenameConversationOutcome, String> {
    db.rename_conversation(&conversation_id, &new_title)
        .map_err(|e| e.to_string())?;
    Ok(RenameConversationOutcome {
        conversation_id,
        new_title,
    })
}

pub async fn get_conversations(
    db: Arc<dyn ConversationStore>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_conversations().map_err(|e| e.to_string())
}

pub async fn archive_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<(), String> {
    db.archive_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn get_archived_conversations(
    db: Arc<dyn ConversationStore>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_archived_conversations().map_err(|e| e.to_string())
}
