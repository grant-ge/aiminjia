use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::runtime::agent::{
    AgentNameRegistry, CancellationRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry,
};

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

    /// Per-process cancellation registry (P2.7).  Used by TeammateStop to
    /// trip a Teammate's cancel token by AgentId.
    fn cancellation_registry(&self) -> Arc<CancellationRegistry>;

    /// LTR (B-gap2): resolve `<aijia_home>/users/{scope}/conversations/{conv_id}`
    /// for the active user scope so the runtime can inject it into
    /// `ToolExecutionContext.conv_dir`.  Returns `None` when the scope is not
    /// resolvable (no user logged in / test host).
    fn resolve_conv_dir(&self, _conv_id: &str) -> Option<PathBuf> {
        None
    }
}
