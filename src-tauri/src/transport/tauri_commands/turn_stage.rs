//! Tauri command for the turn-stage persistence layer (spec §5).
//!
//! - `get_active_turn_stage(conversationId)` — read the current write-through
//!   snapshot for a conversation that may be mid-turn.  Returns `None` when
//!   no turn is active (no file, or file doesn't exist).  Source of truth is
//!   in-memory in the driver; this disk read serves the cross-process /
//!   webview-reload path.
//! - `clear_active_turn_stage(conversationId)` — remove a stale snapshot after
//!   the user chooses to stop a recovered turn that no longer has live runtime
//!   state.
//!
//! Path is resolved user-scoped (`users/{scope}/turn_stages/{conv_id}.json`)
//! via `CurrentUserStorage`; returns `None` when no user is logged in.

use std::sync::Arc;

use crate::runtime::chat::turn_stage::PersistedTurnStage;
use crate::storage::current_user_storage::CurrentUserStorage;
use crate::storage::user_scoped_paths::UserScopedPathResolver;

#[tauri::command]
pub async fn get_active_turn_stage(
    current_user: tauri::State<'_, Arc<CurrentUserStorage>>,
    conversation_id: String,
) -> Result<Option<PersistedTurnStage>, String> {
    let Some(paths) = current_user.resolve_paths() else {
        return Ok(None);
    };
    let path = paths.turn_stage_path(&conversation_id);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<PersistedTurnStage>(&bytes) {
            Ok(record) => Ok(Some(record)),
            Err(e) => {
                log::warn!("[turn-stage] active read parse error for {conversation_id}: {e}");
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read turn_stage.json failed: {e}")),
    }
}

#[tauri::command]
pub async fn clear_active_turn_stage(
    current_user: tauri::State<'_, Arc<CurrentUserStorage>>,
    conversation_id: String,
) -> Result<(), String> {
    let Some(paths) = current_user.resolve_paths() else {
        return Ok(());
    };
    let path = paths.turn_stage_path(&conversation_id);
    match std::fs::remove_file(&path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("clear turn_stage.json failed: {e}")),
    }
}
