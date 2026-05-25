//! P2.5: task-notification XML emitter contract + emit_to_lead routing.
//!
//! Covers:
//! - envelope shape (id, actor, action, subject, status; XML escaping)
//! - emit_to_lead delivers when Team + Lead inbox both available
//! - emit_to_lead is a no-op outside Team mode
//! - emit_to_lead skips self-actor (Lead does not notify itself)

use std::sync::Arc;

use app_lib::runtime::agent::inbox::{AgentInbox, InboxItem, MessageSource};
use app_lib::runtime::agent::task_notification_lead::{
    build_envelope, emit_to_lead, EmitOutcome, TaskAction, TaskNotificationDeps,
};
use app_lib::runtime::agent::{
    AgentNameRegistry, InboxRegistry, LeadIdleSupervisor, Member, MemberRole, TeamRegistry,
};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::messaging::StructuredMessage;
use app_lib::runtime::tools::builtin::team_tools::LEAD_NAME;

const TEAM_NAME: &str = "test-team";

#[test]
fn envelope_has_required_attributes_and_escapes() {
    let xml = build_envelope(
        "t-1",
        "alice",
        TaskAction::Updated,
        "Investigate <bug>",
        "completed",
    );
    assert!(xml.contains(r#"id="t-1""#));
    assert!(xml.contains(r#"actor="alice""#));
    assert!(xml.contains(r#"action="updated""#));
    assert!(xml.contains("&lt;bug&gt;"), "XML must escape <>");
    assert!(xml.contains("<status>completed</status>"));
}

async fn setup_team_with_lead(
    session: &SessionId,
) -> (
    Arc<TeamRegistry>,
    Arc<AgentNameRegistry>,
    Arc<InboxRegistry>,
    Arc<AgentInbox>,
) {
    let team_reg = TeamRegistry::new();
    let name_reg = AgentNameRegistry::new();
    let inbox_reg = InboxRegistry::new();

    let lead = Member {
        agent_id: AgentId::new("lead-id"),
        name: LEAD_NAME.to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    team_reg
        .create(session.clone(), lead, TEAM_NAME.to_string())
        .await
        .unwrap();
    name_reg
        .register(session, TEAM_NAME, LEAD_NAME, AgentId::new("lead-id"))
        .await
        .unwrap();
    let lead_inbox = AgentInbox::new(8);
    inbox_reg
        .register(session, TEAM_NAME, AgentId::new("lead-id"), lead_inbox.clone())
        .await;
    (team_reg, name_reg, inbox_reg, lead_inbox)
}

#[tokio::test]
async fn delivered_when_actor_is_teammate_and_team_exists() {
    let session = SessionId::new("conv-notif-1");
    let (team_reg, name_reg, inbox_reg, lead_inbox) = setup_team_with_lead(&session).await;

    let deps = TaskNotificationDeps {
        team_registry: team_reg,
        agent_names: name_reg,
        inbox_registry: inbox_reg,
        lead_idle: Some(LeadIdleSupervisor::new()),
    };

    let outcome = emit_to_lead(
        &deps,
        &session,
        TEAM_NAME,
        "researcher",
        "task-42",
        TaskAction::Claimed,
        "Audit logs",
        "in_progress",
    )
    .await;
    assert_eq!(outcome, EmitOutcome::Delivered);

    let item = lead_inbox.recv().await.unwrap();
    match item {
        InboxItem::ChatMessage {
            message,
            source: MessageSource::System,
        } => {
            let text = message.as_text().unwrap();
            assert!(text.contains(r#"id="task-42""#));
            assert!(text.contains(r#"actor="researcher""#));
            assert!(text.contains(r#"action="claimed""#));
            assert!(text.contains("<subject>Audit logs</subject>"));
        }
        other => panic!("unexpected item: {other:?}"),
    }

    // It also implicitly validates that StructuredMessage::text wrapping was
    // used (compiler enforces variant), so we don't need a separate assertion.
    let _ = StructuredMessage::text("compiler nudge");
}

#[tokio::test]
async fn no_team_path_skipped() {
    let session = SessionId::new("conv-notif-noteam");
    let team_reg = TeamRegistry::new(); // empty
    let name_reg = AgentNameRegistry::new();
    let inbox_reg = InboxRegistry::new();
    let deps = TaskNotificationDeps {
        team_registry: team_reg,
        agent_names: name_reg,
        inbox_registry: inbox_reg,
        lead_idle: None,
    };
    let outcome = emit_to_lead(
        &deps,
        &session,
        TEAM_NAME,
        "researcher",
        "t-1",
        TaskAction::Created,
        "x",
        "pending",
    )
    .await;
    assert_eq!(outcome, EmitOutcome::NoTeam);
}

#[tokio::test]
async fn lead_actor_does_not_notify_itself() {
    let session = SessionId::new("conv-notif-self");
    let (team_reg, name_reg, inbox_reg, _lead_inbox) = setup_team_with_lead(&session).await;
    let deps = TaskNotificationDeps {
        team_registry: team_reg,
        agent_names: name_reg,
        inbox_registry: inbox_reg,
        lead_idle: None,
    };
    let outcome = emit_to_lead(
        &deps,
        &session,
        TEAM_NAME,
        LEAD_NAME,
        "t-1",
        TaskAction::Created,
        "x",
        "pending",
    )
    .await;
    assert_eq!(outcome, EmitOutcome::SkippedSelfActor);
}
