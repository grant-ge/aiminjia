use app_lib::runtime::agent::subagent_result_envelope::build_subagent_transcript_ref;
use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};
use tempfile::TempDir;

#[tokio::test]
async fn parent_can_load_transcript_entries_via_child_run_id() {
    let temp = TempDir::new().unwrap();
    let runtime = AgentRuntime::from_storage(
        temp.path().join("agent_invocations.json"),
        temp.path().join("subagent_transcripts"),
    )
    .unwrap();

    let parent_run_id = RunId::new("run-parent-i6");
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

    let summary = format!(
        "subagent-envelope:v1:{{\"schemaVersion\":1,\"output\":\"done\",\"iterationsUsed\":1,\"transcriptRef\":\"{}\"}}",
        transcript_ref
    );

    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some(&summary),
            Some(&transcript_ref),
            SessionId::new("sess-i6"),
            parent_run_id,
            RuntimeEventBus::new(),
        )
        .await
        .unwrap();

    let loaded_ref = runtime
        .get_transcript_ref(handle.child_run_id())
        .await
        .unwrap();
    let loaded = runtime
        .load_transcript(handle.child_run_id())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded_ref.as_deref(), Some(transcript_ref.as_str()));
    assert_eq!(loaded[0].content, "done");
}
