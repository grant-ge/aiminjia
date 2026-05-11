use anyhow::Result;
use std::sync::Arc;

use crate::runtime::agent::{AgentNameRegistry, TeamRegistry};

pub trait RuntimeHost: Send + Sync {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()>;

    /// Per-process Team registry; per-Session Team membership lives here.
    fn team_registry(&self) -> Arc<TeamRegistry>;

    /// Per-process registry mapping (SessionId, name) -> AgentId.
    fn agent_names(&self) -> Arc<AgentNameRegistry>;
}
