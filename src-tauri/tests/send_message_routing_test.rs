//! P2.2 integration tests for SendMessage routing.

use std::sync::Arc;

use serde_json::json;

use app_lib::runtime::agent::{
    AgentInbox, AgentNameRegistry, InboxItem, InboxRegistry, MessageSource, TeamRegistry,
};
use app_lib::runtime::agent::{Member, MemberRole};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::messaging::StructuredMessage;
use app_lib::runtime::tools::builtin::send_message::SendMessageRuntimeTool;
use app_lib::runtime::tools::builtin::team_tools::LEAD_NAME;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

const TEAM_NAME: &str = "test-team";

struct Fixture {
    team_registry: Arc<TeamRegistry>,
    name_registry: Arc<AgentNameRegistry>,
    inbox_registry: Arc<InboxRegistry>,
    session: SessionId,
}

impl Fixture {
    async fn new_with_team(session_str: &str) -> Self {
        let team_registry = TeamRegistry::new();
        let name_registry = AgentNameRegistry::new();
        let inbox_registry = InboxRegistry::new();
        let session = SessionId::new(session_str);

        let lead = Member {
            agent_id: AgentId::new("lead-id"),
            name: LEAD_NAME.to_string(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        };
        team_registry
            .create(session.clone(), lead, TEAM_NAME.to_string())
            .await
            .unwrap();
        name_registry
            .register(&session, TEAM_NAME, LEAD_NAME, AgentId::new("lead-id"))
            .await
            .unwrap();

        Self {
            team_registry,
            name_registry,
            inbox_registry,
            session,
        }
    }

    async fn add_teammate(&self, name: &str, agent_id: &str) -> Arc<AgentInbox> {
        let id = AgentId::new(agent_id);
        let inbox = AgentInbox::new(8);
        self.name_registry
            .register(&self.session, TEAM_NAME, name, id.clone())
            .await
            .unwrap();
        self.inbox_registry
            .register(&self.session, TEAM_NAME, id.clone(), inbox.clone())
            .await;
        let team = self.team_registry.get(&self.session, TEAM_NAME).await.unwrap();
        let mut t = team.lock().await;
        t.add_teammate(Member {
            agent_id: id,
            name: name.to_string(),
            role: MemberRole::Teammate {
                employee_id: "emp".into(),
                spawned_by: AgentId::new("lead-id"),
            },
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        })
        .unwrap();
        inbox
    }

    fn ctx_for(&self, caller_agent_id: &str) -> ToolExecutionContext {
        let mut ctx = ToolExecutionContext::for_test(self.session.as_str(), "run-1", "tc-1")
            .with_team_registry(self.team_registry.clone())
            .with_agent_names(self.name_registry.clone())
            .with_inbox_registry(self.inbox_registry.clone())
            .with_active_team(TEAM_NAME.to_string());
        ctx.agent_id = Some(AgentId::new(caller_agent_id));
        ctx
    }
}

#[tokio::test]
async fn delivers_text_message_to_named_teammate() {
    let fx = Fixture::new_with_team("conv-deliver").await;
    let inbox = fx.add_teammate("researcher", "agent-r").await;
    let ctx = fx.ctx_for("lead-id");

    SendMessageRuntimeTool
        .execute(
            json!({
                "to": "researcher",
                "message": { "type": "text", "content": "go investigate" }
            }),
            ctx,
        )
        .await
        .unwrap();

    let item = inbox.recv().await.unwrap();
    match item {
        InboxItem::ChatMessage { message, source } => {
            assert_eq!(message.as_text(), Some("go investigate"));
            assert!(matches!(source, MessageSource::Lead));
        }
        other => panic!("unexpected item: {other:?}"),
    }
}

#[tokio::test]
async fn unknown_recipient_fails_with_helpful_message() {
    let fx = Fixture::new_with_team("conv-unknown").await;
    let ctx = fx.ctx_for("lead-id");

    let err = SendMessageRuntimeTool
        .execute(
            json!({
                "to": "ghost",
                "message": { "type": "text", "content": "hi" }
            }),
            ctx,
        )
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(msg.contains("ghost"), "msg should name target: {msg}");
            assert!(
                msg.contains("no agent named") || msg.contains("TeamCreate"),
                "should be remediation-y: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn self_send_is_rejected() {
    let fx = Fixture::new_with_team("conv-self").await;
    // Lead trying to send to itself.
    let ctx = fx.ctx_for("lead-id");

    let err = SendMessageRuntimeTool
        .execute(
            json!({
                "to": LEAD_NAME,
                "message": { "type": "text", "content": "echo" }
            }),
            ctx,
        )
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("self-send"),
                "expected self-send rejection: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn shutdown_request_variant_round_trips_through_inbox() {
    let fx = Fixture::new_with_team("conv-shutdown").await;
    let inbox = fx.add_teammate("worker", "agent-w").await;
    let ctx = fx.ctx_for("lead-id");

    SendMessageRuntimeTool
        .execute(
            json!({
                "to": "worker",
                "message": { "type": "shutdown_request", "reason": "task done" }
            }),
            ctx,
        )
        .await
        .unwrap();

    let item = inbox.recv().await.unwrap();
    match item {
        InboxItem::ChatMessage {
            message,
            source: MessageSource::Lead,
        } => {
            assert!(matches!(
                message,
                StructuredMessage::ShutdownRequest { ref reason }
                    if reason.as_deref() == Some("task done")
            ));
        }
        other => panic!("unexpected item: {other:?}"),
    }
}

#[tokio::test]
async fn broadcast_fans_out_to_teammates_excluding_sender() {
    let fx = Fixture::new_with_team("conv-broadcast").await;
    let inbox_a = fx.add_teammate("alpha", "agent-a").await;
    let inbox_b = fx.add_teammate("beta", "agent-b").await;
    // Sender is alpha; alpha should NOT receive its own broadcast.
    let ctx = fx.ctx_for("agent-a");

    SendMessageRuntimeTool
        .execute(
            json!({
                "to": "*",
                "message": { "type": "text", "content": "all hands" }
            }),
            ctx,
        )
        .await
        .unwrap();

    // alpha sends, alpha does not receive.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), inbox_a.recv())
            .await
            .is_err(),
        "sender alpha should not get its own broadcast"
    );
    // beta receives.
    let item = tokio::time::timeout(std::time::Duration::from_millis(50), inbox_b.recv())
        .await
        .expect("beta should get broadcast")
        .unwrap();
    match item {
        InboxItem::ChatMessage { message, source } => {
            assert_eq!(message.as_text(), Some("all hands"));
            assert!(matches!(source, MessageSource::Teammate(ref n) if n == "alpha"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_message_payload_is_rejected_with_useful_error() {
    let fx = Fixture::new_with_team("conv-bad-msg").await;
    fx.add_teammate("worker", "agent-w").await;
    let ctx = fx.ctx_for("lead-id");

    let err = SendMessageRuntimeTool
        .execute(
            json!({
                "to": "worker",
                "message": { "type": "totally_made_up", "content": "x" }
            }),
            ctx,
        )
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(msg.contains("invalid `message`"), "msg: {msg}");
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}
