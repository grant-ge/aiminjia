//! Tauri command for the turn-stage persistence layer (spec §5).
//!
//! - `get_active_turn_stage(conversationId)` — read the current write-through
//!   snapshot for a conversation that may be mid-turn.  Returns `None` when
//!   no turn is active (no file, or file doesn't exist).  Source of truth is
//!   in-memory in the driver; this disk read serves the cross-process /
//!   webview-reload path.

use crate::runtime::chat::turn_stage::PersistedTurnStage;
use crate::storage::AiJiaHome;

#[tauri::command]
pub async fn get_active_turn_stage(
    conversation_id: String,
) -> Result<Option<PersistedTurnStage>, String> {
    let path = AiJiaHome::from_home().turn_stage_path(&conversation_id);
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
