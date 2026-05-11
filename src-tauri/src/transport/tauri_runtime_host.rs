use anyhow::Result;
use std::sync::Arc;
use tauri::{Emitter, Manager};

use crate::runtime::agent::{AgentNameRegistry, InboxRegistry, TeamRegistry};
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

    fn inbox_registry(&self) -> Arc<InboxRegistry> {
        self.app.state::<Arc<InboxRegistry>>().inner().clone()
    }
}
