use std::sync::Arc;

use app_lib::runtime::agent::subagent_result_envelope::{
    build_subagent_transcript_ref, SubAgentResultEnvelope,
};
use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::AgentRuntime;
use app_lib::runtime::conversation_service;
use app_lib::runtime::events::RuntimeEvent;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::storage::file_store::AppStorage;
use app_lib::transport::tauri_event_adapter::map_runtime_event;
use serde_json::json;
use tempfile::TempDir;

fn sample_envelope() -> SubAgentResultEnvelope {
    SubAgentResultEnvelope {
        schema_version: 1,
        output: "child finished".to_string(),
        iterations_used: 3,
        generated_files: vec!["report.xlsx".to_string(), "chart.png".to_string()],
        terminal_tool_results: Vec::new(),
        transcript_snapshot: Vec::new(),
        transcript_ref: Some(build_subagent_transcript_ref("child-run-42")),
    }
}

#[tokio::test]
async fn get_messages_structures_subagent_envelope_and_hides_raw_text() {
    let temp = TempDir::new().unwrap();
    let db = Arc::new(AppStorage::new(temp.path()).unwrap());

    let conversation_id = conversation_service::create_conversation(db.clone())
        .await
        .unwrap();
    let summary = sample_envelope().to_storage_summary();

    db.insert_message(
        "msg-subagent-1",
        &conversation_id,
        "assistant",
        &json!({ "text": summary }).to_string(),
    )
    .unwrap();

    let messages = conversation_service::get_messages(db, conversation_id)
        .await
        .unwrap();

    let content = messages[0].get("content").unwrap();
    assert!(
        content.get("text").is_none(),
        "frontend should not receive the raw prefixed envelope string"
    );
    assert_eq!(
        content["subagentEnvelope"]["output"],
        json!("child finished")
    );
    assert_eq!(content["subagentEnvelope"]["iterationsUsed"], json!(3));
    assert_eq!(
        content["subagentEnvelope"]["generatedFiles"],
        json!(["report.xlsx", "chart.png"])
    );
    assert_eq!(
        content["subagentEnvelope"]["transcriptRef"],
        json!("subagent://child-run-42")
    );
}

#[test]
fn message_updated_payload_structures_subagent_envelope_and_hides_raw_text() {
    let summary = sample_envelope().to_storage_summary();
    let event = RuntimeEvent::message_persisted(
        SessionId::new("conv-subagent"),
        RunId::new("run-subagent"),
        "msg-subagent-evt",
        "assistant",
        json!({ "text": summary }),
    );

    let mapped = map_runtime_event(&event).expect("message:updated event should be emitted");
    let content = mapped.payload.get("content").unwrap();

    assert!(
        content.get("text").is_none(),
        "message:updated should expose structured envelope only"
    );
    assert_eq!(content["subagentEnvelope"]["schemaVersion"], json!(1));
    assert_eq!(
        content["subagentEnvelope"]["transcriptRef"],
        json!("subagent://child-run-42")
    );
}

#[tokio::test]
async fn get_subagent_transcript_returns_frontend_entries_without_tool_call_id() {
    let runtime = Arc::new(AgentRuntime::for_test());
    let transcript_ref = build_subagent_transcript_ref("child-run-99");

    runtime
        .store_transcript(
            &transcript_ref,
            &[
                SubagentTranscriptEntryRecord {
                    role: "assistant".to_string(),
                    content: "Planning".to_string(),
                    tool_call_id: None,
                    tool_name: None,
                },
                SubagentTranscriptEntryRecord {
                    role: "tool".to_string(),
                    content: "Saved report.xlsx".to_string(),
                    tool_call_id: Some("tool-call-1".to_string()),
                    tool_name: Some("bash".to_string()),
                },
            ],
        )
        .unwrap();

    let entries = conversation_service::get_subagent_transcript(runtime, transcript_ref)
        .await
        .unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].tool_name.as_deref(), Some("bash"));

    let serialized = serde_json::to_value(&entries).unwrap();
    assert!(
        serialized[1].get("toolCallId").is_none(),
        "frontend transcript entries should not expose legacy tool_call_id"
    );
}

#[tokio::test]
async fn get_subagent_transcript_errors_when_transcript_ref_is_missing() {
    let err = conversation_service::get_subagent_transcript(
        Arc::new(AgentRuntime::for_test()),
        "subagent://missing".to_string(),
    )
    .await
    .unwrap_err();

    assert!(
        err.contains("missing"),
        "error should mention the missing transcript ref: {err}"
    );
}
