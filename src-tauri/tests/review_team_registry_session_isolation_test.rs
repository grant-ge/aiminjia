//! review: TeamRegistry enforces per-SessionId isolation, duplicate-create guard,
//! MAX_TEAMMATES hard limit, and duplicate-name rejection inside a team.
//!
//! These are compile+runtime guards for the Team entity introduced in P1.1.

use app_lib::runtime::agent::team::{Member, MemberRole, TeamError, TeamRegistry, MAX_TEAMMATES};
use app_lib::runtime::ids::{AgentId, SessionId};

fn mk_lead(name: &str) -> Member {
    let now = chrono::Utc::now();
    Member {
        agent_id: AgentId::new(format!("lead-{}", uuid::Uuid::new_v4())),
        name: name.into(),
        role: MemberRole::Lead,
        created_at: now,
        last_active_at: now,
    }
}

fn mk_teammate(name: &str, spawned_by: &AgentId) -> Member {
    let now = chrono::Utc::now();
    Member {
        agent_id: AgentId::new(format!("tm-{}", uuid::Uuid::new_v4())),
        name: name.into(),
        role: MemberRole::Teammate {
            employee_id: "e1".into(),
            spawned_by: spawned_by.clone(),
        },
        created_at: now,
        last_active_at: now,
    }
}

#[tokio::test]
async fn different_session_ids_have_isolated_teams() {
    let reg = TeamRegistry::new();
    let s1: SessionId = "s1".into();
    let s2: SessionId = "s2".into();
    reg.create(s1.clone(), mk_lead("team-lead"), "t1".into())
        .await
        .unwrap();
    assert!(
        reg.get(&s2, "t1").await.is_none(),
        "s2 must not see s1's team"
    );
}

#[tokio::test]
async fn duplicate_create_returns_team_already_exists() {
    let reg = TeamRegistry::new();
    let s: SessionId = "s1".into();
    reg.create(s.clone(), mk_lead("team-lead"), "t1".into())
        .await
        .unwrap();
    let err = reg
        .create(s.clone(), mk_lead("team-lead"), "t1".into())
        .await
        .unwrap_err();
    assert!(matches!(err, TeamError::TeamAlreadyExists(_)));
}

#[tokio::test]
async fn fifth_teammate_returns_max_limit() {
    let reg = TeamRegistry::new();
    let s: SessionId = "s1".into();
    let team = reg
        .create(s.clone(), mk_lead("team-lead"), "t1".into())
        .await
        .unwrap();
    // Acquire a single lock, extract lead_id, fill teammates, then attempt overflow.
    let mut team_g = team.lock().await;
    let lead_id = team_g.lead.agent_id.clone();
    for i in 0..MAX_TEAMMATES {
        team_g
            .add_teammate(mk_teammate(&format!("m{i}"), &lead_id))
            .unwrap();
    }
    let err = team_g
        .add_teammate(mk_teammate("overflow", &lead_id))
        .unwrap_err();
    assert!(matches!(err, TeamError::MaxTeammateLimitReached));
}

#[tokio::test]
async fn duplicate_name_in_team_rejected() {
    let reg = TeamRegistry::new();
    let s: SessionId = "s1".into();
    let team = reg
        .create(s.clone(), mk_lead("team-lead"), "t1".into())
        .await
        .unwrap();
    let mut team_g = team.lock().await;
    let lead_id = team_g.lead.agent_id.clone();
    team_g.add_teammate(mk_teammate("alice", &lead_id)).unwrap();
    let err = team_g
        .add_teammate(mk_teammate("alice", &lead_id))
        .unwrap_err();
    assert!(matches!(err, TeamError::NameAlreadyTaken(_)));
}
