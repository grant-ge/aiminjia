use app_lib::runtime::agent::{AgentNameRegistry, NameRegistryError};
use app_lib::runtime::ids::{AgentId, SessionId};

const TEAM: &str = "test-team";

#[tokio::test]
async fn different_sessions_can_reuse_the_same_name() {
    let reg = AgentNameRegistry::new();
    let s1: SessionId = SessionId::new("s1");
    let s2: SessionId = SessionId::new("s2");

    reg.register(&s1, TEAM, "alice", AgentId::new("a1")).await.unwrap();
    reg.register(&s2, TEAM, "alice", AgentId::new("a2")).await.unwrap();

    assert_eq!(reg.resolve(&s1, TEAM, "alice").await, Some(AgentId::new("a1")));
    assert_eq!(reg.resolve(&s2, TEAM, "alice").await, Some(AgentId::new("a2")));
}

#[tokio::test]
async fn duplicate_registration_in_same_session_rejected() {
    let reg = AgentNameRegistry::new();
    let s: SessionId = SessionId::new("s1");
    reg.register(&s, TEAM, "alice", AgentId::new("a1")).await.unwrap();
    let err = reg.register(&s, TEAM, "alice", AgentId::new("a2")).await.unwrap_err();
    assert!(matches!(err, NameRegistryError::Duplicate(_)));
}

#[tokio::test]
async fn drop_session_clears_all_names_in_that_session() {
    let reg = AgentNameRegistry::new();
    let s: SessionId = SessionId::new("s1");
    reg.register(&s, TEAM, "alice", AgentId::new("a1")).await.unwrap();
    reg.register(&s, TEAM, "bob", AgentId::new("a2")).await.unwrap();
    reg.drop_session(&s).await;
    assert_eq!(reg.resolve(&s, TEAM, "alice").await, None);
    assert_eq!(reg.resolve(&s, TEAM, "bob").await, None);
}

#[tokio::test]
async fn different_teams_in_same_session_can_reuse_same_name() {
    let reg = AgentNameRegistry::new();
    let s: SessionId = SessionId::new("s1");
    reg.register(&s, "team-alpha", "researcher", AgentId::new("a1")).await.unwrap();
    reg.register(&s, "team-beta", "researcher", AgentId::new("a2")).await.unwrap();

    assert_eq!(reg.resolve(&s, "team-alpha", "researcher").await, Some(AgentId::new("a1")));
    assert_eq!(reg.resolve(&s, "team-beta", "researcher").await, Some(AgentId::new("a2")));
}

#[tokio::test]
async fn unregister_team_removes_only_that_team() {
    let reg = AgentNameRegistry::new();
    let s: SessionId = SessionId::new("s1");
    reg.register(&s, "team-alpha", "alice", AgentId::new("a1")).await.unwrap();
    reg.register(&s, "team-beta", "alice", AgentId::new("a2")).await.unwrap();
    reg.unregister_team(&s, "team-alpha").await;
    assert_eq!(reg.resolve(&s, "team-alpha", "alice").await, None);
    assert_eq!(reg.resolve(&s, "team-beta", "alice").await, Some(AgentId::new("a2")));
}
