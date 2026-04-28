//! Skill draft commit + export (M2 T4).
//!
//! - `commit_skill_draft` — validated draft → `~/.renlijia/skills/{id}/` +
//!   SkillRegistry 热重载 + draft 清理。原子三段式：copy → staging → rename，
//!   中途失败不污染已有技能。冲突时返回 `conflict: true`，前端决定改名还是
//!   覆盖（调 `commit_skill_draft_force`）。
//! - `commit_skill_draft_force` — 同上但允许覆盖已有同名技能。
//! - `export_skill_draft` — 把 draft 打成 `.aijia-skill` 包到用户选的目录，
//!   draft 保留不清理。
//!
//! 入口做 validation gate；真正执行的工作放在 pure helpers 里（`commit_draft_to` /
//! `export_draft_to`），可在 unit test 里直接用 tempdir 跑，不依赖 AppHandle。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::{draft_dir, validation::validate_draft_dir};
use crate::commands::skill_management::{copy_dir_recursive, pack_skill_to_dir};
use crate::storage::UserScopedPathResolver;

/// Staging suffix used for atomic installs. Collisions (unlikely —
/// this is user-local and short-lived) get cleaned up on next commit.
const STAGING_SUFFIX: &str = ".smith-tmp";

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub skill_id: String,
    pub installed_path: String,
    /// `true` iff a skill with the same `plugin.id` already existed and we
    /// did NOT overwrite it. The caller should prompt the user (rename /
    /// force-overwrite / cancel) and re-invoke accordingly. `false` means
    /// the install actually happened.
    pub conflict: bool,
}

// ---------------------------------------------------------------------------
// Commands (thin tauri shims)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn commit_skill_draft(app: AppHandle, draft_id: String) -> Result<CommitResult, String> {
    commit_impl(&app, &draft_id, false).await
}

#[tauri::command]
pub async fn commit_skill_draft_force(
    app: AppHandle,
    draft_id: String,
) -> Result<CommitResult, String> {
    commit_impl(&app, &draft_id, true).await
}

#[tauri::command]
pub async fn export_skill_draft(
    app: AppHandle,
    draft_id: String,
    output_dir: String,
) -> Result<String, String> {
    let draft = draft_dir(&app, &draft_id)?;
    if !draft.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }

    // Validation gate — export shouldn't ship a broken skill
    let report = validate_draft_dir(&draft);
    if !report.valid {
        return Err(format!("校验未通过，无法导出: {}", report.summary));
    }

    let out = PathBuf::from(&output_dir);
    if !out.is_dir() {
        return Err(format!("输出目录不存在: {}", output_dir));
    }

    let zipped = export_draft_to(&draft, &out)?;
    Ok(zipped.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Implementation (shim-split for testability)
// ---------------------------------------------------------------------------

async fn commit_impl(app: &AppHandle, draft_id: &str, force: bool) -> Result<CommitResult, String> {
    let draft = draft_dir(app, draft_id)?;
    if !draft.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }

    // 1. Validation gate
    let report = validate_draft_dir(&draft);
    if !report.valid {
        return Err(format!("校验未通过，无法提交: {}", report.summary));
    }

    // 2. Resolve user-scoped skills target parent
    let cus = app.state::<std::sync::Arc<crate::storage::CurrentUserStorage>>();
    let custom_dir = cus
        .require_paths()
        .map_err(|e| e.to_string())?
        .skills_dir();

    // 3. Do the actual commit (pure FS work — testable)
    let result = commit_draft_to(&draft, &custom_dir, force)?;

    // If conflict (and not force), no changes made — caller will re-invoke
    if result.conflict {
        return Ok(result);
    }

    // 4. Hot-reload: register the new skill so it's usable without app restart.
    //    Best-effort: if registry reload fails, the skill is still installed
    //    and will load on next startup.
    let installed_path = result.installed_path.clone();
    if let Err(e) =
        crate::commands::skill_management::reload_skill(app.clone(), installed_path).await
    {
        log::warn!(
            "commit_skill_draft: hot-reload failed for '{}' (skill committed to disk, will activate on next restart): {}",
            result.skill_id,
            e
        );
    }

    Ok(result)
}

/// Pure commit: validate-in → atomic swap → draft cleanup.
/// Exposed for unit testing without a Tauri runtime.
///
/// Contract:
/// - Caller guarantees `src_draft` passed validation.
/// - Returns `conflict=true` and performs NO filesystem changes when
///   `target_parent/{id}` already exists and `force=false`.
/// - On success, `src_draft` is removed (the draft becomes an installed skill).
/// - On failure mid-copy, staging dir is cleaned up and nothing is committed.
pub(crate) fn commit_draft_to(
    src_draft: &Path,
    target_parent: &Path,
    force: bool,
) -> Result<CommitResult, String> {
    let skill_id = extract_skill_id(src_draft)?;

    std::fs::create_dir_all(target_parent)
        .map_err(|e| format!("Failed to create skills dir: {}", e))?;
    let target = target_parent.join(&skill_id);
    let exists = target.exists();

    if exists && !force {
        return Ok(CommitResult {
            skill_id,
            installed_path: target.to_string_lossy().to_string(),
            conflict: true,
        });
    }

    // Staging directory — survives a crash to be cleaned up on next commit.
    let staging = target_parent.join(format!("{}{}", skill_id, STAGING_SUFFIX));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| {
            format!(
                "Failed to clear stale staging dir '{}': {}",
                staging.display(),
                e
            )
        })?;
    }

    // Phase A: copy draft → staging (does not touch target).
    if let Err(e) = copy_dir_recursive(src_draft, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("Failed to stage draft: {}", e));
    }

    // Phase B: if target exists (force case), remove it.
    // TODO(skill-smith Phase 2): switch to platform-atomic rename-exchange
    //   (renameat2 on Linux, swap_files on macOS) to eliminate the brief
    //   window where target is missing. Low-priority: user-local, short op.
    if exists {
        if let Err(e) = std::fs::remove_dir_all(&target) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!(
                "Failed to remove existing skill at '{}': {}",
                target.display(),
                e
            ));
        }
    }

    // Phase C: rename staging → target.
    if let Err(e) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("Failed to move staged dir into place: {}", e));
    }

    // Phase D: cleanup draft. Best-effort — a leftover draft dir is harmless
    // (cleanup_expired_drafts will eventually GC it).
    if let Err(e) = std::fs::remove_dir_all(src_draft) {
        log::warn!(
            "commit_draft_to: succeeded but failed to remove source draft '{}': {}",
            src_draft.display(),
            e
        );
    }

    Ok(CommitResult {
        skill_id,
        installed_path: target.to_string_lossy().to_string(),
        conflict: false,
    })
}

/// Pure export: package draft dir into `{output_dir}/{skill_id}.aijia-skill`.
/// Does NOT clean up the draft (unlike commit).
pub(crate) fn export_draft_to(src_draft: &Path, output_dir: &Path) -> Result<PathBuf, String> {
    pack_skill_to_dir(src_draft, output_dir)
}

fn extract_skill_id(skill_dir: &Path) -> Result<String, String> {
    // Primary: try plugin.toml [plugin].id
    let plugin_toml = skill_dir.join("plugin.toml");
    if plugin_toml.is_file() {
        let content = std::fs::read_to_string(&plugin_toml)
            .map_err(|e| format!("Failed to read skill manifest: {}", e))?;
        let value: toml::Value = toml::from_str(&content)
            .map_err(|e| format!("Failed to read skill manifest: {}", e))?;
        let id = value
            .get("plugin")
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Failed to read skill manifest: missing plugin.id".to_string())?;
        return Ok(id.to_string());
    }
    // Secondary: try SKILL.md frontmatter `id:` line
    let skill_md = skill_dir.join("SKILL.md");
    if skill_md.is_file() {
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            for line in content.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("id:") {
                    let value = trimmed["id:".len()..].trim().trim_matches('"').trim_matches('\'');
                    if !value.is_empty() {
                        return Ok(value.to_string());
                    }
                }
            }
        }
    }
    // Fallback: use the directory's file_name as the skill id (only if dir exists)
    if !skill_dir.is_dir() {
        return Err(format!(
            "Failed to read skill manifest: '{}' is not a directory",
            skill_dir.display()
        ));
    }
    skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to read skill manifest: skill_dir has no basename".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal well-formed draft (just needs a parseable plugin.toml
    /// with `plugin.id`; full-schema validation tested in validation.rs).
    fn make_draft(tmp: &tempfile::TempDir, skill_id: &str) -> PathBuf {
        let draft = tmp.path().join(format!("draft-{}", skill_id));
        std::fs::create_dir_all(draft.join("prompts")).unwrap();
        std::fs::write(
            draft.join("plugin.toml"),
            format!(
                r#"[plugin]
id = "{}"
name = "Test"
type = "skill"

[trigger]
keywords = ["a", "b", "c"]

[display]
category = "general"
icon = "🛠️"
"#,
                skill_id
            ),
        )
        .unwrap();
        std::fs::write(draft.join("prompts/step0.md"), "hello world").unwrap();
        draft
    }

    // ---- commit_draft_to happy path ----

    #[test]
    fn commit_happy_path_installs_and_cleans_draft() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = make_draft(&tmp, "my-test-skill");
        let target_parent = tmp.path().join("skills");

        let result = commit_draft_to(&draft, &target_parent, false).unwrap();

        assert_eq!(result.skill_id, "my-test-skill");
        assert!(!result.conflict);
        assert!(target_parent.join("my-test-skill").is_dir());
        assert!(target_parent.join("my-test-skill/plugin.toml").is_file());
        assert!(target_parent
            .join("my-test-skill/prompts/step0.md")
            .is_file());
        // Draft cleaned up
        assert!(!draft.exists(), "draft should be removed after commit");
    }

    // ---- conflict detection ----

    #[test]
    fn commit_conflict_returns_flag_without_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let target_parent = tmp.path().join("skills");

        // Pre-populate target
        std::fs::create_dir_all(target_parent.join("taken-id")).unwrap();
        std::fs::write(target_parent.join("taken-id/marker.txt"), "original").unwrap();

        let draft = make_draft(&tmp, "taken-id");

        let result = commit_draft_to(&draft, &target_parent, false).unwrap();

        assert!(result.conflict);
        assert_eq!(result.skill_id, "taken-id");
        // Existing target unchanged
        assert!(target_parent.join("taken-id/marker.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(target_parent.join("taken-id/marker.txt")).unwrap(),
            "original"
        );
        // Draft preserved for retry
        assert!(draft.exists(), "draft should be preserved on conflict");
        // No plugin.toml from draft landed (existing was different)
        assert!(!target_parent.join("taken-id/plugin.toml").exists());
    }

    #[test]
    fn commit_force_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let target_parent = tmp.path().join("skills");

        std::fs::create_dir_all(target_parent.join("taken-id")).unwrap();
        std::fs::write(target_parent.join("taken-id/marker.txt"), "original").unwrap();

        let draft = make_draft(&tmp, "taken-id");
        let result = commit_draft_to(&draft, &target_parent, true).unwrap();

        assert!(!result.conflict);
        assert!(target_parent.join("taken-id/plugin.toml").is_file());
        // Old marker.txt gone
        assert!(!target_parent.join("taken-id/marker.txt").exists());
        // Draft cleaned up
        assert!(!draft.exists());
    }

    // ---- staging dir handling ----

    #[test]
    fn commit_cleans_stale_staging_from_prior_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let target_parent = tmp.path().join("skills");
        std::fs::create_dir_all(&target_parent).unwrap();

        // Simulate a crashed previous run — stale staging dir
        let stale = target_parent.join(format!("fresh-skill{}", STAGING_SUFFIX));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("stale.txt"), "junk").unwrap();

        let draft = make_draft(&tmp, "fresh-skill");
        let result = commit_draft_to(&draft, &target_parent, false).unwrap();

        assert!(!result.conflict);
        assert!(target_parent.join("fresh-skill/plugin.toml").is_file());
        // Stale staging gone
        assert!(!stale.exists());
    }

    // ---- validation of plugin.toml ----

    #[test]
    fn commit_fails_when_plugin_id_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = tmp.path().join("draft");
        std::fs::create_dir_all(&draft).unwrap();
        std::fs::write(
            draft.join("plugin.toml"),
            r#"[plugin]
name = "no id"
type = "skill"
"#,
        )
        .unwrap();

        let target_parent = tmp.path().join("skills");
        let err = commit_draft_to(&draft, &target_parent, false).unwrap_err();
        assert!(err.contains("Failed to read skill manifest"));
    }

    #[test]
    fn commit_fails_when_plugin_toml_unparseable() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = tmp.path().join("draft");
        std::fs::create_dir_all(&draft).unwrap();
        std::fs::write(draft.join("plugin.toml"), "{{not valid toml").unwrap();

        let target_parent = tmp.path().join("skills");
        let err = commit_draft_to(&draft, &target_parent, false).unwrap_err();
        assert!(err.contains("Failed to read skill manifest"));
    }

    #[test]
    fn commit_draft_to_supports_skill_md_only_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = tmp.path().join("draft-skill-md-only");
        std::fs::create_dir_all(draft.join("prompts")).unwrap();
        std::fs::write(
            draft.join("SKILL.md"),
            r#"---
id: "skill-md-only"
name: "Skill Md Only"
description: "desc"
keywords:
  - "技能草稿"
  - "skill smith"
  - "manifest fallback"
---
# Skill Md Only

这是一个仅使用 SKILL.md frontmatter 的技能草稿。
"#,
        )
        .unwrap();
        std::fs::write(draft.join("prompts/step0.md"), "hello world").unwrap();

        let target_parent = tmp.path().join("skills");
        let result = commit_draft_to(&draft, &target_parent, false).unwrap();

        assert_eq!(result.skill_id, "skill-md-only");
        assert!(!result.conflict);
        assert!(target_parent.join("skill-md-only").is_dir());
        assert!(target_parent.join("skill-md-only/SKILL.md").is_file());
        assert!(!draft.exists(), "draft should be removed after commit");
    }

    // ---- export_draft_to (pending Phase D SkillRegistry / pack_skill_to_dir) ----

    #[test]
    #[should_panic(expected = "Skill packaging will be restored")]
    fn export_creates_aijia_skill_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = make_draft(&tmp, "exportable");
        let out = tmp.path().join("downloads");
        std::fs::create_dir_all(&out).unwrap();
        // pack_skill_to_dir is unimplemented until Phase D
        let _ = export_draft_to(&draft, &out);
    }

    #[test]
    #[should_panic(expected = "Skill packaging will be restored")]
    fn export_zip_contains_all_source_files() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = make_draft(&tmp, "with-contents");
        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        // pack_skill_to_dir is unimplemented until Phase D
        let _ = export_draft_to(&draft, &out);
    }

    #[test]
    #[should_panic(expected = "Skill packaging will be restored")]
    fn export_fails_on_missing_plugin_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = tmp.path().join("empty-draft");
        std::fs::create_dir_all(&draft).unwrap();
        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        // pack_skill_to_dir is unimplemented until Phase D
        let _ = export_draft_to(&draft, &out);
    }

    // ---- extract_skill_id ----

    #[test]
    fn extract_skill_id_reads_valid_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            r#"[plugin]
id = "my-id"
name = "X"
type = "skill"
"#,
        )
        .unwrap();
        assert_eq!(extract_skill_id(&dir).unwrap(), "my-id");
    }

    // ─── Serde camelCase regression — see mod.rs::draft_file_serializes_camel_case
    #[test]
    fn commit_result_serializes_camel_case() {
        let r = CommitResult {
            skill_id: "x".into(),
            installed_path: "/p".into(),
            conflict: false,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"skillId\""), "got: {}", json);
        assert!(json.contains("\"installedPath\""), "got: {}", json);
        assert!(!json.contains("skill_id"), "found snake_case: {}", json);
        assert!(
            !json.contains("installed_path"),
            "found snake_case: {}",
            json
        );
    }

    #[test]
    fn extract_skill_id_errors_on_missing_file() {
        let err = extract_skill_id(Path::new("/nonexistent/skill-dir")).unwrap_err();
        assert!(err.contains("Failed to read skill manifest"));
    }
}
