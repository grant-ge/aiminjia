use anyhow::Result;
use tauri::Emitter;

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
}
