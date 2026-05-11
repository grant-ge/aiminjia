//! P1.8 integration tests for session-lifecycle cleanup of LTR registries.
//!
//! Covers two of the four cleanup triggers from v4 §7.3:
//! - `SessionRuntime::cancel_session` — async cleanup spawned in-band.
//! - `clear_all` semantics on the registries (used by the app-close hook in lib.rs;
//!   the hook itself is wired in lib.rs and not exercised here because RunEvent
//!   is hard to mock — the unit-level guarantee is that clear_all empties the table).
//!
//! TeamDelete is covered by team_tools_test.rs (P1.7) and session-GC is deferred.

use std::sync::Arc;
use std::time::Duration;

use app_lib::runtime::agent::{
    AgentNameRegistry, Member, MemberRole, TeamRegistry,
};
use app_lib::runtime::cancellation::CancellationReason;
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::session_runtime::SessionRuntime;

fn seed_lead(name: &str, agent_id: &str) -> Member {
    Member {
        agent_id: AgentId::new(agent_id),
        name: name.to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn cancel_session_drops_team_and_name_bindings() {
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let session = SessionId::new("conv-cancel-cleanup");

    // Pre-populate registries as if the Lead had run TeamCreate + spawned a Teammate.
    team_registry
        .create(session.clone(), seed_lead("team-lead", "lead-1"), "team-x".to_string())
        .await
        .unwrap();
    name_registry
        .register(&session, "team-lead", AgentId::new("lead-1"))
        .await
        .unwrap();
    name_registry
        .register(&session, "researcher", AgentId::new("teammate-1"))
        .await
        .unwrap();

    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
        .with_team_registry(team_registry.clone())
        .with_agent_names(name_registry.clone());

    runtime.cancel_session(&session, CancellationReason::UserCancel);

    // cleanup is spawned async — give the runtime a tick to land it.
    for _ in 0..20 {
        if team_registry.get(&session).await.is_none()
            && name_registry.resolve(&session, "team-lead").await.is_none()
            && name_registry.resolve(&session, "researcher").await.is_none()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("cancel_session did not clean up registries within 500ms");
}

#[tokio::test]
async fn cancel_session_without_registries_is_noop() {
    // No registry injected — cancel_session must not panic.
    let session = SessionId::new("conv-no-reg");
    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
    runtime.cancel_session(&session, CancellationReason::UserCancel);
}

#[tokio::test]
async fn clear_all_empties_team_registry() {
    let team_registry = TeamRegistry::new();
    let s1 = SessionId::new("a");
    let s2 = SessionId::new("b");
    team_registry
        .create(s1.clone(), seed_lead("lead", "a-id"), "team-a".to_string())
        .await
        .unwrap();
    team_registry
        .create(s2.clone(), seed_lead("lead", "b-id"), "team-b".to_string())
        .await
        .unwrap();

    let dropped = team_registry.clear_all().await;
    assert_eq!(dropped, 2);
    assert!(team_registry.get(&s1).await.is_none());
    assert!(team_registry.get(&s2).await.is_none());
}

#[tokio::test]
async fn clear_all_empties_name_registry() {
    let name_registry = AgentNameRegistry::new();
    let s1 = SessionId::new("a");
    let s2 = SessionId::new("b");
    name_registry
        .register(&s1, "alpha", AgentId::new("a-1"))
        .await
        .unwrap();
    name_registry
        .register(&s2, "beta", AgentId::new("b-1"))
        .await
        .unwrap();

    let dropped = name_registry.clear_all().await;
    assert_eq!(dropped, 2);
    assert!(name_registry.resolve(&s1, "alpha").await.is_none());
    assert!(name_registry.resolve(&s2, "beta").await.is_none());
}
