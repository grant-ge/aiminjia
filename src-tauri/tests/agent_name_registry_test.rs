use app_lib::runtime::agent::{AgentNameRegistry, NameRegistryError};
use app_lib::runtime::ids::{AgentId, SessionId};

#[tokio::test]
async fn different_sessions_can_reuse_the_same_name() {
    let reg = AgentNameRegistry::new();
    let s1: SessionId = SessionId::new("s1");
    let s2: SessionId = SessionId::new("s2");

    reg.register(&s1, "alice", AgentId::new("a1")).await.unwrap();
    reg.register(&s2, "alice", AgentId::new("a2")).await.unwrap();

    assert_eq!(reg.resolve(&s1, "alice").await, Some(AgentId::new("a1")));
    assert_eq!(reg.resolve(&s2, "alice").await, Some(AgentId::new("a2")));
}

#[tokio::test]
async fn duplicate_registration_in_same_session_rejected() {
    let reg = AgentNameRegistry::new();
    let s: SessionId = SessionId::new("s1");
    reg.register(&s, "alice", AgentId::new("a1")).await.unwrap();
    let err = reg.register(&s, "alice", AgentId::new("a2")).await.unwrap_err();
    assert!(matches!(err, NameRegistryError::Duplicate(_)));
}

#[tokio::test]
async fn drop_session_clears_all_names_in_that_session() {
    let reg = AgentNameRegistry::new();
    let s: SessionId = SessionId::new("s1");
    reg.register(&s, "alice", AgentId::new("a1")).await.unwrap();
    reg.register(&s, "bob", AgentId::new("a2")).await.unwrap();
    reg.drop_session(&s).await;
    assert_eq!(reg.resolve(&s, "alice").await, None);
    assert_eq!(reg.resolve(&s, "bob").await, None);
}
