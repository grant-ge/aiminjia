//! Tests for `TeamRegistry::persist` and `TeamRegistry::delete_persisted`.
//!
//! Verifies that the write-through disk mirror (`<conv_dir>/team.json`) is
//! written correctly and updated on subsequent mutations, and that
//! `delete_persisted` is idempotent.

use std::fs;

use tempfile::TempDir;

use app_lib::runtime::agent::{Member, MemberRole, TeamRegistry, TeamSnapshot};
use app_lib::runtime::ids::{AgentId, SessionId};

fn mk_lead(name: &str) -> Member {
    let now = chrono::Utc::now();
    Member {
        agent_id: AgentId::new("lead-1"),
        name: name.into(),
        role: MemberRole::Lead,
        created_at: now,
        last_active_at: now,
    }
}

fn mk_teammate(name: &str, employee_id: &str, lead_id: &AgentId) -> Member {
    let now = chrono::Utc::now();
    Member {
        agent_id: AgentId::new(format!("tm-{name}")),
        name: name.into(),
        role: MemberRole::Teammate {
            employee_id: employee_id.into(),
            spawned_by: lead_id.clone(),
        },
        created_at: now,
        last_active_at: now,
    }
}

#[tokio::test]
async fn persist_writes_team_json_with_lead_only() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().to_path_buf();
    let reg = TeamRegistry::new();
    let s = SessionId::new("conv-1");
    reg.create(s.clone(), mk_lead("team-lead"), "research-team".into())
        .await
        .unwrap();

    reg.persist(&s, &conv_dir).await.unwrap();

    let path = conv_dir.join("team.json");
    assert!(path.exists());
    let contents = fs::read_to_string(&path).unwrap();
    let snap: TeamSnapshot = serde_json::from_str(&contents).unwrap();
    assert_eq!(snap.team_name, "research-team");
    assert_eq!(snap.lead.name, "team-lead");
    assert_eq!(snap.lead.role, "lead");
    assert!(snap.lead.employee_id.is_none());
    assert!(snap.lead.spawned_by.is_none());
    assert_eq!(snap.teammates.len(), 0);
}

#[tokio::test]
async fn persist_reflects_added_teammates() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().to_path_buf();
    let reg = TeamRegistry::new();
    let s = SessionId::new("conv-2");
    let team = reg
        .create(s.clone(), mk_lead("team-lead"), "rt".into())
        .await
        .unwrap();
    let lead_id = team.lock().await.lead.agent_id.clone();
    {
        let mut t = team.lock().await;
        t.add_teammate(mk_teammate("researcher", "xiaoyan", &lead_id))
            .unwrap();
    }

    reg.persist(&s, &conv_dir).await.unwrap();

    let snap: TeamSnapshot =
        serde_json::from_str(&fs::read_to_string(conv_dir.join("team.json")).unwrap()).unwrap();
    assert_eq!(snap.teammates.len(), 1);
    assert_eq!(snap.teammates[0].name, "researcher");
    assert_eq!(snap.teammates[0].role, "teammate");
    assert_eq!(snap.teammates[0].employee_id.as_deref(), Some("xiaoyan"));
}

#[tokio::test]
async fn persist_overwrites_after_remove_teammate() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().to_path_buf();
    let reg = TeamRegistry::new();
    let s = SessionId::new("conv-3");
    let team = reg
        .create(s.clone(), mk_lead("team-lead"), "rt".into())
        .await
        .unwrap();
    let lead_id = team.lock().await.lead.agent_id.clone();
    {
        let mut t = team.lock().await;
        t.add_teammate(mk_teammate("alice", "e", &lead_id)).unwrap();
        t.add_teammate(mk_teammate("bob", "e", &lead_id)).unwrap();
    }
    reg.persist(&s, &conv_dir).await.unwrap();

    {
        let mut t = team.lock().await;
        let removed = t.remove_teammate("alice");
        assert!(removed);
    }
    reg.persist(&s, &conv_dir).await.unwrap();

    let snap: TeamSnapshot =
        serde_json::from_str(&fs::read_to_string(conv_dir.join("team.json")).unwrap()).unwrap();
    assert_eq!(snap.teammates.len(), 1);
    assert_eq!(snap.teammates[0].name, "bob");
}

#[tokio::test]
async fn delete_persisted_removes_team_json_and_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().to_path_buf();
    let reg = TeamRegistry::new();
    let s = SessionId::new("conv-4");
    reg.create(s.clone(), mk_lead("team-lead"), "rt".into())
        .await
        .unwrap();
    reg.persist(&s, &conv_dir).await.unwrap();
    assert!(conv_dir.join("team.json").exists());

    TeamRegistry::delete_persisted(&conv_dir).unwrap();
    assert!(!conv_dir.join("team.json").exists());

    // Idempotent — second delete on missing file must not error.
    TeamRegistry::delete_persisted(&conv_dir).unwrap();
}
