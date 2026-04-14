//! LLM tool handlers for the skill-smith conversational skill creation flow
//! (Phase 11 M3.1 — minimal bootstrap subset).
//!
//! These handlers wrap the T2-T7 Tauri commands so the LLM (running inside
//! the skill-smith workflow) can call them as regular `tools_only`. Session
//! state (which `draft_id` belongs to which conversation) is tracked via
//! `AppStorage::set_memory` keyed by `skill_smith:{conversation_id}:draft_id`
//! — same mechanism save_analysis_note already uses for per-conversation
//! storage, so we don't need a new schema.
//!
//! ## Subset covered in M3.1 MVP
//!
//! | Handler | Wraps |
//! |---|---|
//! | `handle_skill_smith_create_draft` | T2 create_skill_draft |
//! | `handle_skill_smith_write_file`   | T2 write_skill_draft_file (generic) |
//! | `handle_skill_smith_validate`     | T3 validate_skill_draft |
//! | `handle_skill_smith_dry_run`      | T7 dry_run_skill_draft |
//!
//! ## Deferred to M3.1-b
//!
//! - `install` / `export` — wraps T4 commit/export
//! - Structured variants: `write_plugin_manifest` / `write_workflow` — these
//!   take object fields and Rust-side serialize to TOML, eliminating LLM
//!   syntax errors. M3.1 MVP uses generic `write_file` (LLM writes TOML text
//!   itself, relying on validate → repair loop).

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::plugin::context::PluginContext;

use super::require_str;

// ---------------------------------------------------------------------------
// Session state helpers
// ---------------------------------------------------------------------------

/// Storage key for "which draft does this conversation own?"
fn draft_key(conversation_id: &str) -> String {
    format!("skill_smith:{}:draft_id", conversation_id)
}

/// Look up the draft_id bound to this conversation, if any.
pub(crate) fn lookup_bound_draft(ctx: &PluginContext) -> Result<Option<String>> {
    ctx.storage.get_memory(&draft_key(&ctx.conversation_id))
}

/// Bind a draft_id to the current conversation (overwrites any prior binding).
fn bind_draft(ctx: &PluginContext, draft_id: &str) -> Result<()> {
    ctx.storage.set_memory(
        &draft_key(&ctx.conversation_id),
        draft_id,
        Some("skill_smith_session"),
    )?;
    Ok(())
}

/// Resolve the draft_id to use for this call:
/// 1. Explicit `draft_id` arg (preferred — LLM should pass it).
/// 2. Fallback to session-bound draft_id from storage (resilient to LLM
///    context loss / retry).
fn resolve_draft_id(ctx: &PluginContext, args: &Value) -> Result<String> {
    if let Some(explicit) = args.get("draft_id").and_then(|v| v.as_str()) {
        if !explicit.is_empty() {
            return Ok(explicit.to_string());
        }
    }
    lookup_bound_draft(ctx)?
        .ok_or_else(|| anyhow!("No draft_id provided and no draft bound to this conversation; call skill_smith_create_draft first"))
}

// ---------------------------------------------------------------------------
// Handlers (async; called by ToolPlugin wrappers in plugin/builtin/tools/)
// ---------------------------------------------------------------------------

/// Create a new draft and bind it to the current conversation.
///
/// Idempotent: if this conversation already has a draft, returns the existing
/// `draft_id` with `already_bound: true`. Pass `force_new: true` to forcibly
/// orphan the old draft and create a fresh one (the orphan gets GC'd after
/// 7 days by cleanup_expired_drafts).
pub(crate) async fn handle_skill_smith_create_draft(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let force_new = args
        .get("force_new")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Idempotent path: reuse existing binding unless force_new.
    if !force_new {
        if let Some(existing) = lookup_bound_draft(ctx)? {
            return Ok(json!({
                "status": "ok",
                "draft_id": existing,
                "already_bound": true,
            })
            .to_string());
        }
    }

    let app = ctx
        .app_handle
        .as_ref()
        .ok_or_else(|| anyhow!("skill_smith tools require AppHandle (not available in sub-agent context)"))?;
    let draft_id =
        crate::commands::skill_smith::create_skill_draft(app.clone()).await
            .map_err(|e| anyhow!("create_skill_draft failed: {}", e))?;

    bind_draft(ctx, &draft_id)?;

    Ok(json!({
        "status": "created",
        "draft_id": draft_id,
        "already_bound": false,
    })
    .to_string())
}

/// Write a file inside the draft (generic; LLM provides raw content as a
/// string — for TOML files LLM must produce valid syntax; the follow-up
/// validate call will report errors).
///
/// Required args: `relative_path`, `content`. `draft_id` optional (falls
/// back to session binding).
pub(crate) async fn handle_skill_smith_write_file(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let draft_id = resolve_draft_id(ctx, args)?;
    let relative_path = require_str(args, "relative_path")?;
    let content = require_str(args, "content")?;

    let app = ctx
        .app_handle
        .as_ref()
        .ok_or_else(|| anyhow!("skill_smith tools require AppHandle"))?;

    crate::commands::skill_smith::write_skill_draft_file(
        app.clone(),
        draft_id.clone(),
        relative_path.to_string(),
        content.to_string(),
    )
    .await
    .map_err(|e| anyhow!("write_skill_draft_file failed: {}", e))?;

    Ok(json!({
        "status": "written",
        "draft_id": draft_id,
        "relative_path": relative_path,
        "size_bytes": content.len(),
    })
    .to_string())
}

/// Validate the draft against the skill schema. Mirrors T3 output structure
/// so the LLM can consume `errors[].fix_hint` for auto-repair.
pub(crate) async fn handle_skill_smith_validate(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let draft_id = resolve_draft_id(ctx, args)?;
    let app = ctx
        .app_handle
        .as_ref()
        .ok_or_else(|| anyhow!("skill_smith tools require AppHandle"))?;

    let report = crate::commands::skill_smith::validation::validate_skill_draft(
        app.clone(),
        draft_id.clone(),
    )
    .await
    .map_err(|e| anyhow!("validate_skill_draft failed: {}", e))?;

    // Wrap the full report in our tool response (valid/errors/warnings/summary
    // already serde-serialize as camelCase-ish; LLM reads them directly).
    let value = serde_json::to_value(&report)?;
    Ok(json!({
        "draft_id": draft_id,
        "report": value,
    })
    .to_string())
}

/// Run 6-check dry-run (schema / prompts-reference / prompts-content /
/// python-scripts / knowledge / loadable). Safe side-effect-free read.
pub(crate) async fn handle_skill_smith_dry_run(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let draft_id = resolve_draft_id(ctx, args)?;
    let app = ctx
        .app_handle
        .as_ref()
        .ok_or_else(|| anyhow!("skill_smith tools require AppHandle"))?;

    let report =
        crate::commands::skill_smith::dry_run::dry_run_skill_draft(app.clone(), draft_id.clone())
            .await
            .map_err(|e| anyhow!("dry_run_skill_draft failed: {}", e))?;

    let value = serde_json::to_value(&report)?;
    Ok(json!({
        "draft_id": draft_id,
        "report": value,
    })
    .to_string())
}

// ---------------------------------------------------------------------------
// Tests (session state + arg resolution logic — AppHandle-dependent paths
// are covered by T2/T3/T7's own tests; we don't re-exercise them here.)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::tool_executor::tests::{create_test_context, create_test_db};

    #[test]
    fn draft_key_includes_conversation_id() {
        assert_eq!(draft_key("conv-42"), "skill_smith:conv-42:draft_id");
    }

    #[tokio::test]
    async fn lookup_returns_none_when_no_binding() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        assert!(lookup_bound_draft(&ctx).unwrap().is_none());
    }

    #[tokio::test]
    async fn bind_then_lookup_round_trips() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        bind_draft(&ctx, "abc123def456").unwrap();
        assert_eq!(lookup_bound_draft(&ctx).unwrap().as_deref(), Some("abc123def456"));
    }

    #[tokio::test]
    async fn resolve_prefers_explicit_arg() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "bound-draft12").unwrap();

        let args = json!({ "draft_id": "explicit-one" });
        assert_eq!(resolve_draft_id(&ctx, &args).unwrap(), "explicit-one");
    }

    #[tokio::test]
    async fn resolve_falls_back_to_session_binding() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "bound-draftxy").unwrap();

        let args = json!({});
        assert_eq!(resolve_draft_id(&ctx, &args).unwrap(), "bound-draftxy");
    }

    #[tokio::test]
    async fn resolve_empty_explicit_falls_through_to_binding() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "bound-draftxy").unwrap();

        let args = json!({ "draft_id": "" });
        assert_eq!(resolve_draft_id(&ctx, &args).unwrap(), "bound-draftxy");
    }

    #[tokio::test]
    async fn resolve_errors_when_no_explicit_and_no_binding() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let err = resolve_draft_id(&ctx, &json!({})).unwrap_err();
        assert!(err.to_string().contains("No draft_id provided"));
    }

    #[tokio::test]
    async fn create_draft_without_app_handle_errors_gracefully() {
        // create_test_context sets app_handle = None. This covers the
        // "sub-agent context can't create drafts" path.
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let err = handle_skill_smith_create_draft(&ctx, &json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("AppHandle"));
    }

    #[tokio::test]
    async fn create_draft_reuses_binding_when_not_force_new() {
        // Pre-bind a draft; create_draft should return it without touching
        // AppHandle. This proves idempotency works even when AppHandle is
        // absent (sub-agent context).
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "preexisting01").unwrap();

        let result = handle_skill_smith_create_draft(&ctx, &json!({}))
            .await
            .expect("should succeed without AppHandle when binding exists");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["draft_id"], "preexisting01");
        assert_eq!(parsed["already_bound"], true);
    }

    #[tokio::test]
    async fn create_draft_force_new_requires_app_handle() {
        // force_new=true must bypass the binding shortcut and try to create,
        // which fails without AppHandle — test context has none.
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "preexisting01").unwrap();

        let err = handle_skill_smith_create_draft(&ctx, &json!({"force_new": true}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("AppHandle"));
    }
}
