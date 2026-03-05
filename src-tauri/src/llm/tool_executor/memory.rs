//! Cognitive memory tool handlers.
//!
//! Four tools: save_memory, search_memory, load_core_memory, distill_memories.

use anyhow::Result;
use serde_json::{json, Value};

use crate::plugin::context::PluginContext;

// ─── save_memory ────────────────────────────────────────────────────────────

pub(crate) async fn handle_save_memory(ctx: &PluginContext, args: &Value) -> Result<String> {
    let content = super::require_str(args, "content")?;
    let category = super::require_str(args, "category")?;
    let to_core = args.get("to_core").and_then(|v| v.as_bool()).unwrap_or(false);

    let tags: Vec<String> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let (id, was_to_core) = ctx.storage.save_cognitive_memory(
        content,
        category,
        &tags,
        &ctx.conversation_id,
        to_core,
    )?;

    Ok(json!({
        "status": "saved",
        "id": id,
        "category": category,
        "to_core": was_to_core,
    })
    .to_string())
}

// ─── search_memory ──────────────────────────────────────────────────────────

pub(crate) async fn handle_search_memory(ctx: &PluginContext, args: &Value) -> Result<String> {
    let query = super::require_str(args, "query")?;
    let category = super::optional_str(args, "category");
    let days = super::optional_i64(args, "days", 30);

    let results = ctx.storage.search_cognitive_memory(
        query,
        category,
        days,
        &ctx.conversation_id,
    )?;

    Ok(json!({
        "status": "ok",
        "count": results.len(),
        "results": results,
    })
    .to_string())
}

// ─── load_core_memory ───────────────────────────────────────────────────────

pub(crate) async fn handle_load_core_memory(ctx: &PluginContext, _args: &Value) -> Result<String> {
    let content = ctx.storage.load_core_memory();

    if content.is_empty() {
        Ok(json!({
            "status": "empty",
            "content": "",
            "message": "No core memory exists yet. Use save_memory to build knowledge.",
        })
        .to_string())
    } else {
        let line_count = content.lines().count();
        Ok(json!({
            "status": "ok",
            "content": content,
            "line_count": line_count,
        })
        .to_string())
    }
}

// ─── distill_memories ───────────────────────────────────────────────────────

pub(crate) async fn handle_distill_memories(ctx: &PluginContext, args: &Value) -> Result<String> {
    let days = super::optional_i64(args, "days", 7);
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let report = ctx.storage.distill_cognitive_memories(days, dry_run)?;

    Ok(json!({
        "status": "ok",
        "dry_run": dry_run,
        "promoted": report.promoted,
        "skipped_dup": report.skipped_dup,
        "demoted": report.demoted,
        "archived": report.archived,
        "core_lines": report.core_lines,
    })
    .to_string())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::tool_executor::tests::{create_test_context, create_test_db};

    #[tokio::test]
    async fn test_handle_save_memory() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({
            "content": "User prefers box plots for salary distribution",
            "category": "preference",
            "tags": ["boxplot", "salary"]
        });

        let result = handle_save_memory(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "saved");
        assert_eq!(parsed["category"], "preference");
        assert_eq!(parsed["to_core"], false);
    }

    #[tokio::test]
    async fn test_handle_save_memory_to_core() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({
            "content": "Company has 500 employees across 3 offices",
            "category": "fact",
            "to_core": true,
            "tags": ["company", "headcount"]
        });

        let result = handle_save_memory(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "saved");
        assert_eq!(parsed["to_core"], true);
    }

    #[tokio::test]
    async fn test_handle_save_memory_validation() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        // Too short
        let args = json!({ "content": "short", "category": "fact" });
        assert!(handle_save_memory(&ctx, &args).await.is_err());

        // Missing category
        let args = json!({ "content": "valid content length here" });
        assert!(handle_save_memory(&ctx, &args).await.is_err());
    }

    #[tokio::test]
    async fn test_handle_search_memory() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        // Save first
        let args = json!({
            "content": "User prefers box plots for salary distribution",
            "category": "preference",
            "tags": ["boxplot", "salary"]
        });
        handle_save_memory(&ctx, &args).await.unwrap();

        // Search
        let search_args = json!({ "query": "box plot salary" });
        let result = handle_search_memory(&ctx, &search_args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["count"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_handle_load_core_memory_empty() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let result = handle_load_core_memory(&ctx, &json!({})).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "empty");
    }

    #[tokio::test]
    async fn test_handle_load_core_memory_with_content() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        // Save to core first
        let args = json!({
            "content": "Company has 500 employees across 3 offices",
            "category": "fact",
            "to_core": true,
        });
        handle_save_memory(&ctx, &args).await.unwrap();

        let result = handle_load_core_memory(&ctx, &json!({})).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["content"].as_str().unwrap().contains("500 employees"));
    }

    #[tokio::test]
    async fn test_handle_distill_memories() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({ "days": 7, "dry_run": true });
        let result = handle_distill_memories(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["dry_run"], true);
    }
}
