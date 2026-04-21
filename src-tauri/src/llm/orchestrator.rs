//! Legacy step-state helpers retained for tools that still need per-conversation
//! workflow snapshots on disk.
//!
//! Lotus no longer uses a separate "analysis mode" main loop. This module only
//! exposes minimal storage helpers for reading/updating persisted step state.

use crate::storage::file_store::AppStorage;
use std::sync::Arc;

/// Status of the persisted workflow step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    InProgress,
    Completed,
    Paused,
}

/// Snapshot of the persisted step state read from storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepState {
    pub step: u32,
    pub status: StepStatus,
}

/// Read the current persisted step state for a conversation.
///
/// Returns `None` when there is no active step state or the stored workflow has
/// already been finalized.
pub fn get_step_state(db: &AppStorage, conversation_id: &str) -> Option<StepState> {
    let state = match db.get_analysis_state(conversation_id) {
        Ok(Some(state)) => state,
        _ => return None,
    };

    if state.get("finalStatus").and_then(|v| v.as_str()).is_some() {
        return None;
    }

    let step = state["currentStep"].as_i64().unwrap_or(0) as u32;
    let status_key = format!("step{}_status", step);
    let status = match state
        .get("stepStatus")
        .and_then(|value| value.get(&status_key))
        .and_then(|value| value.as_str())
        .unwrap_or("in_progress")
    {
        "completed" => StepStatus::Completed,
        "paused" => StepStatus::Paused,
        _ => StepStatus::InProgress,
    };

    Some(StepState { step, status })
}

/// Persist a step transition back to storage.
pub fn advance_step(
    db: &Arc<AppStorage>,
    conversation_id: &str,
    step: u32,
    status: &str,
) -> Result<(), String> {
    let step_status = format!(r#"{{"step{}_status":"{}"}}"#, step, status);
    db.upsert_analysis_state(conversation_id, step as i32, &step_status, "{}")
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (AppStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = AppStorage::new(dir.path()).unwrap();
        db.create_conversation("conv", "Test").unwrap();
        (db, dir)
    }

    #[test]
    fn get_step_state_returns_none_without_record() {
        let (db, _dir) = test_db();
        assert_eq!(get_step_state(&db, "conv"), None);
    }

    #[test]
    fn get_step_state_reads_in_progress_status() {
        let (db, _dir) = test_db();
        db.upsert_analysis_state("conv", 2, r#"{\"step2_status\":\"in_progress\"}"#, "{}")
            .unwrap();

        assert_eq!(
            get_step_state(&db, "conv"),
            Some(StepState {
                step: 2,
                status: StepStatus::InProgress,
            })
        );
    }

    #[test]
    fn get_step_state_ignores_finalized_records() {
        let (db, _dir) = test_db();
        db.upsert_analysis_state("conv", 3, r#"{\"step3_status\":\"completed\"}"#, "{}")
            .unwrap();
        db.finalize_analysis("conv", "completed").unwrap();

        assert_eq!(get_step_state(&db, "conv"), None);
    }

    #[test]
    fn advance_step_persists_status() {
        let (db, _dir) = test_db();
        let db = Arc::new(db);
        advance_step(&db, "conv", 4, "completed").unwrap();

        assert_eq!(
            get_step_state(&db, "conv"),
            Some(StepState {
                step: 4,
                status: StepStatus::Completed,
            })
        );
    }
}
