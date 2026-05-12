//! Tauri command surface for the pending message queue.

use std::sync::Arc;

use tauri::AppHandle;
use tauri::Manager;

use crate::runtime::ids::SessionId;
use crate::runtime::pending::{PendingItem, PendingQueueManager};

#[tauri::command]
pub async fn pending_snapshot_for_session(
    app: AppHandle,
    session_id: String,
) -> Result<Vec<PendingItem>, String> {
    let mgr = app
        .try_state::<Arc<PendingQueueManager>>()
        .ok_or_else(|| "PendingQueueManager not initialised".to_string())?
        .inner()
        .clone();
    Ok(mgr.snapshot(&SessionId::new(session_id)).await)
}

#[tauri::command]
pub async fn pending_remove_item(
    app: AppHandle,
    session_id: String,
    item_id: String,
) -> Result<bool, String> {
    let mgr = app
        .try_state::<Arc<PendingQueueManager>>()
        .ok_or_else(|| "PendingQueueManager not initialised".to_string())?
        .inner()
        .clone();
    mgr.remove_item(&SessionId::new(session_id), &item_id)
        .await
        .map_err(|e| format!("{e:#}"))
}
