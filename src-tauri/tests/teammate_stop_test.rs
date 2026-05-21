//! P2.7: TeammateStop forcibly cancels a Teammate by name.

use std::sync::Arc;

use serde_json::json;

use app_lib::runtime::agent::{AgentNameRegistry, CancellationRegistry};
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::tools::builtin::teammate_stop::TeammateStopRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

const TEAM: &str = "test-team";

fn build_ctx(
    session: &str,
    names: Arc<AgentNameRegistry>,
    cancels: Arc<CancellationRegistry>,
) -> ToolExecutionContext {
    ToolExecutionContext::for_test(session, "run-1", "tc-1")
        .with_agent_names(names)
        .with_cancellation_registry(cancels)
        .with_active_team(TEAM.to_string())
}

#[tokio::test]
async fn stop_resolves_name_and_trips_cancellation_token() {
    let names = AgentNameRegistry::new();
    let cancels = CancellationRegistry::new();
    let session = SessionId::new("conv-stop");

    let token = CancellationToken::new();
    let agent_id = AgentId::new("agent-r");
    names
        .register(&session, TEAM, "researcher", agent_id.clone())
        .await
        .unwrap();
    cancels
        .register(&session, TEAM, agent_id.clone(), token.clone())
        .await;

    let ctx = build_ctx("conv-stop", names, cancels);
    let result = TeammateStopRuntimeTool
        .execute(json!({"agent_name": "researcher"}), ctx)
        .await
        .unwrap();

    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["stopped"], true);
    assert_eq!(payload["agent_name"], "researcher");
    assert!(token.is_cancelled(), "cancel token should be tripped");
}

#[tokio::test]
async fn stop_unknown_name_is_idempotent_noop() {
    let names = AgentNameRegistry::new();
    let cancels = CancellationRegistry::new();
    let ctx = build_ctx("conv-stop-unknown", names, cancels);

    let result = TeammateStopRuntimeTool
        .execute(json!({"agent_name": "ghost"}), ctx)
        .await
        .expect("idempotent — must not error on missing name");

    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["stopped"], false);
    assert_eq!(payload["reason"], "not_found");
}

#[tokio::test]
async fn stop_resolves_but_no_token_returns_no_cancel_token_noop() {
    let names = AgentNameRegistry::new();
    let cancels = CancellationRegistry::new();
    let session = SessionId::new("conv-stop-detached");
    names
        .register(&session, TEAM, "ghost-wired", AgentId::new("g-1"))
        .await
        .unwrap();
    // cancellation registry NOT populated for this agent — simulates a
    // Teammate that already exited but whose name registration somehow
    // lingered (rare cleanup race).

    let ctx = build_ctx("conv-stop-detached", names, cancels);
    let result = TeammateStopRuntimeTool
        .execute(json!({"agent_name": "ghost-wired"}), ctx)
        .await
        .unwrap();

    let payload = result.data.as_ref().unwrap();
    assert_eq!(payload["stopped"], false);
    assert_eq!(payload["reason"], "no_cancel_token");
}

#[tokio::test]
async fn stop_missing_agent_name_field_errors() {
    let names = AgentNameRegistry::new();
    let cancels = CancellationRegistry::new();
    let ctx = build_ctx("conv-stop-bad", names, cancels);
    let err = TeammateStopRuntimeTool
        .execute(json!({}), ctx)
        .await
        .unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("agent_name"),
        "expected missing-field error: {msg}"
    );
}
