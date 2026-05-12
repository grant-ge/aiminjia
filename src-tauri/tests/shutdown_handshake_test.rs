//! P2.6: shutdown_request handshake — Teammate must NOT self-terminate
//! when it receives a shutdown_request.  It records the request in its
//! transcript (so an LLM turn could pick it up) and stays idle until the
//! Lead explicitly cancels (TaskStop / TeamDelete).

use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

use app_lib::runtime::agent::inbox::{AgentInbox, InboxItem, MessageSource};
use app_lib::runtime::agent::output_writer::{
    AgentTranscriptMeta, TranscriptKind, transcript_path_for_kind,
};
use app_lib::runtime::agent::team::{Member, MemberRole, Team};
use app_lib::runtime::agent::worker_runtime::{run_worker, TeammateWorkerCtx, WorkerMode};
use app_lib::runtime::agent::AgentNameRegistry;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::messaging::StructuredMessage;

fn build_ctx(
    tmp: &TempDir,
    session_id: &str,
    agent_id: &AgentId,
    agent_name: &str,
    inbox: Arc<AgentInbox>,
    cancel: CancellationToken,
) -> (TeammateWorkerCtx, Arc<Mutex<Team>>) {
    let conv_dir = tmp.path().join("conversations").join(session_id);
    let lead = Member {
        agent_id: AgentId::new("lead-id"),
        name: "team-lead".to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    let mut team = Team::new(SessionId::new(session_id), lead, "team".to_string());
    team.add_teammate(Member {
        agent_id: agent_id.clone(),
        name: agent_name.to_string(),
        role: MemberRole::Teammate {
            employee_id: "emp".into(),
            spawned_by: AgentId::new("lead-id"),
        },
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    })
    .unwrap();
    let team_handle = Arc::new(Mutex::new(team));
    let names = AgentNameRegistry::new();
    let meta = AgentTranscriptMeta {
        agent_id: agent_id.as_str().to_string(),
        agent_name: Some(agent_name.to_string()),
        kind: TranscriptKind::Teammate,
        employee_id: Some("emp".into()),
        team_id: Some(session_id.into()),
        spawned_by: Some("lead-id".into()),
        spawned_at: chrono::Utc::now(),
        model: None,
        is_async: true,
        tool_whitelist: vec![],
        boot_system_prompt: None,
    };
    let ctx = TeammateWorkerCtx {
        agent_id: agent_id.clone(),
        session_id: SessionId::new(session_id),
        conv_id: session_id.into(),
        cancel,
        inbox,
        agent_names: names,
        inbox_registry: None,
        cancellation_registry: None,
        conv_dir: Some(conv_dir),
        meta,
        llm_engine: None,
    };
    (ctx, team_handle)
}

#[tokio::test]
async fn shutdown_request_chat_message_does_not_terminate_teammate() {
    let tmp = TempDir::new().unwrap();
    let session = "conv-shutdown-stay";
    let agent_id = AgentId::new("agent-stay");
    let agent_name = "researcher";

    let cancel = CancellationToken::new();
    let inbox = AgentInbox::new(8);
    let (ctx, team) = build_ctx(&tmp, session, &agent_id, agent_name, inbox.clone(), cancel.clone());
    let conv_dir = ctx.conv_dir.clone().unwrap();

    let team_clone = team.clone();
    let agent_name_str = agent_name.to_string();
    let handle = tokio::spawn(async move {
        run_worker(
            WorkerMode::TeammateIdle {
                team_handle: team_clone,
                agent_name: agent_name_str,
            },
            ctx,
            None,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    inbox
        .send(InboxItem::ChatMessage {
            message: StructuredMessage::ShutdownRequest {
                reason: Some("任务完成".into()),
            },
            source: MessageSource::Lead,
        })
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Worker should still be idle — handle not finished.
    assert!(
        !handle.is_finished(),
        "shutdown_request must NOT cause Teammate to self-terminate"
    );

    // Transcript should now contain the shutdown-request wrapper.
    let path = transcript_path_for_kind(&conv_dir, &TranscriptKind::Teammate, agent_id.as_str());
    let body = std::fs::read_to_string(&path).expect("transcript should exist");
    assert!(
        body.contains("shutdown-request"),
        "transcript should record the shutdown-request: {body}"
    );
    assert!(
        body.contains("任务完成"),
        "reason should be preserved in transcript: {body}"
    );

    // Cleanup: cancel + join.
    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .expect("worker should exit after cancel")
        .expect("no panic");
}

#[tokio::test]
async fn explicit_cancel_after_shutdown_request_completes_cleanup() {
    let tmp = TempDir::new().unwrap();
    let session = "conv-shutdown-cancel";
    let agent_id = AgentId::new("agent-cleanup");
    let agent_name = "worker";

    let cancel = CancellationToken::new();
    let inbox = AgentInbox::new(4);
    let (ctx, team) = build_ctx(&tmp, session, &agent_id, agent_name, inbox.clone(), cancel.clone());

    let team_clone = team.clone();
    let agent_name_str = agent_name.to_string();
    let handle = tokio::spawn(async move {
        run_worker(
            WorkerMode::TeammateIdle {
                team_handle: team_clone,
                agent_name: agent_name_str,
            },
            ctx,
            None,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    inbox
        .send(InboxItem::ChatMessage {
            message: StructuredMessage::ShutdownRequest { reason: None },
            source: MessageSource::Lead,
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!handle.is_finished(), "still running after shutdown_request");

    // Now Lead explicitly cancels — Teammate exits and is removed from Team.
    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .expect("must exit within 3s of cancel")
        .expect("no panic");

    let team_guard = team.lock().await;
    assert!(team_guard.find_by_name(agent_name).is_none());
}
