use crate::runtime::ids::{AgentId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
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

    /// LTR (P1.8): drop **all** teams.  Used by the app-close hook so a
    /// relaunch starts with a fresh registry.  Returns the number of teams
    /// that were dropped (handy for logging / tests).
    pub async fn clear_all(&self) -> usize {
        let mut g = self.teams.lock().await;
        let n = g.len();
        g.clear();
        n
    }

    /// Write the current Team state to `<conv_dir>/team.json`.
    ///
    /// `conv_dir` should be `<aijia_home>/users/{scope}/conversations/{conv_id}`.
    /// No-op (returns `Ok(())`) if no team exists for `session_id`.
    /// The file is a write-through mirror; memory (this registry) stays
    /// source-of-truth.
    pub async fn persist(
        &self,
        session_id: &SessionId,
        conv_dir: &Path,
    ) -> Result<(), TeamPersistError> {
        let Some(team_handle) = self.get(session_id).await else {
            return Ok(()); // already deleted
        };
        let snapshot = {
            let team = team_handle.lock().await;
            TeamSnapshot::from(&*team)
        };
        let path = conv_dir.join("team.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(TeamPersistError::Io)?;
        }
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(TeamPersistError::Serde)?;
        write_atomic_team(&path, &bytes).map_err(TeamPersistError::Io)?;
        Ok(())
    }

    /// Remove `team.json` from disk.  Best-effort and idempotent: a
    /// `NotFound` error is silently ignored.  Used by TeamDelete (P1.7).
    pub fn delete_persisted(conv_dir: &Path) -> std::io::Result<()> {
        let path = conv_dir.join("team.json");
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Atomic write used by `TeamRegistry::persist`.
///
/// Writes to a `.json.tmp` sibling first, then renames atomically.
/// On crash mid-write the original file is left intact rather than
/// becoming a zero-byte stub.
fn write_atomic_team(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── Serialisable snapshot DTOs ────────────────────────────────────────────────

/// On-disk representation of a [`Team`], written to
/// `<conv_dir>/team.json` by [`TeamRegistry::persist`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSnapshot {
    pub team_name: String,
    pub session_id: SessionId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub lead: MemberSnapshot,
    pub teammates: Vec<MemberSnapshot>,
}

/// On-disk representation of a single [`Member`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberSnapshot {
    pub agent_id: AgentId,
    pub name: String,
    /// `"lead"` or `"teammate"`.
    pub role: String,
    /// `None` for the team lead.
    pub employee_id: Option<String>,
    /// `None` for the team lead.
    pub spawned_by: Option<AgentId>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active_at: chrono::DateTime<chrono::Utc>,
}

impl From<&Member> for MemberSnapshot {
    fn from(m: &Member) -> Self {
        let (role, employee_id, spawned_by) = match &m.role {
            MemberRole::Lead => ("lead".to_string(), None, None),
            MemberRole::Teammate {
                employee_id,
                spawned_by,
            } => (
                "teammate".to_string(),
                Some(employee_id.clone()),
                Some(spawned_by.clone()),
            ),
        };
        Self {
            agent_id: m.agent_id.clone(),
            name: m.name.clone(),
            role,
            employee_id,
            spawned_by,
            created_at: m.created_at,
            last_active_at: m.last_active_at,
        }
    }
}

impl From<&Team> for TeamSnapshot {
    fn from(t: &Team) -> Self {
        Self {
            team_name: t.team_name.clone(),
            session_id: t.session_id.clone(),
            created_at: t.created_at,
            lead: (&t.lead).into(),
            teammates: t.teammates.iter().map(Into::into).collect(),
        }
    }
}

/// Errors returned by [`TeamRegistry::persist`].
#[derive(thiserror::Error, Debug)]
pub enum TeamPersistError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}
