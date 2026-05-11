use crate::runtime::ids::{AgentId, SessionId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const MAX_TEAMMATES: usize = 4;

#[derive(Debug, Clone)]
pub enum MemberRole {
    Lead,
    Teammate {
        employee_id: String,
        spawned_by: AgentId,
    },
}

#[derive(Debug, Clone)]
pub struct Member {
    pub agent_id: AgentId,
    pub name: String,
    pub role: MemberRole,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active_at: chrono::DateTime<chrono::Utc>, // updated by worker_runtime on each LLM turn (P1.6)
}

#[derive(Debug)]
pub struct Team {
    pub session_id: SessionId,
    pub team_name: String,
    pub lead: Member,
    pub teammates: Vec<Member>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(thiserror::Error, Debug)]
pub enum TeamError {
    #[error("max teammate limit reached ({MAX_TEAMMATES})")]
    MaxTeammateLimitReached,
    #[error("name already taken in this team: {0}")]
    NameAlreadyTaken(String),
    #[error("team already exists for session {0:?}")]
    TeamAlreadyExists(SessionId),
}

impl Team {
    pub fn new(session_id: SessionId, lead: Member, team_name: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            session_id,
            team_name,
            lead,
            teammates: Vec::new(),
            created_at: now,
        }
    }

    pub fn add_teammate(&mut self, m: Member) -> Result<(), TeamError> {
        if self.teammates.len() >= MAX_TEAMMATES {
            return Err(TeamError::MaxTeammateLimitReached);
        }
        if self.lead.name == m.name || self.teammates.iter().any(|t| t.name == m.name) {
            return Err(TeamError::NameAlreadyTaken(m.name));
        }
        self.teammates.push(m);
        Ok(())
    }

    /// Removes the named teammate.  Returns `true` if a member was removed.
    /// Idempotent — calling twice with the same name returns `false` the second time.
    pub fn remove_teammate(&mut self, name: &str) -> bool {
        let before = self.teammates.len();
        self.teammates.retain(|m| m.name != name);
        self.teammates.len() < before
    }

    pub fn members(&self) -> impl Iterator<Item = &Member> {
        std::iter::once(&self.lead).chain(self.teammates.iter())
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Member> {
        self.members().find(|m| m.name == name)
    }
}

/// Per-session team store.
///
/// Locking order: always acquire the outer registry `Mutex` first;
/// only then lock an individual `Arc<Mutex<Team>>`.  Never hold the
/// inner lock while calling `create` / `get` / `delete`.
#[derive(Debug, Default)]
pub struct TeamRegistry {
    teams: Mutex<HashMap<SessionId, Arc<Mutex<Team>>>>,
}

impl TeamRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn create(
        &self,
        session_id: SessionId,
        lead: Member,
        team_name: String,
    ) -> Result<Arc<Mutex<Team>>, TeamError> {
        let mut g = self.teams.lock().await;
        if g.contains_key(&session_id) {
            return Err(TeamError::TeamAlreadyExists(session_id));
        }
        let team = Arc::new(Mutex::new(Team::new(session_id.clone(), lead, team_name)));
        g.insert(session_id, team.clone());
        Ok(team)
    }

    pub async fn get(&self, session_id: &SessionId) -> Option<Arc<Mutex<Team>>> {
        self.teams.lock().await.get(session_id).cloned()
    }

    pub async fn delete(&self, session_id: &SessionId) -> Option<Arc<Mutex<Team>>> {
        self.teams.lock().await.remove(session_id)
    }
}
