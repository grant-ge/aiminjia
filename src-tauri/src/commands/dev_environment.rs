//! Dev-only environment switcher commands.
//!
//! The entire module is gated on `debug_assertions`; in release builds it is
//! not compiled and the commands are not registered, so the environment is
//! pinned to production with no runtime escape hatch. See [`crate::environment`].

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::environment::dev;
use crate::storage::GlobalConfigStore;

/// A built-in environment preset for the dev switcher UI. `key` is a stable,
/// language-neutral id (`test` / `pre` / `prod`) the frontend translates.
#[derive(Debug, Serialize)]
pub struct EnvironmentPreset {
    pub key: String,
    pub tenant: String,
    pub ops: String,
}

/// Current dev environment state surfaced to the switcher.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevEnvironmentState {
    /// The tenant origin the app is currently talking to.
    pub current_tenant: String,
    /// The ops-portal origin the app is currently talking to.
    pub current_ops: String,
    /// `true` when a dev override is active (i.e. not production).
    pub is_override: bool,
    /// Built-in presets (test / pre / prod).
    pub presets: Vec<EnvironmentPreset>,
}

/// Read the current dev environment state and the available presets.
#[tauri::command]
pub fn get_dev_environment() -> DevEnvironmentState {
    let env = dev::effective();
    DevEnvironmentState {
        current_tenant: env.tenant,
        current_ops: env.ops,
        is_override: dev::current_override().is_some(),
        presets: dev::PRESETS
            .iter()
            .map(|(key, tenant, ops)| EnvironmentPreset {
                key: key.to_string(),
                tenant: tenant.to_string(),
                ops: ops.to_string(),
            })
            .collect(),
    }
}

/// Switch the environment. Empty origins (or the production pair) reset to
/// production.
///
/// Persists to global config and updates the in-memory override so subsequent
/// auth/LLM/ops requests use it. The caller must re-login afterwards: tokens
/// issued by the previous environment are not valid on the new one.
#[tauri::command]
pub fn set_dev_environment(
    global_store: State<'_, Arc<GlobalConfigStore>>,
    tenant: String,
    ops: String,
) -> Result<DevEnvironmentState, String> {
    let normalized = dev::set(&tenant, &ops);
    match &normalized {
        Some(env) => global_store
            .set_setting(dev::CONFIG_KEY, &dev::serialize(env))
            .map_err(|e| e.to_string())?,
        None => global_store
            .delete_setting(dev::CONFIG_KEY)
            .map_err(|e| e.to_string())?,
    }
    Ok(get_dev_environment())
}
