//! save_analysis_note handler.

use anyhow::Result;
use serde_json::{json, Value};

use crate::plugin::context::PluginContext;

use super::require_str;

/// 8. save_analysis_note — store an intermediate finding in enterprise memory.
pub(crate) async fn handle_save_analysis_note(ctx: &PluginContext, args: &Value) -> Result<String> {
    let key = require_str(args, "key")?;
    let content = require_str(args, "content")?;
    let step = super::optional_i64(args, "step", 0);

    // Prefix key with conversation id for scoping.
    let full_key = format!("note:{}:{}", ctx.conversation_id, key);
    let source = format!("analysis_step_{}", step);

    ctx.storage.set_memory(&full_key, content, Some(&source))?;

    Ok(json!({
        "status": "saved",
        "key": key,
        "step": step,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::tool_executor::tests::{create_test_context, create_test_db};

    #[tokio::test]
    async fn test_handle_save_analysis_note() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({
            "key": "salary_distribution",
            "content": "Salary follows a log-normal distribution",
            "step": 2
        });

        let result = handle_save_analysis_note(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["status"], "saved");
        assert_eq!(parsed["key"], "salary_distribution");
        assert_eq!(parsed["step"], 2);

        // Verify the memory was actually stored.
        let full_key = format!("note:{}:salary_distribution", ctx.conversation_id);
        let stored = ctx.storage.get_memory(&full_key).unwrap();
        assert_eq!(stored, Some("Salary follows a log-normal distribution".to_string()));
    }

    #[tokio::test]
    async fn test_handle_save_analysis_note_missing_key() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({"content": "some note"});
        let result = handle_save_analysis_note(&ctx, &args).await;
        assert!(result.is_err());
    }
}
