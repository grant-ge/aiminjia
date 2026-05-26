use std::sync::Arc;

use serde_json::json;
use tauri::State;

use crate::runtime::network::probe::NetworkProbe;
use crate::runtime::network::state::NetworkSnapshot;

/// Return the latest cached snapshot, or null if no probe has completed yet.
#[tauri::command]
pub async fn network_get_status(
    probe: State<'_, Arc<NetworkProbe>>,
) -> Result<Option<NetworkSnapshot>, String> {
    Ok(probe.snapshot())
}

/// Trigger an immediate probe. Returns `{ "triggered": bool }` —
/// false if throttled (called within 1 second of the previous force probe).
#[tauri::command]
pub async fn network_force_probe(
    probe: State<'_, Arc<NetworkProbe>>,
) -> Result<serde_json::Value, String> {
    let triggered = probe.request_force_probe();
    Ok(json!({ "triggered": triggered }))
}
