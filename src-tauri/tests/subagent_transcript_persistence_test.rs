use app_lib::runtime::agent::subagent_result_envelope::{
    build_subagent_transcript_ref, SubAgentResultEnvelope,
};
use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};
use tempfile::TempDir;

#[test]
fn transcript_ref_builder_uses_subagent_scheme() {
    assert_eq!(
        build_subagent_transcript_ref("child-run-42"),
        "subagent://child-run-42"
    );
}

#[tokio::test]
async fn background_completion_persists_summary_and_transcript_ref_together() {
    let temp = TempDir::new().unwrap();
    let runtime = AgentRuntime::from_storage(
        temp.path().join("agent_invocations.json"),
        temp.path().join("subagent_transcripts"),
    )
    .unwrap();
    let bus = RuntimeEventBus::new();
    let session_id = SessionId::new("sess-i5");
    let parent_run_id = RunId::new("run-parent-i5");

    let mut request = SpawnChildRunRequest::for_test(parent_run_id.clone());
    request.background = true;
    let handle = runtime.spawn_child_run(request).await.unwrap();

    let transcript_ref = build_subagent_transcript_ref(handle.child_run_id().as_str());
    runtime
        .store_transcript(
            &transcript_ref,
            &[SubagentTranscriptEntryRecord {
                role: "assistant".to_string(),
                content: "done".to_string(),
                tool_call_id: None,
                tool_name: None,
            }],
        )
        .unwrap();

    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "done".to_string(),
        iterations_used: 1,
        generated_files: Vec::new(),
        terminal_tool_results: Vec::new(),
        transcript_snapshot: Vec::new(),
        transcript_ref: Some(transcript_ref.clone()),
        terminal_stop_reason: None,
        max_tokens_recovery_attempts: 0,
    };
    let summary = envelope.to_storage_summary();

    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some(&summary),
            Some(&transcript_ref),
            session_id,
            parent_run_id,
            bus,
        )
        .await
        .unwrap();

    let stored_summary = runtime
        .get_summary(handle.child_run_id())
        .await
        .unwrap()
        .unwrap();
    let decoded = SubAgentResultEnvelope::from_storage_summary(&stored_summary).unwrap();
    let stored_ref = runtime
        .get_transcript_ref(handle.child_run_id())
        .await
        .unwrap();

    assert_eq!(
        decoded.transcript_ref.as_deref(),
        Some(transcript_ref.as_str())
    );
    assert_eq!(stored_ref.as_deref(), Some(transcript_ref.as_str()));
}
