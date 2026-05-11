//! P2.9: plan_approval_request / plan_approval_response round-trip via
//! SendMessage + worker_runtime stub transcript rendering.

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
    let mut team = Team::new(SessionId::new(session_id), lead, "team".into());
    team.add_teammate(Member {
        agent_id: agent_id.clone(),
        name: agent_name.into(),
        role: MemberRole::Teammate {
            employee_id: "emp".into(),
            spawned_by: AgentId::new("lead-id"),
        },
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    })
    .unwrap();
    let team_handle = Arc::new(Mutex::new(team));
    let meta = AgentTranscriptMeta {
        agent_id: agent_id.as_str().into(),
        agent_name: Some(agent_name.into()),
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
        agent_names: AgentNameRegistry::new(),
        inbox_registry: None,
        cancellation_registry: None,
        conv_dir: Some(conv_dir),
        meta,
    };
    (ctx, team_handle)
}

#[tokio::test]
async fn plan_approval_request_renders_xml_in_transcript() {
    let tmp = TempDir::new().unwrap();
    let session = "conv-plan-req";
    let agent_id = AgentId::new("agent-plan-1");
    let agent_name = "researcher";

    let cancel = CancellationToken::new();
    let inbox = AgentInbox::new(4);
    let (ctx, team) = build_ctx(&tmp, session, &agent_id, agent_name, inbox.clone(), cancel.clone());
    let conv_dir = ctx.conv_dir.clone().unwrap();

    let team_clone = team.clone();
    let name_str = agent_name.to_string();
    let handle = tokio::spawn(async move {
        run_worker(
            WorkerMode::TeammateIdle {
                team_handle: team_clone,
                agent_name: name_str,
            },
            ctx,
            None,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    inbox
        .send(InboxItem::ChatMessage {
            message: StructuredMessage::PlanApprovalRequest {
                request_id: "pa-42".into(),
                plan: "执行 rm -rf /tmp/cache 清理".into(),
            },
            source: MessageSource::Lead,
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let path = transcript_path_for_kind(&conv_dir, &TranscriptKind::Teammate, agent_id.as_str());
    let body = std::fs::read_to_string(&path).expect("transcript should exist");
    assert!(body.contains(r#"<plan-approval-request id="pa-42">"#), "{body}");
    assert!(body.contains("rm -rf /tmp/cache"), "{body}");
    assert!(body.contains("plan_approval_response"), "instructions: {body}");

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn plan_approval_response_renders_xml_in_transcript() {
    let tmp = TempDir::new().unwrap();
    let session = "conv-plan-resp";
    let agent_id = AgentId::new("agent-plan-2");
    let agent_name = "researcher";

    let cancel = CancellationToken::new();
    let inbox = AgentInbox::new(4);
    let (ctx, team) = build_ctx(&tmp, session, &agent_id, agent_name, inbox.clone(), cancel.clone());
    let conv_dir = ctx.conv_dir.clone().unwrap();

    let team_clone = team.clone();
    let name_str = agent_name.to_string();
    let handle = tokio::spawn(async move {
        run_worker(
            WorkerMode::TeammateIdle {
                team_handle: team_clone,
                agent_name: name_str,
            },
            ctx,
            None,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    inbox
        .send(InboxItem::ChatMessage {
            message: StructuredMessage::PlanApprovalResponse {
                request_id: "pa-42".into(),
                approve: false,
                feedback: Some("先备份再删".into()),
            },
            source: MessageSource::Lead,
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let path = transcript_path_for_kind(&conv_dir, &TranscriptKind::Teammate, agent_id.as_str());
    let body = std::fs::read_to_string(&path).expect("transcript should exist");
    assert!(
        body.contains(r#"<plan-approval-response id="pa-42" approve="false">"#),
        "{body}"
    );
    assert!(body.contains("先备份再删"), "{body}");

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .unwrap()
        .unwrap();
}
