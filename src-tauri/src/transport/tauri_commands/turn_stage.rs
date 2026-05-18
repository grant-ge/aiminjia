//! Tauri commands for the turn-stage persistence layer (spec §5).
//!
//! Two read-only commands the frontend uses to hydrate / recover state:
//! - `get_active_turn_stage(conversationId)` — read the current write-through
//!   snapshot for a conversation that may be mid-turn.  Returns `None` when
//!   no turn is active (no file, or file doesn't exist).  Source of truth is
//!   in-memory in the driver; this disk read serves the cross-process /
//!   webview-reload path.
//! - `get_interrupted_turn(conversationId)` — read the crash-sentinel record
//!   produced by the startup sweep when a previous process died mid-turn.
//!   Frontend uses this to render the "上次对话未完成" banner.
//! - `dismiss_interrupted_turn(conversationId)` — delete the sentinel after
//!   the user clicks 关闭 (or resends).

use serde::{Deserialize, Serialize};

use crate::runtime::chat::turn_stage::PersistedTurnStage;
use crate::storage::AiJiaHome;

/// Mirror of the on-disk `interrupted_turn.json` produced by the startup
/// recovery sweep.  Kept in this module (not in the runtime layer) so the
/// runtime stays transport-neutral.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedTurnRecord {
    pub conversation_id: String,
    pub run_id: String,
    /// The last persisted stage from the prior process — used in the banner
    /// label ("上次在 *执行 Bash* 时中断").
    pub last_stage: crate::runtime::events::TurnStage,
    /// Wall-clock ms when the recovery sweep observed the orphan file.
    pub interrupted_at_ms: u64,
}

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

#[tauri::command]
pub async fn get_interrupted_turn(
    conversation_id: String,
) -> Result<Option<InterruptedTurnRecord>, String> {
    let path = AiJiaHome::from_home().interrupted_turn_path(&conversation_id);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<InterruptedTurnRecord>(&bytes) {
            Ok(record) => Ok(Some(record)),
            Err(e) => {
                log::warn!(
                    "[turn-stage] interrupted read parse error for {conversation_id}: {e}"
                );
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read interrupted_turn.json failed: {e}")),
    }
}

#[tauri::command]
pub async fn dismiss_interrupted_turn(conversation_id: String) -> Result<(), String> {
    let path = AiJiaHome::from_home().interrupted_turn_path(&conversation_id);
    match std::fs::remove_file(&path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove interrupted_turn.json failed: {e}")),
    }
}
