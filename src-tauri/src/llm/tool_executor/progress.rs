//! update_progress handler.

use anyhow::Result;
use serde_json::{json, Value};
use tauri::Emitter;

use crate::plugin::context::PluginContext;

use super::{optional_str, require_str};

/// 10. update_progress — update the analysis progress state.
pub(crate) async fn handle_update_progress(ctx: &PluginContext, args: &Value) -> Result<String> {
    let current_step = super::optional_i64(args, "current_step", 1) as i32;
    let step_status = require_str(args, "step_status")?;
    let summary = optional_str(args, "summary").unwrap_or("");

    let state_data = json!({
        "summary": summary,
    })
    .to_string();

    let step_status_json = json!({
        format!("step_{}", current_step): step_status,
    })
    .to_string();

    ctx.storage.upsert_analysis_state(
        &ctx.conversation_id,
        current_step,
        &step_status_json,
        &state_data,
    )?;

    if let Some(ref app) = ctx.app_handle {
        let _ = app.emit("analysis:step-changed", serde_json::json!({
            "step": current_step,
            "status": step_status,
        }));
    }

    Ok(json!({
        "status": "updated",
        "currentStep": current_step,
        "stepStatus": step_status,
        "summary": summary,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::tool_executor::tests::{create_test_context, create_test_db};

    #[tokio::test]
    async fn test_handle_update_progress() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({
            "current_step": 3,
            "step_status": "completed",
            "summary": "Statistical analysis done"
        });

        let result = handle_update_progress(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["status"], "updated");
        assert_eq!(parsed["currentStep"], 3);
        assert_eq!(parsed["stepStatus"], "completed");

        // Verify via database.
        let state = ctx.storage.get_analysis_state(&ctx.conversation_id).unwrap();
        assert!(state.is_some());
        assert_eq!(state.unwrap()["currentStep"], 3);
    }

    #[tokio::test]
    async fn test_handle_update_progress_missing_status() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({"current_step": 1});
        let result = handle_update_progress(&ctx, &args).await;
        assert!(result.is_err());
    }
}
