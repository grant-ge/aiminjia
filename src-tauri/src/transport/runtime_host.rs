use anyhow::Result;
use std::sync::Arc;

use crate::runtime::agent::{AgentNameRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry};

pub trait RuntimeHost: Send + Sync {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()>;

    /// Per-process Team registry; per-Session Team membership lives here.
    fn team_registry(&self) -> Arc<TeamRegistry>;

    /// Per-process registry mapping (SessionId, name) -> AgentId.
    fn agent_names(&self) -> Arc<AgentNameRegistry>;

    /// Per-process registry mapping (SessionId, AgentId) -> Arc<AgentInbox>;
    /// SendMessage routing depends on this (P2.2).
    fn inbox_registry(&self) -> Arc<InboxRegistry>;

    /// Per-process Lead idle supervisor (P2.4).  Used by SendMessage to
    /// enqueue/wake the Lead and by chat_turn_driver for turn-end self-check.
    fn lead_idle_supervisor(&self) -> Arc<LeadIdleSupervisor>;
}
