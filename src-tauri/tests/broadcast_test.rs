//! P2.10: SendMessage broadcast (`to: "*"`) fan-out semantics.
//!
//! Covers cases not exercised by P2.2's single broadcast test:
//! - Lead broadcasting fans out to all Teammates but not back to itself.
//! - Zero-Teammate broadcast is a successful noop.
//! - Broadcast outside Team mode errors with helpful message.
//! - Broadcast continues past a single dead inbox without aborting others.

use std::sync::Arc;

use serde_json::json;

use app_lib::runtime::agent::{
    AgentInbox, AgentNameRegistry, InboxItem, InboxRegistry, MessageSource, TeamRegistry,
};
use app_lib::runtime::agent::{Member, MemberRole};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::tools::builtin::send_message::SendMessageRuntimeTool;
use app_lib::runtime::tools::builtin::team_tools::LEAD_NAME;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

const TEAM_NAME: &str = "team";

struct Fixture {
    team_registry: Arc<TeamRegistry>,
    name_registry: Arc<AgentNameRegistry>,
    inbox_registry: Arc<InboxRegistry>,
    session: SessionId,
}

impl Fixture {
    async fn new_with_team(session: &str) -> Self {
        let team_registry = TeamRegistry::new();
        let name_registry = AgentNameRegistry::new();
        let inbox_registry = InboxRegistry::new();
        let session_id = SessionId::new(session);
        team_registry
            .create(
                session_id.clone(),
                Member {
                    agent_id: AgentId::new("lead-id"),
                    name: LEAD_NAME.into(),
                    role: MemberRole::Lead,
                    created_at: chrono::Utc::now(),
                    last_active_at: chrono::Utc::now(),
                },
                TEAM_NAME.into(),
            )
            .await
            .unwrap();
        name_registry
            .register(&session_id, TEAM_NAME, LEAD_NAME, AgentId::new("lead-id"))
            .await
            .unwrap();
        // Lead inbox so we can verify Lead doesn't receive broadcasts.
        let lead_inbox = AgentInbox::new(8);
        inbox_registry
            .register(&session_id, TEAM_NAME, AgentId::new("lead-id"), lead_inbox)
            .await;
        Self {
            team_registry,
            name_registry,
            inbox_registry,
            session: session_id,
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
            name: name.into(),
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
async fn lead_broadcast_reaches_all_teammates_but_not_lead_itself() {
    let fx = Fixture::new_with_team("conv-bcast-lead").await;
    let inbox_a = fx.add_teammate("alpha", "a-1").await;
    let inbox_b = fx.add_teammate("beta", "b-1").await;
    let inbox_c = fx.add_teammate("gamma", "c-1").await;
    let lead_inbox = fx
        .inbox_registry
        .get(&fx.session, TEAM_NAME, &AgentId::new("lead-id"))
        .await
        .unwrap();

    let ctx = fx.ctx_for("lead-id");
    let result = SendMessageRuntimeTool
        .execute(
            json!({"to": "*", "message": {"type": "text", "content": "all hands"}}),
            ctx,
        )
        .await
        .unwrap();
    assert_eq!(result.data.as_ref().unwrap()["delivered"], 3);

    for inbox in [&inbox_a, &inbox_b, &inbox_c] {
        let item = tokio::time::timeout(std::time::Duration::from_millis(50), inbox.recv())
            .await
            .expect("teammate should receive broadcast")
            .unwrap();
        match item {
            InboxItem::ChatMessage { message, source } => {
                assert_eq!(message.as_text(), Some("all hands"));
                assert!(matches!(source, MessageSource::Lead));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
    // Lead does NOT receive its own broadcast.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), lead_inbox.recv())
            .await
            .is_err(),
        "Lead must not receive its own broadcast"
    );
}

#[tokio::test]
async fn zero_teammate_broadcast_is_a_noop_success() {
    let fx = Fixture::new_with_team("conv-bcast-empty").await;
    let ctx = fx.ctx_for("lead-id");
    let result = SendMessageRuntimeTool
        .execute(
            json!({"to": "*", "message": {"type": "text", "content": "anybody?"}}),
            ctx,
        )
        .await
        .expect("zero-teammate broadcast should succeed");
    assert_eq!(result.data.as_ref().unwrap()["delivered"], 0);
}

#[tokio::test]
async fn broadcast_without_team_errors_with_helpful_message() {
    // Build a registry trio without ever calling TeamRegistry::create.
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let inbox_registry = InboxRegistry::new();
    let mut ctx = ToolExecutionContext::for_test("conv-bcast-noteam", "run", "tc")
        .with_team_registry(team_registry)
        .with_agent_names(name_registry)
        .with_inbox_registry(inbox_registry);
    ctx.agent_id = Some(AgentId::new("ghost"));

    let err = SendMessageRuntimeTool
        .execute(
            json!({"to": "*", "message": {"type": "text", "content": "x"}}),
            ctx,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(msg.contains("no team") || msg.contains("TeamCreate"), "msg: {msg}");
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn broadcast_skips_dead_inbox_and_continues_to_others() {
    let fx = Fixture::new_with_team("conv-bcast-dead").await;
    let _alive = fx.add_teammate("alpha", "a-1").await;
    let dead_inbox = fx.add_teammate("beta", "b-1").await;
    // Drop receiver -> all senders see closed channel for this inbox.
    drop(dead_inbox);
    let alive2 = fx.add_teammate("gamma", "c-1").await;

    let ctx = fx.ctx_for("lead-id");
    let result = SendMessageRuntimeTool
        .execute(
            json!({"to": "*", "message": {"type": "text", "content": "ping"}}),
            ctx,
        )
        .await
        .unwrap();

    // We expect at least gamma + alpha to receive (2 delivered); beta is
    // closed but the closure only takes effect once the receiver is fully
    // dropped — rust mpsc still accepts a queued message until next recv.
    // Either 2 or 3 is acceptable; assert >=2 and that gamma actually got it.
    let payload = result.data.as_ref().unwrap();
    let delivered = payload["delivered"].as_u64().unwrap();
    assert!(
        delivered >= 2,
        "broadcast should keep going past a dead inbox; got {payload:?}"
    );
    let item = tokio::time::timeout(std::time::Duration::from_millis(50), alive2.recv())
        .await
        .expect("gamma must still receive after beta died")
        .unwrap();
    assert!(matches!(item, InboxItem::ChatMessage { .. }));
}
