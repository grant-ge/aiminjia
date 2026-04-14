//! Skill-Smith — conversational skill creation infrastructure.
//!
//! Submodules:
//! - (this file) **draft file system** (T2): manages the isolated scratchpad
//!   under `{app_data}/_drafts/{draft_id}/` where the creation flow writes WIP
//!   `plugin.toml`, `workflow.toml`, prompts, and (later) scripts/knowledge
//!   before committing to `custom_plugins/`.
//! - [`validation`] (T3): parses draft TOML files against the skill schema and
//!   returns a structured report that can be fed back to the LLM for
//!   automatic repair.
//!
//! See `docs/plans/2026-04-14-skill-smith-m2-tasks.md`.
//!
//! ## Path safety
//!
//! All write paths go through `resolve_draft_file` which performs
//! component-level validation (no `..`, no absolute paths, no leading-dot
//! components, depth/size caps) before joining. Defense-in-depth vs. path
//! traversal without relying on canonicalize (which requires the file to
//! already exist).

pub mod commit;
pub mod validation;
// Command functions are registered via their full path
// (e.g. `commands::skill_smith::validation::validate_skill_draft`) in `lib.rs`
// because Tauri's `generate_handler!` macro needs the `__cmd__` helper to live
// in the same module as the `#[tauri::command]` attribute.

use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Manager};

const DRAFTS_DIR_NAME: &str = "_drafts";
const DRAFT_EXPIRY_DAYS: u64 = 7;
const MAX_DRAFT_FILE_SIZE: u64 = 1_000_000; // 1MB per file
const MAX_RELATIVE_PATH_LEN: usize = 512;
const MAX_PATH_DEPTH: usize = 5;

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// True iff `s` is exactly 12 lowercase hexadecimal characters.
/// Kept as a plain loop to avoid pulling in `regex` for one pattern.
fn is_valid_draft_id(s: &str) -> bool {
    s.len() == 12 && s.chars().all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
}

fn validate_draft_id(draft_id: &str) -> Result<(), String> {
    if !is_valid_draft_id(draft_id) {
        return Err(format!(
            "Invalid draft_id '{}': must be 12 lowercase hex characters",
            draft_id
        ));
    }
    Ok(())
}

/// Component-level whitelist validation for a user-supplied relative path.
/// Rejects `..`, absolute paths, Windows prefixes, leading-dot files/dirs,
/// empty strings, over-long strings, and excessive nesting.
fn validate_relative_path(rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() {
        return Err("relative_path cannot be empty".to_string());
    }
    if rel.len() > MAX_RELATIVE_PATH_LEN {
        return Err(format!(
            "relative_path too long ({} chars, limit {})",
            rel.len(),
            MAX_RELATIVE_PATH_LEN
        ));
    }

    let path = Path::new(rel);
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    return Err(format!(
                        "Leading-dot path component not allowed: '{}'",
                        name_str
                    ));
                }
                depth += 1;
            }
            Component::ParentDir => {
                return Err("'..' components not allowed".to_string());
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("Absolute paths not allowed".to_string());
            }
            Component::CurDir => {
                // "./" is harmless, skip
            }
        }
    }

    if depth == 0 {
        return Err("relative_path must contain at least one component".to_string());
    }
    if depth > MAX_PATH_DEPTH {
        return Err(format!(
            "relative_path too deep ({} levels, max {})",
            depth, MAX_PATH_DEPTH
        ));
    }

    Ok(path.to_path_buf())
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

fn drafts_root(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let root = app_data.join(DRAFTS_DIR_NAME);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

pub(crate) fn draft_dir(app: &AppHandle, draft_id: &str) -> Result<PathBuf, String> {
    validate_draft_id(draft_id)?;
    Ok(drafts_root(app)?.join(draft_id))
}

fn resolve_draft_file(app: &AppHandle, draft_id: &str, rel: &str) -> Result<PathBuf, String> {
    validate_draft_id(draft_id)?;
    let rel_pb = validate_relative_path(rel)?;
    let dir = drafts_root(app)?.join(draft_id);
    if !dir.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }
    Ok(dir.join(rel_pb))
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct DraftFile {
    pub relative_path: String,
    pub size: u64,
    pub modified_ts: i64,
}

#[derive(Serialize)]
pub struct DraftSummary {
    pub draft_id: String,
    pub created_at: i64,
    pub last_modified: i64,
    /// One of: "intent" | "manifest" | "workflow" | "prompts"
    /// (further stages like "ready" are added by T3 validate / T7 dry-run)
    pub stage: String,
    pub skill_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Create a new empty draft. Returns the 12-char hex draft_id.
#[tauri::command]
pub async fn create_skill_draft(app: AppHandle) -> Result<String, String> {
    let root = drafts_root(&app)?;
    // Generate unique ID; retry on (astronomically unlikely) collision.
    for _ in 0..10 {
        let raw = uuid::Uuid::new_v4().simple().to_string();
        let id = raw[..12].to_string();
        let dir = root.join(&id);
        if !dir.exists() {
            std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
            return Ok(id);
        }
    }
    Err("Failed to allocate unique draft_id after 10 attempts".to_string())
}

/// Write `content` to `{draft}/{relative_path}`, creating parent directories.
#[tauri::command]
pub async fn write_skill_draft_file(
    app: AppHandle,
    draft_id: String,
    relative_path: String,
    content: String,
) -> Result<(), String> {
    let target = resolve_draft_file(&app, &draft_id, &relative_path)?;
    if content.len() as u64 > MAX_DRAFT_FILE_SIZE {
        return Err(format!(
            "File too large ({} bytes, limit {})",
            content.len(),
            MAX_DRAFT_FILE_SIZE
        ));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Read `{draft}/{relative_path}` as UTF-8 string.
#[tauri::command]
pub async fn read_skill_draft_file(
    app: AppHandle,
    draft_id: String,
    relative_path: String,
) -> Result<String, String> {
    let target = resolve_draft_file(&app, &draft_id, &relative_path)?;
    if !target.is_file() {
        return Err(format!("File '{}' not found in draft", relative_path));
    }
    std::fs::read_to_string(&target).map_err(|e| e.to_string())
}

/// Recursively list all files in a draft.
#[tauri::command]
pub async fn list_skill_draft_files(
    app: AppHandle,
    draft_id: String,
) -> Result<Vec<DraftFile>, String> {
    let dir = draft_dir(&app, &draft_id)?;
    if !dir.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }
    let mut files = Vec::new();
    walk_draft_dir(&dir, &dir, &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

/// List all known drafts (for WelcomeScreen / SkillsTab banner resume UI).
/// Sorted by last_modified desc.
#[tauri::command]
pub async fn list_skill_drafts(app: AppHandle) -> Result<Vec<DraftSummary>, String> {
    let root = drafts_root(&app)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !is_valid_draft_id(name) {
            continue;
        }
        if let Ok(summary) = summarize_draft(&path, name) {
            out.push(summary);
        }
    }
    out.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(out)
}

/// Remove a draft directory entirely.
#[tauri::command]
pub async fn discard_skill_draft(app: AppHandle, draft_id: String) -> Result<(), String> {
    let dir = draft_dir(&app, &draft_id)?;
    if !dir.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(())
}

/// Scan drafts dir and delete anything untouched for > DRAFT_EXPIRY_DAYS.
/// Called once at app startup (non-blocking). Returns count removed.
#[tauri::command]
pub async fn cleanup_expired_drafts(app: AppHandle) -> Result<usize, String> {
    let root = drafts_root(&app)?;
    let now = Utc::now().timestamp();
    let expiry_seconds = (DRAFT_EXPIRY_DAYS as i64) * 24 * 60 * 60;
    let mut removed = 0usize;

    for entry in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !is_valid_draft_id(name) {
            continue;
        }

        let last_mod = compute_last_modified(&path);
        if now - last_mod > expiry_seconds && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }

    Ok(removed)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn walk_draft_dir(
    root: &Path,
    current: &Path,
    out: &mut Vec<DraftFile>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            walk_draft_dir(root, &path, out)?;
        } else if path.is_file() {
            let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or_else(|| Utc::now().timestamp());
            // Normalize separators to forward-slash (cross-platform TS consumer)
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            out.push(DraftFile {
                relative_path: rel_str,
                size: meta.len(),
                modified_ts: modified,
            });
        }
    }
    Ok(())
}

fn compute_last_modified(dir: &Path) -> i64 {
    let mut files = Vec::new();
    let _ = walk_draft_dir(dir, dir, &mut files);
    if let Some(max) = files.iter().map(|f| f.modified_ts).max() {
        return max;
    }
    // Fallback to the directory's own mtime (empty draft just created)
    std::fs::metadata(dir)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn summarize_draft(path: &Path, draft_id: &str) -> Result<DraftSummary, String> {
    let dir_meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let created_at = dir_meta
        .created()
        .ok()
        .or_else(|| dir_meta.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let last_modified = compute_last_modified(path).max(created_at);

    // Infer stage from files present
    let has_plugin = path.join("plugin.toml").is_file();
    let has_workflow = path.join("workflow.toml").is_file();
    let has_prompts = path
        .join("prompts")
        .read_dir()
        .ok()
        .map(|rd| {
            rd.filter_map(Result::ok).any(|ent| {
                ent.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let stage = if !has_plugin {
        "intent"
    } else if !has_workflow {
        "manifest"
    } else if !has_prompts {
        "workflow"
    } else {
        "prompts"
    }
    .to_string();

    // Try to read skill name from plugin.toml (best-effort; ignore parse errors)
    let skill_name = std::fs::read_to_string(path.join("plugin.toml"))
        .ok()
        .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
        .and_then(|v| v.get("plugin").cloned())
        .and_then(|p| p.get("name").cloned())
        .and_then(|n| n.as_str().map(|s| s.to_string()));

    Ok(DraftSummary {
        draft_id: draft_id.to_string(),
        created_at,
        last_modified,
        stage,
        skill_name,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- draft_id validator ----

    #[test]
    fn is_valid_draft_id_accepts_valid() {
        assert!(is_valid_draft_id("abc123def456"));
        assert!(is_valid_draft_id("0123456789ab"));
        assert!(is_valid_draft_id("ffffffffffff"));
    }

    #[test]
    fn is_valid_draft_id_rejects_invalid() {
        assert!(!is_valid_draft_id("ABC123DEF456")); // uppercase
        assert!(!is_valid_draft_id("abc123def")); // too short
        assert!(!is_valid_draft_id("abc123def4567")); // too long
        assert!(!is_valid_draft_id("abc123-ef456")); // dash
        assert!(!is_valid_draft_id("abc123_ef456")); // underscore
        assert!(!is_valid_draft_id("abc123zef456")); // non-hex letter
        assert!(!is_valid_draft_id(""));
        assert!(!is_valid_draft_id("../../escape"));
    }

    // ---- relative path validator ----

    #[test]
    fn validate_relative_path_accepts_valid() {
        assert!(validate_relative_path("plugin.toml").is_ok());
        assert!(validate_relative_path("prompts/step0.md").is_ok());
        assert!(validate_relative_path("scripts/knowledge/benchmarks.json").is_ok());
        assert!(validate_relative_path("./prompts/base.md").is_ok()); // CurDir OK
    }

    #[test]
    fn validate_relative_path_rejects_parent_traversal() {
        assert!(validate_relative_path("../etc/passwd").is_err());
        assert!(validate_relative_path("prompts/../../secret").is_err());
        assert!(validate_relative_path("..").is_err());
        assert!(validate_relative_path("a/../b").is_err());
    }

    #[test]
    fn validate_relative_path_rejects_absolute() {
        assert!(validate_relative_path("/etc/passwd").is_err());
        assert!(validate_relative_path("/plugin.toml").is_err());
    }

    #[test]
    fn validate_relative_path_rejects_hidden() {
        assert!(validate_relative_path(".git/config").is_err());
        assert!(validate_relative_path("prompts/.env").is_err());
        assert!(validate_relative_path(".hidden.md").is_err());
    }

    #[test]
    fn validate_relative_path_rejects_empty() {
        assert!(validate_relative_path("").is_err());
    }

    #[test]
    fn validate_relative_path_rejects_too_deep() {
        assert!(validate_relative_path("a/b/c/d/e/f/g").is_err()); // 7 levels
        assert!(validate_relative_path("a/b/c/d/e").is_ok()); // 5 levels (boundary)
        assert!(validate_relative_path("a/b/c/d/e/f").is_err()); // 6 levels
    }

    #[test]
    fn validate_relative_path_rejects_too_long() {
        let long = "a/".repeat(MAX_RELATIVE_PATH_LEN) + "file.md";
        assert!(validate_relative_path(&long).is_err());
    }

    #[test]
    fn validate_draft_id_error_message_helpful() {
        let err = validate_draft_id("BAD").unwrap_err();
        assert!(err.contains("Invalid draft_id"));
        assert!(err.contains("12 lowercase hex"));
    }

    // ---- File system integration tests via tempfile ----
    //
    // These exercise the non-Tauri helpers directly (which is where the
    // safety logic lives). The async #[tauri::command] wrappers are pure
    // thin shims around these.

    #[test]
    fn walk_draft_dir_finds_all_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("plugin.toml"), "a").unwrap();
        std::fs::create_dir_all(root.join("prompts")).unwrap();
        std::fs::write(root.join("prompts/step0.md"), "hello").unwrap();
        std::fs::create_dir_all(root.join("scripts/knowledge")).unwrap();
        std::fs::write(root.join("scripts/knowledge/rules.json"), "{}").unwrap();

        let mut out = Vec::new();
        walk_draft_dir(root, root, &mut out).unwrap();
        let paths: std::collections::HashSet<_> =
            out.iter().map(|f| f.relative_path.clone()).collect();

        assert_eq!(paths.len(), 3);
        assert!(paths.contains("plugin.toml"));
        assert!(paths.contains("prompts/step0.md"));
        assert!(paths.contains("scripts/knowledge/rules.json"));
    }

    #[test]
    fn summarize_draft_infers_intent_stage_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let summary = summarize_draft(tmp.path(), "000000000000").unwrap();
        assert_eq!(summary.stage, "intent");
        assert!(summary.skill_name.is_none());
    }

    #[test]
    fn summarize_draft_infers_manifest_stage() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("plugin.toml"),
            r#"[plugin]
id = "test-skill"
name = "Test Skill"
"#,
        )
        .unwrap();

        let summary = summarize_draft(tmp.path(), "000000000000").unwrap();
        assert_eq!(summary.stage, "manifest");
        assert_eq!(summary.skill_name.as_deref(), Some("Test Skill"));
    }

    #[test]
    fn summarize_draft_infers_workflow_stage() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("plugin.toml"), "[plugin]\nname = \"X\"").unwrap();
        std::fs::write(tmp.path().join("workflow.toml"), "[[steps]]").unwrap();
        let summary = summarize_draft(tmp.path(), "000000000000").unwrap();
        assert_eq!(summary.stage, "workflow");
    }

    #[test]
    fn summarize_draft_infers_prompts_stage() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("plugin.toml"), "[plugin]\nname = \"X\"").unwrap();
        std::fs::write(tmp.path().join("workflow.toml"), "[[steps]]").unwrap();
        std::fs::create_dir_all(tmp.path().join("prompts")).unwrap();
        std::fs::write(tmp.path().join("prompts/step0.md"), "# hi").unwrap();

        let summary = summarize_draft(tmp.path(), "000000000000").unwrap();
        assert_eq!(summary.stage, "prompts");
    }

    #[test]
    fn summarize_draft_ignores_malformed_plugin_toml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("plugin.toml"), "not valid toml {{{").unwrap();

        // Should not panic, stage should still be computed
        let summary = summarize_draft(tmp.path(), "000000000000").unwrap();
        assert_eq!(summary.stage, "manifest");
        assert!(summary.skill_name.is_none());
    }
}
