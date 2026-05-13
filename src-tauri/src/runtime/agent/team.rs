use crate::runtime::ids::{AgentId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const MAX_TEAMMATES: usize = 4;

/// Member lifecycle states.  Persisted to `team.json` for the UI to render the
/// roster (grey-out stopped/cancelled members rather than hiding them).
///
/// Transitions:
///   Spawning ─(first SSE chunk / first tool call)─▶ Active
///   Active   ─(TeammateStop)──────────────────────▶ Stopped
///   Active   ─(cascade on TeamDelete)─────────────▶ Cancelled
///
/// We **deliberately omit a `Failed` state** (decision #5): without a
/// hard timeout, "the LLM call hangs" is indistinguishable from "the LLM is
/// taking 60 seconds to think". Dead teammates show as `Active` forever and
/// are surfaced via lead prompt education + user-driven cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Spawning,
    Active,
    Stopped,
    Cancelled,
}

impl Default for MemberStatus {
    fn default() -> Self {
        MemberStatus::Spawning
    }
}

/// Team lifecycle status. Active teams have `team.json` on disk; Disbanded teams
/// have been moved to `teams/history/{disbanded_at}.json` and `team.json` is
/// removed (so `file::exists("team.json")` is the single source of truth for
/// "does this conversation have an active team").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamStatus {
    Active,
    Disbanded,
}

impl Default for TeamStatus {
    fn default() -> Self {
        TeamStatus::Active
    }
}

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
    /// Lifecycle state. Defaults to Spawning at construction; the worker
    /// runtime / TeammateStop tool / TeamDelete cascade transition this.
    pub status: MemberStatus,
    /// Set when `status` transitions to Stopped or Cancelled.
    pub stopped_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Human-readable reason for the stop (`tool_call` / `cascade` / etc).
    pub stopped_reason: Option<String>,
}

#[derive(Debug)]
pub struct Team {
    pub session_id: SessionId,
    pub team_name: String,
    /// `None` after the LLM didn't supply a description. UI shows it as the
    /// sub-line under the team name; falsy → omit.
    pub description: Option<String>,
    pub lead: Member,
    pub teammates: Vec<Member>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: TeamStatus,
    /// Set when `status` transitions to Disbanded.
    pub disbanded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub disbanded_reason: Option<String>,
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
            description: None,
            lead,
            teammates: Vec::new(),
            created_at: now,
            status: TeamStatus::Active,
            disbanded_at: None,
            disbanded_reason: None,
        }
    }

    /// Mark the team as Disbanded. Caller is responsible for archiving
    /// team.json to history/ + removing team.json from disk. Members that
    /// were still Active/Spawning are marked Cancelled (cascade).
    pub fn disband(&mut self, reason: &str) {
        let now = chrono::Utc::now();
        self.status = TeamStatus::Disbanded;
        self.disbanded_at = Some(now);
        self.disbanded_reason = Some(reason.to_string());
        for m in &mut self.teammates {
            if matches!(m.status, MemberStatus::Spawning | MemberStatus::Active) {
                m.status = MemberStatus::Cancelled;
                m.stopped_at = Some(now);
                m.stopped_reason = Some(reason.to_string());
            }
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

    /// Mark a named teammate as `Stopped` (decision #4: keep in roster, don't
    /// remove). Idempotent. Returns `true` if a member was found + transitioned;
    /// `false` if the name was unknown or the member was already terminal.
    ///
    /// Use this from `TeammateStop` (reason="tool_call") rather than
    /// `remove_teammate` to preserve UI history.
    pub fn mark_teammate_stopped(&mut self, name: &str, reason: &str) -> bool {
        let Some(m) = self.teammates.iter_mut().find(|m| m.name == name) else {
            return false;
        };
        if matches!(m.status, MemberStatus::Stopped | MemberStatus::Cancelled) {
            return false;
        }
        m.status = MemberStatus::Stopped;
        m.stopped_at = Some(chrono::Utc::now());
        m.stopped_reason = Some(reason.to_string());
        true
    }

    /// Transition a teammate from Spawning → Active. Called from worker_runtime
    /// on the teammate's first LLM turn. Idempotent for already-Active members;
    /// noop for terminal (Stopped/Cancelled) members.
    pub fn mark_teammate_active(&mut self, name: &str) -> bool {
        let Some(m) = self.teammates.iter_mut().find(|m| m.name == name) else {
            return false;
        };
        if matches!(m.status, MemberStatus::Spawning) {
            m.status = MemberStatus::Active;
            m.last_active_at = chrono::Utc::now();
            true
        } else {
            false
        }
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

    /// Archive a [`TeamSnapshot`] to `<conv_dir>/teams/history/{ts}.json`.
    /// Used by TeamDelete (v0.3 decision #3 "soft delete"): the team is
    /// marked Disbanded before this call, the snapshot is dumped to history,
    /// then `team.json` is removed from disk.
    ///
    /// Best-effort: serialization or IO failure logs a warning but does not
    /// propagate — losing archived history is preferable to leaving a half-
    /// disbanded team blocking the next TeamCreate.
    pub fn archive_to_history(
        conv_dir: &Path,
        snapshot: &TeamSnapshot,
    ) -> Result<(), TeamPersistError> {
        let history_dir = conv_dir.join("teams").join("history");
        std::fs::create_dir_all(&history_dir).map_err(TeamPersistError::Io)?;
        let ts = snapshot
            .disbanded_at
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y%m%dT%H%M%S%.3fZ")
            .to_string();
        let path = history_dir.join(format!("{}.json", ts));
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(TeamPersistError::Serde)?;
        write_atomic_team(&path, &bytes).map_err(TeamPersistError::Io)?;
        Ok(())
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
///
/// v0.3 (2026-05-13): added `status` + `description` + `disbanded_at` +
/// `disbanded_reason` + per-member `MemberStatus`. All defaulted with
/// `#[serde(default)]` so older snapshots written by v0.2 still deserialize
/// (legacy files surface as `status=Active`, members as `Active`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSnapshot {
    pub team_name: String,
    pub session_id: SessionId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub lead: MemberSnapshot,
    pub teammates: Vec<MemberSnapshot>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: TeamStatus,
    #[serde(default)]
    pub disbanded_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub disbanded_reason: Option<String>,
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
    /// v0.3+. Lead is always `Active`. Defaults to `Active` for back-compat
    /// with older snapshots that didn't include this field.
    #[serde(default = "default_active_status")]
    pub status: MemberStatus,
    #[serde(default)]
    pub stopped_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub stopped_reason: Option<String>,
}

fn default_active_status() -> MemberStatus {
    MemberStatus::Active
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
            status: m.status,
            stopped_at: m.stopped_at,
            stopped_reason: m.stopped_reason.clone(),
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
            description: t.description.clone(),
            status: t.status,
            disbanded_at: t.disbanded_at,
            disbanded_reason: t.disbanded_reason.clone(),
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
