use anyhow::Result;
use std::sync::Arc;
use tauri::Emitter;

use crate::runtime::agent::{AgentNameRegistry, TeamRegistry};
use crate::transport::runtime_host::RuntimeHost;

pub struct TauriRuntimeHost {
    app: tauri::AppHandle,
}

impl TauriRuntimeHost {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeHost for TauriRuntimeHost {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()> {
        self.app.emit(name, payload)?;
        Ok(())
    }

    fn team_registry(&self) -> Arc<TeamRegistry> {
        self.app.state::<Arc<TeamRegistry>>().inner().clone()
    }

    fn agent_names(&self) -> Arc<AgentNameRegistry> {
        self.app.state::<Arc<AgentNameRegistry>>().inner().clone()
    }
}
