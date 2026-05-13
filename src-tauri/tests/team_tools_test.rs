//! P1.7 integration tests for TeamCreate / TeamDelete builtin tools.

use std::sync::Arc;

use serde_json::json;

use app_lib::runtime::agent::{AgentNameRegistry, MemberRole, TeamRegistry};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::tools::builtin::team_tools::{
    TeamCreateRuntimeTool, TeamDeleteRuntimeTool, LEAD_NAME,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

fn build_ctx(
    session_id: &str,
    agent_id: Option<&str>,
    team_registry: Arc<TeamRegistry>,
    name_registry: Arc<AgentNameRegistry>,
) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::for_test(session_id, "run-1", "tc-1")
        .with_team_registry(team_registry)
        .with_agent_names(name_registry);
    if let Some(id) = agent_id {
        ctx.agent_id = Some(AgentId::new(id));
    }
    ctx
}

#[tokio::test]
async fn team_create_seeds_registry_and_registers_lead_name() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = "conv-create-happy";
    let ctx = build_ctx(session, Some("lead-id-1"), team_registry.clone(), name_registry.clone());

    let result = TeamCreateRuntimeTool
        .execute(json!({"team_name": "research-team"}), ctx)
        .await
        .unwrap();

    // Tool result JSON has the expected shape.
    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["team_name"], "research-team");
    assert_eq!(payload["session_id"], session);
    assert_eq!(payload["lead_name"], LEAD_NAME);

    // Team exists in registry with the calling agent as Lead.
    let team_handle = team_registry
        .get(&SessionId::new(session))
        .await
        .expect("team should exist");
    let team = team_handle.lock().await;
    assert_eq!(team.team_name, "research-team");
    assert_eq!(team.lead.name, LEAD_NAME);
    assert!(matches!(team.lead.role, MemberRole::Lead));
    assert_eq!(team.lead.agent_id.as_str(), "lead-id-1");

    // Lead name is registered so SendMessage(to: "team-lead") works.
    let resolved = name_registry
        .resolve(&SessionId::new(session), LEAD_NAME)
        .await
        .expect("team-lead should be registered");
    assert_eq!(resolved.as_str(), "lead-id-1");
}

#[tokio::test]
async fn team_create_default_team_name_uses_session_prefix() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = "abcdef1234567890";
    let ctx = build_ctx(session, Some("lead-x"), team_registry.clone(), name_registry);

    let result = TeamCreateRuntimeTool
        .execute(json!({}), ctx)
        .await
        .unwrap();

    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["team_name"], "team-abcdef12");
}

#[tokio::test]
async fn team_create_twice_returns_already_exists_error() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = "conv-dup-team";

    let ctx1 = build_ctx(session, Some("lead-1"), team_registry.clone(), name_registry.clone());
    TeamCreateRuntimeTool
        .execute(json!({"team_name": "team-a"}), ctx1)
        .await
        .unwrap();

    let ctx2 = build_ctx(session, Some("lead-2"), team_registry.clone(), name_registry.clone());
    let err = TeamCreateRuntimeTool
        .execute(json!({"team_name": "team-b"}), ctx2)
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.to_lowercase().contains("already") || msg.contains("exists"),
                "expected already-exists message, got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn team_delete_removes_team_and_clears_names() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = "conv-delete-happy";

    // Seed via TeamCreate so the name registry also gets populated.
    let create_ctx = build_ctx(session, Some("lead-d"), team_registry.clone(), name_registry.clone());
    TeamCreateRuntimeTool
        .execute(json!({"team_name": "ephemeral"}), create_ctx)
        .await
        .unwrap();

    // Also register a teammate name to verify drop_session clears it.
    let sid = SessionId::new(session);
    name_registry
        .register(&sid, "researcher", AgentId::new("teammate-1"))
        .await
        .unwrap();

    let delete_ctx = build_ctx(session, Some("lead-d"), team_registry.clone(), name_registry.clone());
    let result = TeamDeleteRuntimeTool
        .execute(json!({}), delete_ctx)
        .await
        .unwrap();

    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["team_existed"], true);
    assert_eq!(payload["team_name"], "ephemeral");

    // Team has been removed.
    assert!(team_registry.get(&sid).await.is_none());

    // Both names cleared.
    assert!(name_registry.resolve(&sid, LEAD_NAME).await.is_none());
    assert!(name_registry.resolve(&sid, "researcher").await.is_none());
}

#[tokio::test]
async fn team_delete_without_team_is_idempotent_noop() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = "conv-no-team";
    let ctx = build_ctx(session, Some("lead-n"), team_registry.clone(), name_registry);

    let result = TeamDeleteRuntimeTool
        .execute(json!({}), ctx)
        .await
        .expect("TeamDelete should be a noop, not error, when no team exists");

    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["team_existed"], false);
    assert_eq!(payload["teammates_dismissed"], 0);
}

/// LTR P2 收尾：TeamCreate 必须为 Lead 注册 inbox，否则 teammate 调
/// `SendMessage(to: "team-lead")` 会因为查不到 inbox 而被拒。
#[tokio::test]
async fn team_create_registers_lead_inbox_when_registry_is_present() {
    use app_lib::runtime::agent::inbox::{InboxItem, MessageSource};
    use app_lib::runtime::agent::InboxRegistry;
    use app_lib::runtime::messaging::StructuredMessage;

    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let inbox_registry = InboxRegistry::new();
    let session = "conv-lead-inbox";
    let lead_agent = "lead-with-inbox";

    let mut ctx = ToolExecutionContext::for_test(session, "run-1", "tc-inbox-1")
        .with_team_registry(team_registry.clone())
        .with_agent_names(name_registry.clone())
        .with_inbox_registry(inbox_registry.clone());
    ctx.agent_id = Some(AgentId::new(lead_agent));

    TeamCreateRuntimeTool
        .execute(json!({"team_name": "with-inbox"}), ctx)
        .await
        .expect("TeamCreate should succeed");

    // Lead inbox is now resolvable through the registry by the agent_id we
    // provided as the calling Lead.
    let sid = SessionId::new(session);
    let inbox = inbox_registry
        .get(&sid, &AgentId::new(lead_agent))
        .await
        .expect("Lead inbox should be registered after TeamCreate");

    // Round-trip a message through the inbox so we know it's wired and not
    // a dummy registration.
    inbox
        .send(InboxItem::ChatMessage {
            message: StructuredMessage::text("hi-lead"),
            source: MessageSource::Teammate("researcher".into()),
        })
        .await
        .expect("send into Lead inbox should succeed");

    match inbox.drain_pending().await.into_iter().next().unwrap() {
        InboxItem::ChatMessage { message, source } => {
            assert_eq!(message.as_text(), Some("hi-lead"));
            assert_eq!(source, MessageSource::Teammate("researcher".into()));
        }
        other => panic!("unexpected inbox item: {other:?}"),
    }
}

/// Legacy / unit-test paths build a `ToolExecutionContext` without the
/// inbox registry — `TeamCreate` must remain a successful no-op in that
/// case (it just logs a warning), not error out.
#[tokio::test]
async fn team_create_succeeds_without_inbox_registry_for_legacy_paths() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = "conv-no-inbox-reg";
    let ctx = build_ctx(session, Some("lead-legacy"), team_registry, name_registry.clone());

    TeamCreateRuntimeTool
        .execute(json!({"team_name": "legacy-team"}), ctx)
        .await
        .expect("TeamCreate should succeed even without inbox_registry wired");

    // The Lead name is still registered — that part is independent of
    // the inbox path.
    let sid = SessionId::new(session);
    assert!(name_registry.resolve(&sid, LEAD_NAME).await.is_some());
}
