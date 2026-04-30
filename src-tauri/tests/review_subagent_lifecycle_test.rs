use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::{AgentRuntime, ResumeChildRunRequest, SpawnChildRunRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::ids::{RunId, SessionId};

async fn spawn(
    runtime: &AgentRuntime,
    parent_run_id: &str,
    background: bool,
) -> app_lib::runtime::agent::ChildRunHandle {
    runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new(parent_run_id),
            background,
            allowed_tools: vec!["dummy".to_string()],
        })
        .await
        .unwrap()
}

fn transcript_entries() -> Vec<SubagentTranscriptEntryRecord> {
    vec![
        SubagentTranscriptEntryRecord {
            role: "user".to_string(),
            content: "请分析数据".to_string(),
            tool_call_id: None,
            tool_name: None,
        },
        SubagentTranscriptEntryRecord {
            role: "assistant".to_string(),
            content: "分析完成".to_string(),
            tool_call_id: None,
            tool_name: None,
        },
    ]
}

#[tokio::test]
async fn spawn_child_run_creates_running_child_with_independent_ids() {
    let runtime = AgentRuntime::for_test();
    let parent_run_id = RunId::new("parent-run-1");
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest::for_test(parent_run_id.clone()))
        .await
        .unwrap();

    assert!(!handle.agent_id().as_str().is_empty());
    assert!(!handle.child_run_id().as_str().is_empty());
    assert_ne!(handle.child_run_id(), &parent_run_id);
    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "running"
    );
}

#[tokio::test]
async fn complete_run_marks_child_status_completed() {
    let runtime = AgentRuntime::for_test();
    let handle = spawn(&runtime, "parent-complete", false).await;

    runtime.complete_run(handle.child_run_id()).await.unwrap();

    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "completed"
    );
}

#[tokio::test]
async fn cancel_run_marks_child_status_cancelled() {
    let runtime = AgentRuntime::for_test();
    let handle = spawn(&runtime, "parent-cancel", false).await;

    runtime
        .cancel_run(handle.child_run_id().clone())
        .await
        .unwrap();

    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "cancelled"
    );
}

#[tokio::test]
async fn fail_run_marks_child_status_failed() {
    let runtime = AgentRuntime::for_test();
    let handle = spawn(&runtime, "parent-fail", false).await;

    runtime.fail_run(handle.child_run_id()).await.unwrap();

    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "failed"
    );
}

#[tokio::test]
async fn missing_child_run_status_returns_missing() {
    let runtime = AgentRuntime::for_test();

    assert_eq!(
        runtime.status(&RunId::new("nonexistent")).await.unwrap(),
        "missing"
    );
}

#[tokio::test]
async fn completing_background_child_run_emits_agent_idle_event() {
    let runtime = AgentRuntime::for_test();
    let handle = spawn(&runtime, "parent-background", true).await;
    let bus = RuntimeEventBus::new();

    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some("summary"),
            Some("subagent://transcript-bg"),
            SessionId::new("session-bg"),
            RunId::new("parent-background"),
            bus.clone(),
        )
        .await
        .unwrap();

    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "completed"
    );
    assert!(bus
        .recorded()
        .iter()
        .any(|event| matches!(event.kind, RuntimeEventKind::AgentIdle { .. })));
}

#[test]
fn stored_transcript_can_be_loaded_by_transcript_ref() {
    let runtime = AgentRuntime::for_test();
    let entries = transcript_entries();

    runtime
        .store_transcript("subagent://transcript-1", &entries)
        .unwrap();
    let loaded = runtime
        .transcript_store_get("subagent://transcript-1")
        .unwrap()
        .unwrap();

    assert_eq!(loaded, entries);
}

#[tokio::test]
async fn child_run_can_resolve_transcript_ref_then_load_transcript() {
    let runtime = AgentRuntime::for_test();
    let handle = spawn(&runtime, "parent-transcript", true).await;
    let entries = transcript_entries();
    runtime
        .store_transcript("subagent://transcript-child", &entries)
        .unwrap();

    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some("summary"),
            Some("subagent://transcript-child"),
            SessionId::new("session-transcript"),
            RunId::new("parent-transcript"),
            RuntimeEventBus::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        runtime
            .get_transcript_ref(handle.child_run_id())
            .await
            .unwrap(),
        Some("subagent://transcript-child".to_string())
    );
    assert_eq!(
        runtime
            .load_transcript(handle.child_run_id())
            .await
            .unwrap(),
        Some(entries)
    );
}

#[tokio::test]
async fn resume_child_run_returns_existing_handle_for_agent_id() {
    let runtime = AgentRuntime::for_test();
    let handle = spawn(&runtime, "parent-resume", false).await;

    let resumed = runtime
        .resume_child_run(ResumeChildRunRequest::new(handle.agent_id().as_str()))
        .await
        .unwrap();

    assert_eq!(resumed.agent_id(), handle.agent_id());
    assert_eq!(resumed.child_run_id(), handle.child_run_id());
}

#[tokio::test]
async fn resume_unknown_agent_id_returns_error() {
    let runtime = AgentRuntime::for_test();

    let err = runtime
        .resume_child_run(ResumeChildRunRequest::new("nonexistent-agent"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("missing invocation"));
}
