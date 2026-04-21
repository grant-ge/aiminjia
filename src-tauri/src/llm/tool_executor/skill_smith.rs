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
///
/// Empty string is treated as "no binding" — this is how install clears
/// the binding after a successful commit (see `handle_skill_smith_install`)
/// without requiring a new `delete_memory` API on AppStorage.
pub(crate) fn lookup_bound_draft(ctx: &PluginContext) -> Result<Option<String>> {
    Ok(ctx
        .storage
        .get_memory(&draft_key(&ctx.conversation_id))?
        .filter(|s| !s.is_empty()))
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

/// Install the active draft into `~/.renlijia/skills/{skill_id}/` and trigger
/// hot-reload. On conflict (skill with same id already installed) and
/// `force=false`, returns `status: "conflict"` without changes — the LLM
/// should then ask the user whether to rename or force-overwrite.
///
/// On success, the underlying draft is cleaned up (see T4). Any subsequent
/// write/validate/dry_run against the old draft_id will fail with
/// "Draft not found"; if the user wants to create another skill in the same
/// conversation, call `skill_smith_create_draft` with `force_new=true`.
pub(crate) async fn handle_skill_smith_install(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let draft_id = resolve_draft_id(ctx, args)?;
    let force = args
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let app = ctx
        .app_handle
        .as_ref()
        .ok_or_else(|| anyhow!("skill_smith tools require AppHandle"))?;

    let result = if force {
        crate::commands::skill_smith::commit::commit_skill_draft_force(
            app.clone(),
            draft_id.clone(),
        )
        .await
    } else {
        crate::commands::skill_smith::commit::commit_skill_draft(app.clone(), draft_id.clone())
            .await
    }
    .map_err(|e| anyhow!("commit_skill_draft failed: {}", e))?;

    if result.conflict {
        // No changes made; LLM asks user, then may re-invoke with force=true.
        return Ok(json!({
            "status": "conflict",
            "skill_id": result.skill_id,
            "installed_path": result.installed_path,
            "message": format!(
                "A skill with id '{}' is already installed. Ask the user whether to overwrite (re-call with force=true) or rename (write a new plugin.toml with a different plugin.id and retry).",
                result.skill_id
            ),
        })
        .to_string());
    }

    // Success — clear session binding so subsequent calls don't point at
    // the now-removed draft. `set_memory` with "" is the cheapest way to
    // invalidate without adding a new delete API; resolve_draft_id's
    // empty-string guard treats it as no binding. If clear fails (rare —
    // disk full?), warn so operators see it but don't fail the install
    // (the skill is already on disk).
    if let Err(e) = ctx.storage.set_memory(
        &draft_key(&ctx.conversation_id),
        "",
        Some("skill_smith_session"),
    ) {
        log::warn!(
            "skill_smith_install: skill committed but failed to clear session binding for conversation {}: {}. Future tool calls in this conversation will fail with 'Draft not found' until force_new=true on create_draft.",
            ctx.conversation_id,
            e
        );
    }

    Ok(json!({
        "status": "installed",
        "skill_id": result.skill_id,
        "installed_path": result.installed_path,
        "message": format!(
            "Skill '{}' installed to {}. It's live immediately (no restart). The draft has been cleaned up.",
            result.skill_id, result.installed_path
        ),
    })
    .to_string())
}

/// Package the active draft as a `.aijia-skill` zip at `output_dir`.
/// Unlike install, the draft is preserved. If `output_dir` is not provided,
/// defaults to `{workspace}/exports/`.
pub(crate) async fn handle_skill_smith_export(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let draft_id = resolve_draft_id(ctx, args)?;
    let app = ctx
        .app_handle
        .as_ref()
        .ok_or_else(|| anyhow!("skill_smith tools require AppHandle"))?;

    // Resolve output dir: explicit arg wins, else default to workspace/exports/.
    let output_dir = match args.get("output_dir").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            let exports = ctx.workspace_path.join("exports");
            std::fs::create_dir_all(&exports)
                .map_err(|e| anyhow!("Failed to create default exports dir: {}", e))?;
            exports.to_string_lossy().to_string()
        }
    };

    let path = crate::commands::skill_smith::commit::export_skill_draft(
        app.clone(),
        draft_id.clone(),
        output_dir.clone(),
    )
    .await
    .map_err(|e| anyhow!("export_skill_draft failed: {}", e))?;

    Ok(json!({
        "status": "exported",
        "draft_id": draft_id,
        "output_path": path,
        "message": format!(
            "Skill packed to {}. Share this .aijia-skill file — recipients can install it via Settings → Skills → Install from folder.",
            path
        ),
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

    // ---- Empty-string binding sentinel (install post-success cleanup) ----

    #[tokio::test]
    async fn empty_string_binding_is_treated_as_no_binding() {
        // Simulate install's post-success cleanup: it writes "" to the
        // session key, and lookup_bound_draft should return None.
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "").unwrap();

        assert!(lookup_bound_draft(&ctx).unwrap().is_none());
    }

    #[tokio::test]
    async fn create_draft_creates_new_after_install_cleared_binding() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        // First bind a draft, then simulate install clearing it.
        bind_draft(&ctx, "pre-install-d").unwrap();
        bind_draft(&ctx, "").unwrap();

        // Without AppHandle, create_draft now hits the "no binding" path and
        // errors on AppHandle (correct — it would try to actually create).
        let err = handle_skill_smith_create_draft(&ctx, &json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("AppHandle"));
    }

    // ---- install / export handlers ----

    #[tokio::test]
    async fn install_without_app_handle_errors() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "any-draft-id").unwrap();

        let err = handle_skill_smith_install(&ctx, &json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("AppHandle"));
    }

    #[tokio::test]
    async fn install_errors_when_no_draft_bound_or_explicit() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let err = handle_skill_smith_install(&ctx, &json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No draft_id provided"));
    }

    #[tokio::test]
    async fn export_without_app_handle_errors() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);
        bind_draft(&ctx, "any-draft-id").unwrap();

        let err = handle_skill_smith_export(&ctx, &json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("AppHandle"));
    }
}
