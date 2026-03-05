//! Analysis state management.
//!
//! Stores per-conversation analysis state in `analysis.json`.

use std::path::{Path, PathBuf};

use chrono::Utc;
use log::info;

use super::conversations::conv_dir;
use super::error::StorageResult;
use super::io::{atomic_write_json, read_json_optional};
use super::types::StoredAnalysisState;

fn analysis_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("analysis.json")
}

/// Upsert the analysis state for a conversation.
pub fn upsert_analysis_state(
    base_dir: &Path,
    conversation_id: &str,
    current_step: i32,
    step_status: &str,
    state_data: &str,
) -> StorageResult<()> {
    let now = Utc::now().to_rfc3339();
    let step_status_val: serde_json::Value =
        serde_json::from_str(step_status).unwrap_or(serde_json::json!({}));
    let state_data_val: serde_json::Value =
        serde_json::from_str(state_data).unwrap_or(serde_json::json!({}));

    // Preserve final_status if it exists
    let existing = read_analysis_state_raw(base_dir, conversation_id)?;
    let final_status = existing.and_then(|s| s.final_status);

    let state = StoredAnalysisState {
        conversation_id: conversation_id.to_string(),
        current_step,
        step_status: step_status_val,
        state_data: state_data_val,
        final_status,
        updated_at: now,
    };

    atomic_write_json(&analysis_path(base_dir, conversation_id), &state)?;
    Ok(())
}

/// Get the analysis state for a conversation.
pub fn get_analysis_state(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Option<serde_json::Value>> {
    let state = read_analysis_state_raw(base_dir, conversation_id)?;
    Ok(state.map(|s| {
        serde_json::json!({
            "id": format!("as_{}", conversation_id),
            "conversationId": s.conversation_id,
            "currentStep": s.current_step,
            "stepStatus": s.step_status,
            "stateData": s.state_data,
            "updatedAt": s.updated_at,
            "finalStatus": s.final_status,
        })
    }))
}

/// Mark an analysis as finalized (completed or aborted).
pub fn finalize_analysis(
    base_dir: &Path,
    conversation_id: &str,
    final_status: &str,
) -> StorageResult<()> {
    let path = analysis_path(base_dir, conversation_id);
    let mut state: StoredAnalysisState = match read_json_optional(&path)? {
        Some(s) => s,
        None => return Ok(()),
    };

    state.final_status = Some(final_status.to_string());
    state.updated_at = Utc::now().to_rfc3339();
    atomic_write_json(&path, &state)?;
    Ok(())
}

/// Reset any analysis_states stuck in "in_progress" to "paused".
pub fn reset_stuck_analysis_state(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<()> {
    let path = analysis_path(base_dir, conversation_id);
    let mut state: StoredAnalysisState = match read_json_optional(&path)? {
        Some(s) => s,
        None => return Ok(()),
    };

    let status_str = serde_json::to_string(&state.step_status).unwrap_or_default();
    if status_str.contains("in_progress") {
        let updated = status_str.replace("in_progress", "paused");
        state.step_status = serde_json::from_str(&updated).unwrap_or(state.step_status);
        state.updated_at = Utc::now().to_rfc3339();
        atomic_write_json(&path, &state)?;
        info!(
            "Reset stuck analysis state for conversation {}",
            conversation_id
        );
    }

    Ok(())
}

// ─── Internal ────────────────────────────────────────────────────────────────

fn read_analysis_state_raw(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Option<StoredAnalysisState>> {
    Ok(read_json_optional(&analysis_path(base_dir, conversation_id))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        std::fs::create_dir_all(base.join("conversations")).unwrap();
        super::super::conversations::create_conversation(&base, "c1", "Test").unwrap();
        (base, dir)
    }

    #[test]
    fn test_upsert_and_get() {
        let (base, _dir) = setup();

        assert!(get_analysis_state(&base, "c1").unwrap().is_none());

        upsert_analysis_state(&base, "c1", 2, r#"{"step1":"done"}"#, r#"{"key":"val"}"#)
            .unwrap();

        let state = get_analysis_state(&base, "c1").unwrap().unwrap();
        assert_eq!(state["currentStep"], 2);
        assert_eq!(state["stepStatus"]["step1"], "done");
    }

    #[test]
    fn test_finalize() {
        let (base, _dir) = setup();

        upsert_analysis_state(&base, "c1", 5, r#"{}"#, r#"{}"#).unwrap();
        finalize_analysis(&base, "c1", "completed").unwrap();

        let state = get_analysis_state(&base, "c1").unwrap().unwrap();
        assert_eq!(state["finalStatus"], "completed");
    }

    #[test]
    fn test_reset_stuck() {
        let (base, _dir) = setup();

        upsert_analysis_state(
            &base,
            "c1",
            2,
            r#"{"step1":"completed","step2":"in_progress"}"#,
            r#"{}"#,
        )
        .unwrap();

        reset_stuck_analysis_state(&base, "c1").unwrap();

        let state = get_analysis_state(&base, "c1").unwrap().unwrap();
        assert_eq!(state["stepStatus"]["step2"], "paused");
        assert_eq!(state["stepStatus"]["step1"], "completed");
    }

    #[test]
    fn test_upsert_preserves_final_status() {
        let (base, _dir) = setup();

        upsert_analysis_state(&base, "c1", 5, r#"{}"#, r#"{}"#).unwrap();
        finalize_analysis(&base, "c1", "completed").unwrap();

        // Upsert again should preserve final_status
        upsert_analysis_state(&base, "c1", 5, r#"{"step5":"done"}"#, r#"{}"#).unwrap();

        let state = get_analysis_state(&base, "c1").unwrap().unwrap();
        assert_eq!(state["finalStatus"], "completed");
    }
}
