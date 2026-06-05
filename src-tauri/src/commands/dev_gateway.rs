//! Dev-only gateway switcher commands.
//!
//! The entire module is gated on `debug_assertions`; in release builds it is
//! not compiled and the commands are not registered, so the gateway origin is
//! pinned to production with no runtime escape hatch. See [`crate::gateway`].

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::gateway::dev;
use crate::storage::GlobalConfigStore;

/// A built-in gateway preset for the dev switcher UI.
#[derive(Debug, Serialize)]
pub struct GatewayPreset {
    pub label: String,
    pub host: String,
}

/// Current dev gateway state surfaced to the settings panel.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevGatewayState {
    /// The host the app is currently talking to (override or production).
    pub current_host: String,
    /// `true` when a dev override is active (i.e. not production).
    pub is_override: bool,
    /// Built-in presets (production / test / pre).
    pub presets: Vec<GatewayPreset>,
}

/// Read the current dev gateway state and the available presets.
#[tauri::command]
pub fn get_dev_gateway() -> DevGatewayState {
    DevGatewayState {
        current_host: dev::effective_host(),
        is_override: dev::current_override().is_some(),
        presets: dev::PRESETS
            .iter()
            .map(|(label, host)| GatewayPreset {
                label: label.to_string(),
                host: host.to_string(),
            })
            .collect(),
    }
}

/// Switch the gateway host. An empty `host` resets to production.
///
/// Persists to global config and updates the in-memory override so subsequent
/// auth/LLM requests use it. The caller must re-login afterwards: tokens issued
/// by the previous host are not valid on the new one.
#[tauri::command]
pub fn set_dev_gateway(
    global_store: State<'_, Arc<GlobalConfigStore>>,
    host: String,
) -> Result<DevGatewayState, String> {
    let normalized = dev::set(&host);
    match &normalized {
        Some(value) => global_store
            .set_setting(dev::CONFIG_KEY, value)
            .map_err(|e| e.to_string())?,
        None => global_store
            .delete_setting(dev::CONFIG_KEY)
            .map_err(|e| e.to_string())?,
    }
    Ok(get_dev_gateway())
}
