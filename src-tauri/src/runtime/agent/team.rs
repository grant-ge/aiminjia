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
    #[error("team `{0}` already exists in this conversation")]
    TeamAlreadyExists(String),
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

/// Per-session, per-team registry.
///
/// Outer key: `SessionId` (conversation)
/// Inner key: `team_name` (ASCII, validated before insertion)
///
/// Locking order: always acquire the outer registry `Mutex` first;
/// only then lock an individual `Arc<Mutex<Team>>`.  Never hold the
/// inner lock while calling `create` / `get` / `delete_team`.
#[derive(Debug, Default)]
pub struct TeamRegistry {
    teams: Mutex<HashMap<SessionId, HashMap<String, Arc<Mutex<Team>>>>>,
    /// Per-session active team name. Single owner of "which team is the
    /// session currently focused on" — replaces the duplicated cache that
    /// used to live in `QueryEngine.active_team_name`. Tools call
    /// `set_active` after `TeamCreate` / `TeamSwitch` and `clear_active`
    /// after `TeamDelete`; `ToolExecutionContext.active_team_name` is
    /// derived from `active(&session)` at ctx-build time, so the next tool
    /// call in the same turn sees the new value.
    active: Mutex<HashMap<SessionId, String>>,
}

impl TeamRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Create a new team for the session.  `team_name` must have been
    /// validated by `validate_team_name` before calling this.
    pub async fn create(
        &self,
        session_id: SessionId,
        lead: Member,
        team_name: String,
    ) -> Result<Arc<Mutex<Team>>, TeamError> {
        let mut g = self.teams.lock().await;
        let inner = g.entry(session_id.clone()).or_insert_with(HashMap::new);
        if inner.contains_key(&team_name) {
            return Err(TeamError::TeamAlreadyExists(team_name));
        }
        let team = Arc::new(Mutex::new(Team::new(session_id.clone(), lead, team_name.clone())));
        inner.insert(team_name, team.clone());
        Ok(team)
    }

    pub async fn get(&self, session_id: &SessionId, team_name: &str) -> Option<Arc<Mutex<Team>>> {
        self.teams.lock().await.get(session_id)?.get(team_name).cloned()
    }

    pub async fn list(&self, session_id: &SessionId) -> Vec<(String, Arc<Mutex<Team>>)> {
        self.teams.lock().await.get(session_id)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Delete one specific team within a session. Used by TeamDelete tool.
    /// If the inner map becomes empty, the session entry is also removed.
    pub async fn delete_team(&self, session_id: &SessionId, team_name: &str) -> Option<Arc<Mutex<Team>>> {
        let mut g = self.teams.lock().await;
        let inner = g.get_mut(session_id)?;
        let removed = inner.remove(team_name);
        if inner.is_empty() {
            g.remove(session_id);
        }
        removed
    }

    /// Drop the entire session (all teams). Used by cancel_session / conv close.
    /// Strictly different from `delete_team`—this removes every team at once.
    pub async fn drop_session(&self, session_id: &SessionId) -> Vec<(String, Arc<Mutex<Team>>)> {
        let mut g = self.teams.lock().await;
        let dropped = g.remove(session_id)
            .map(|m| m.into_iter().collect())
            .unwrap_or_default();
        self.active.lock().await.remove(session_id);
        dropped
    }

    /// LTR (P1.8): drop **all** teams.  Used by the app-close hook so a
    /// relaunch starts with a fresh registry.  Returns the number of teams
    /// that were dropped (handy for logging / tests).
    pub async fn clear_all(&self) -> usize {
        let mut g = self.teams.lock().await;
        let n: usize = g.values().map(|m| m.len()).sum();
        g.clear();
        self.active.lock().await.clear();
        n
    }

    // ── Active team accessors (replaces QueryEngine.active_team_name) ────
    //
    // Single owner of "the team currently driving this session". Tools
    // mutate via `set_active` / `clear_active`; readers (tool execution
    // context builders, chat_turn_driver) call `active`. There is no on-disk
    // mirror of this state at this layer — `conv.json::active_team_name` is
    // the persistence shim used to restore this value on resume, but the
    // in-memory copy is authoritative at runtime.

    /// Set the active team for the given session, replacing any previous
    /// value. Called by `TeamCreate` (right after the team is created) and
    /// by `TeamSwitch`.
    pub async fn set_active(&self, session_id: &SessionId, team_name: String) {
        self.active
            .lock()
            .await
            .insert(session_id.clone(), team_name);
    }

    /// Clear the active team for the given session. Called by `TeamDelete`
    /// **only** when the deleted team is the currently-active one (the
    /// caller decides — registry has no opinion).
    pub async fn clear_active(&self, session_id: &SessionId) {
        self.active.lock().await.remove(session_id);
    }

    /// Read the active team name for the given session, if any.  Used by
    /// the QueryEngine when building a `ToolExecutionContext`.
    pub async fn active(&self, session_id: &SessionId) -> Option<String> {
        self.active.lock().await.get(session_id).cloned()
    }

    /// Write the current Team state to `<conv_dir>/teams/{team_name}/config.json`.
    ///
    /// `conv_dir` should be `<aijia_home>/users/{scope}/conversations/{conv_id}`.
    /// No-op (returns `Ok(())`) if no matching team exists for `session_id`.
    /// The file is a write-through mirror; memory (this registry) stays
    /// source-of-truth.
    pub async fn persist(
        &self,
        session_id: &SessionId,
        team_name: &str,
        conv_dir: &Path,
    ) -> Result<(), TeamPersistError> {
        let Some(team_handle) = self.get(session_id, team_name).await else {
            return Ok(());
        };
        let snapshot = {
            let team = team_handle.lock().await;
            TeamSnapshot::from(&*team)
        };
        let path = crate::runtime::agent::team_paths::TeamPaths::for_team(conv_dir, team_name).config_json();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(TeamPersistError::Io)?;
        }
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(TeamPersistError::Serde)?;
        crate::storage::fs_atomic::write_atomic(&path, &bytes)
            .map_err(|e| TeamPersistError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        Ok(())
    }

    /// Remove `teams/{team_name}/` from disk.  Best-effort and idempotent: a
    /// `NotFound` error is silently ignored.  Used by TeamDelete (PR5).
    pub fn delete_persisted_team(conv_dir: &Path, team_name: &str) -> std::io::Result<()> {
        let root = crate::runtime::agent::team_paths::TeamPaths::for_team(conv_dir, team_name)
            .team_root()
            .expect("for_team always has team_root");
        match std::fs::remove_dir_all(&root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

// ── hydrate_from_disk ─────────────────────────────────────────────────────────

#[derive(thiserror::Error, Debug)]
pub enum TeamHydrateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl TeamRegistry {
    /// Cold-start / resume: scan `<conv_dir>/teams/*/config.json` and rebuild
    /// the in-memory map.  **Idempotent**: already-present team_names are
    /// skipped.  Corrupted config.json files only emit a warning.
    ///
    /// Returns the number of teams successfully loaded.
    pub async fn hydrate_from_disk(
        &self,
        session_id: &SessionId,
        conv_dir: &Path,
    ) -> Result<usize, TeamHydrateError> {
        let teams_root = conv_dir.join("teams");
        if !teams_root.exists() { return Ok(0); }
        let mut count = 0;
        for entry in std::fs::read_dir(&teams_root)? {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            let team_dir = entry.path();
            if !team_dir.is_dir() { continue; }
            let dir_name = match entry.file_name().into_string() {
                Ok(s) => s,
                Err(_) => continue,
            };
            let config = team_dir.join("config.json");
            if !config.exists() { continue; }
            let bytes = match std::fs::read(&config) {
                Ok(b) => b,
                Err(e) => { log::warn!("hydrate skip {:?}: {e}", config); continue; }
            };
            let snapshot: TeamSnapshot = match serde_json::from_slice(&bytes) {
                Ok(s) => s,
                Err(e) => { log::warn!("hydrate skip {:?}: {e}", config); continue; }
            };
            if snapshot.team_name != dir_name {
                log::warn!("hydrate skip: team_name `{}` != dir `{}`", snapshot.team_name, dir_name);
                continue;
            }
            let mut g = self.teams.lock().await;
            let inner = g.entry(session_id.clone()).or_insert_with(HashMap::new);
            if inner.contains_key(&snapshot.team_name) {
                continue; // idempotent: already hydrated, skip
            }
            let lead_member = Member {
                agent_id: snapshot.lead.agent_id.clone(),
                name: snapshot.lead.name.clone(),
                role: MemberRole::Lead,
                created_at: snapshot.lead.created_at,
                last_active_at: snapshot.lead.last_active_at,
            };
            let mut team = Team::new(session_id.clone(), lead_member, snapshot.team_name.clone());
            team.created_at = snapshot.created_at;
            for tm in &snapshot.teammates {
                let mate = Member {
                    agent_id: tm.agent_id.clone(),
                    name: tm.name.clone(),
                    role: MemberRole::Teammate {
                        employee_id: tm.employee_id.clone().unwrap_or_default(),
                        spawned_by: tm.spawned_by.clone().unwrap_or_else(|| snapshot.lead.agent_id.clone()),
                    },
                    created_at: tm.created_at,
                    last_active_at: tm.last_active_at,
                };
                let _ = team.add_teammate(mate);
            }
            inner.insert(snapshot.team_name.clone(), Arc::new(Mutex::new(team)));
            count += 1;
        }
        Ok(count)
    }
}

// ── Serialisable snapshot DTOs ────────────────────────────────────────────────

/// On-disk representation of a [`Team`], written to
/// `<conv_dir>/teams/{team_name}/config.json` by [`TeamRegistry::persist`].
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod registry_v2_tests {
    use super::*;
    use tempfile::tempdir;

    fn dummy_lead(name: &str) -> Member {
        Member {
            agent_id: AgentId::new(format!("lead-{name}")),
            name: name.to_string(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn create_two_teams_in_same_session() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.create(s.clone(), dummy_lead("b"), "beta".to_string()).await.unwrap();
        let listed = reg.list(&s).await;
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn duplicate_team_name_rejected() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        let err = reg.create(s, dummy_lead("a2"), "alpha".to_string()).await.unwrap_err();
        assert!(matches!(err, TeamError::TeamAlreadyExists(_)));
    }

    #[tokio::test]
    async fn delete_team_keeps_other_teams() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.create(s.clone(), dummy_lead("b"), "beta".to_string()).await.unwrap();
        reg.delete_team(&s, "alpha").await.unwrap();
        assert!(reg.get(&s, "alpha").await.is_none());
        assert!(reg.get(&s, "beta").await.is_some());
    }

    #[tokio::test]
    async fn drop_session_clears_all() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.create(s.clone(), dummy_lead("b"), "beta".to_string()).await.unwrap();
        let dropped = reg.drop_session(&s).await;
        assert_eq!(dropped.len(), 2);
        assert_eq!(reg.list(&s).await.len(), 0);
    }

    #[tokio::test]
    async fn hydrate_from_empty_conv() {
        let dir = tempdir().unwrap();
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        let n = reg.hydrate_from_disk(&s, dir.path()).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn persist_then_hydrate_roundtrip() {
        let dir = tempdir().unwrap();
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.persist(&s, "alpha", dir.path()).await.unwrap();
        // drop in-memory state, re-hydrate
        reg.drop_session(&s).await;
        assert_eq!(reg.list(&s).await.len(), 0);
        let n = reg.hydrate_from_disk(&s, dir.path()).await.unwrap();
        assert_eq!(n, 1);
        assert!(reg.get(&s, "alpha").await.is_some());
    }

    #[tokio::test]
    async fn active_set_get_clear() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s-active");
        assert_eq!(reg.active(&s).await, None);
        reg.set_active(&s, "alpha".to_string()).await;
        assert_eq!(reg.active(&s).await, Some("alpha".to_string()));
        reg.set_active(&s, "beta".to_string()).await;
        assert_eq!(reg.active(&s).await, Some("beta".to_string()));
        reg.clear_active(&s).await;
        assert_eq!(reg.active(&s).await, None);
    }

    #[tokio::test]
    async fn active_is_isolated_per_session() {
        let reg = TeamRegistry::new();
        let s1 = SessionId::new("s-1");
        let s2 = SessionId::new("s-2");
        reg.set_active(&s1, "alpha".to_string()).await;
        reg.set_active(&s2, "beta".to_string()).await;
        assert_eq!(reg.active(&s1).await, Some("alpha".to_string()));
        assert_eq!(reg.active(&s2).await, Some("beta".to_string()));
        reg.clear_active(&s1).await;
        assert_eq!(reg.active(&s1).await, None);
        assert_eq!(reg.active(&s2).await, Some("beta".to_string()));
    }

    #[tokio::test]
    async fn drop_session_clears_active() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s-drop");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.set_active(&s, "alpha".to_string()).await;
        reg.drop_session(&s).await;
        assert_eq!(reg.active(&s).await, None);
    }

    #[tokio::test]
    async fn clear_all_clears_active() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s-clear");
        reg.set_active(&s, "alpha".to_string()).await;
        reg.clear_all().await;
        assert_eq!(reg.active(&s).await, None);
    }
}
