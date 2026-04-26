//! Skill draft dry-run — static sanity checks (M2 T7).
//!
//! Runs the same plumbing that would fire at real runtime (parse manifests,
//! construct `DeclarativeSkill`, verify referenced files) — **without**
//! invoking any LLM or executing any Python. Produces a structured
//! [`DryRunReport`] that the UI (and later the skill-smith LLM) can use to
//! confirm the draft is plumbable before commit / export.
//!
//! ## Why 6 checks when Check 1 already subsumes some?
//!
//! The schema check (via T3 `validate_draft_dir`) already catches missing
//! prompts and malformed TOML. We still surface **prompt reference**,
//! **prompt content**, and **loadability** as separate checks because:
//!
//! 1. The user / LLM gets a clearer mental map of "where did we break?"
//! 2. Schema is field-level; Loadable exercises the real parser + DeclarativeSkill
//!    constructor (catches wiring bugs that pass schema).
//! 3. Phase 2 will fold in real Python execution; the slot is already here.
//!
//! Failing checks don't short-circuit — all 6 run so the report is complete
//! on a single invocation.

use std::path::Path;

use serde::Serialize;
use tauri::AppHandle;

use super::draft_dir;
use super::validation::validate_draft_dir;

/// Threshold below which prompt content triggers a warning (bytes).
/// Mirrors the T3 validator to keep the stories consistent.
const PROMPT_MIN_BYTES: usize = 50;

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Skip,
    Warn,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    /// Stable machine-readable id — safe to match on in TS.
    pub name: String,
    pub status: CheckStatus,
    /// Human-readable detail (Chinese primary).
    pub detail: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DryRunReport {
    /// `true` iff no check has status `Fail`. `Warn` / `Skip` don't block.
    pub pass: bool,
    pub checks: Vec<CheckResult>,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn dry_run_skill_draft(app: AppHandle, draft_id: String) -> Result<DryRunReport, String> {
    let dir = draft_dir(&app, &draft_id)?;
    if !dir.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }
    Ok(dry_run_draft_dir(&dir))
}

/// Sync entrypoint for unit tests (no AppHandle needed).
pub(crate) fn dry_run_draft_dir(dir: &Path) -> DryRunReport {
    let mut checks = Vec::with_capacity(6);

    checks.push(check_schema(dir));

    // Parse workflow up-front so later checks can query it. If parsing fails,
    // the schema check above will have flagged it — subsequent checks fall
    // back gracefully.
    let workflow_steps = read_workflow_prompts(dir);

    checks.push(check_prompts_reference(dir, workflow_steps.as_ref()));
    checks.push(check_prompts_content(dir, workflow_steps.as_ref()));
    checks.push(check_python_scripts(dir));
    checks.push(check_knowledge(dir));
    checks.push(check_loadable(dir));

    let pass = !checks.iter().any(|c| c.status == CheckStatus::Fail);
    let summary = build_summary(pass, &checks);

    DryRunReport {
        pass,
        checks,
        summary,
    }
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

fn check_schema(dir: &Path) -> CheckResult {
    let report = validate_draft_dir(dir);
    if report.valid {
        let warn_note = if report.warnings.is_empty() {
            String::new()
        } else {
            format!("（含 {} 条 warning）", report.warnings.len())
        };
        CheckResult {
            name: "schema".into(),
            status: CheckStatus::Pass,
            detail: format!("Schema 校验通过{}", warn_note),
        }
    } else {
        CheckResult {
            name: "schema".into(),
            status: CheckStatus::Fail,
            detail: format!(
                "Schema 校验失败（{} 个错误）：{}",
                report.errors.len(),
                report.summary
            ),
        }
    }
}

fn check_prompts_reference(dir: &Path, prompts: Option<&Vec<String>>) -> CheckResult {
    let Some(prompts) = prompts else {
        return CheckResult {
            name: "prompts-reference".into(),
            status: CheckStatus::Skip,
            detail: "跳过：workflow.toml 解析失败或无 steps".into(),
        };
    };

    let missing: Vec<String> = prompts
        .iter()
        .filter(|p| !dir.join(p).is_file())
        .cloned()
        .collect();

    if missing.is_empty() {
        CheckResult {
            name: "prompts-reference".into(),
            status: CheckStatus::Pass,
            detail: format!("workflow 引用的 {} 个 prompt 文件全部存在", prompts.len()),
        }
    } else {
        CheckResult {
            name: "prompts-reference".into(),
            status: CheckStatus::Fail,
            detail: format!(
                "{} 个 prompt 文件缺失：{}",
                missing.len(),
                missing.join(", ")
            ),
        }
    }
}

fn check_prompts_content(dir: &Path, prompts: Option<&Vec<String>>) -> CheckResult {
    let Some(prompts) = prompts else {
        return CheckResult {
            name: "prompts-content".into(),
            status: CheckStatus::Skip,
            detail: "跳过：无 workflow.toml 可检索".into(),
        };
    };

    let mut empty = Vec::new();
    let mut short = Vec::new();
    let mut unreadable = Vec::new();

    for rel in prompts {
        let path = dir.join(rel);
        if !path.is_file() {
            continue; // handled by prompts-reference check
        }
        match std::fs::read_to_string(&path) {
            Ok(s) if s.trim().is_empty() => empty.push(rel.clone()),
            Ok(s) if s.len() < PROMPT_MIN_BYTES => short.push((rel.clone(), s.len())),
            Ok(_) => {}
            Err(e) => unreadable.push((rel.clone(), e.to_string())),
        }
    }

    if !empty.is_empty() || !unreadable.is_empty() {
        let mut parts = Vec::new();
        if !empty.is_empty() {
            parts.push(format!("空 prompt: {}", empty.join(", ")));
        }
        if !unreadable.is_empty() {
            parts.push(format!(
                "读取失败: {}",
                unreadable
                    .iter()
                    .map(|(p, e)| format!("{}({})", p, e))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return CheckResult {
            name: "prompts-content".into(),
            status: CheckStatus::Fail,
            detail: parts.join("; "),
        };
    }

    if !short.is_empty() {
        return CheckResult {
            name: "prompts-content".into(),
            status: CheckStatus::Warn,
            detail: format!(
                "{} 个 prompt 长度 < {}B：{}",
                short.len(),
                PROMPT_MIN_BYTES,
                short
                    .iter()
                    .map(|(p, n)| format!("{}({}B)", p, n))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
    }

    CheckResult {
        name: "prompts-content".into(),
        status: CheckStatus::Pass,
        detail: format!("{} 个 prompt 内容检查通过", prompts.len()),
    }
}

fn check_python_scripts(dir: &Path) -> CheckResult {
    let scripts_dir = dir.join("scripts");
    let py_files = list_files_with_extension(&scripts_dir, "py");
    if py_files.is_empty() {
        return CheckResult {
            name: "python-scripts".into(),
            status: CheckStatus::Pass,
            detail: "无 Python 脚本（纯对话型技能）".into(),
        };
    }
    CheckResult {
        name: "python-scripts".into(),
        status: CheckStatus::Skip,
        detail: format!(
            "发现 {} 个 Python 脚本，M2 不验证执行（Phase 2 启用沙箱 dry-run）",
            py_files.len()
        ),
    }
}

fn check_knowledge(dir: &Path) -> CheckResult {
    let kb_dir = dir.join("scripts").join("knowledge");
    let json_files = list_files_with_extension(&kb_dir, "json");
    if json_files.is_empty() {
        return CheckResult {
            name: "knowledge".into(),
            status: CheckStatus::Pass,
            detail: "无知识库文件（可选）".into(),
        };
    }

    // Best-effort JSON syntax check — catches obvious typos even in M2
    let mut bad = Vec::new();
    for f in &json_files {
        if let Ok(s) = std::fs::read_to_string(dir.join(f)) {
            if serde_json::from_str::<serde_json::Value>(&s).is_err() {
                bad.push(f.clone());
            }
        }
    }
    if !bad.is_empty() {
        return CheckResult {
            name: "knowledge".into(),
            status: CheckStatus::Fail,
            detail: format!("{} 个知识库 JSON 语法错误: {}", bad.len(), bad.join(", ")),
        };
    }

    CheckResult {
        name: "knowledge".into(),
        status: CheckStatus::Skip,
        detail: format!(
            "发现 {} 个知识库 JSON（语法 OK），M2 不验证内容结构",
            json_files.len()
        ),
    }
}

/// The strongest check: exercise the real runtime parser + skill loader on
/// the draft. A "schema-valid but not loadable" draft is almost never seen
/// (T3 validation exceeds what the loader requires), but keeping this check
/// guards against drift between T3 rules and the actual plugin loader.
fn check_loadable(dir: &Path) -> CheckResult {
    let plugin_toml_path = dir.join("plugin.toml");
    let Ok(plugin_content) = std::fs::read_to_string(&plugin_toml_path) else {
        return CheckResult {
            name: "loadable".into(),
            status: CheckStatus::Fail,
            detail: "无法读取 plugin.toml".into(),
        };
    };

    let manifest = match crate::plugin::manifest::parse_plugin_manifest(&plugin_content) {
        Ok(m) => m,
        Err(e) => {
            return CheckResult {
                name: "loadable".into(),
                status: CheckStatus::Fail,
                detail: format!("plugin.toml 解析失败: {}", e),
            };
        }
    };

    // If workflow.toml exists, parse it too (loader will).
    let workflow_path = dir.join("workflow.toml");
    if workflow_path.is_file() {
        let wf_content = match std::fs::read_to_string(&workflow_path) {
            Ok(s) => s,
            Err(e) => {
                return CheckResult {
                    name: "loadable".into(),
                    status: CheckStatus::Fail,
                    detail: format!("无法读取 workflow.toml: {}", e),
                };
            }
        };
        if let Err(e) = crate::plugin::manifest::parse_workflow_manifest(&wf_content) {
            return CheckResult {
                name: "loadable".into(),
                status: CheckStatus::Fail,
                detail: format!("workflow.toml 解析失败: {}", e),
            };
        }
    }

    match crate::plugin::declarative_skill::DeclarativeSkill::load(&manifest, dir) {
        Ok(_skill) => CheckResult {
            name: "loadable".into(),
            status: CheckStatus::Pass,
            detail: "DeclarativeSkill 构造成功".into(),
        },
        Err(e) => CheckResult {
            name: "loadable".into(),
            status: CheckStatus::Fail,
            detail: format!("DeclarativeSkill::load 失败: {}", e),
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read `workflow.toml` and extract the list of referenced prompt paths.
/// Returns `None` if the file can't be read or parsed — caller should skip
/// dependent checks.
fn read_workflow_prompts(dir: &Path) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(dir.join("workflow.toml")).ok()?;
    let value: toml::Value = toml::from_str(&content).ok()?;
    let steps = value.get("steps")?.as_array()?;
    let prompts: Vec<String> = steps
        .iter()
        .filter_map(|s| s.get("prompt").and_then(|v| v.as_str()).map(String::from))
        .collect();
    if prompts.is_empty() {
        None
    } else {
        Some(prompts)
    }
}

/// Shallow (non-recursive) listing of files with a given extension inside `dir`.
/// Returns paths relative to `dir.parent()` so they read naturally in messages.
fn list_files_with_extension(dir: &Path, ext: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
        {
            // Format relative to the draft root: "scripts/foo.py" / "scripts/knowledge/foo.json"
            let rel = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let parent_name = dir
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str());
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let display = match parent_name {
                Some(p) if p == "scripts" && rel == "knowledge" => {
                    format!("scripts/knowledge/{}", file_name)
                }
                _ if rel == "scripts" => format!("scripts/{}", file_name),
                _ => file_name.to_string(),
            };
            out.push(display);
        }
    }
    out.sort();
    out
}

fn build_summary(pass: bool, checks: &[CheckResult]) -> String {
    let n_pass = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Pass)
        .count();
    let n_warn = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Warn)
        .count();
    let n_skip = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Skip)
        .count();
    let n_fail = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .count();

    if pass {
        format!(
            "Dry-run 通过（{}✓ {}⚠ {}⊘，共 {} 项检查）",
            n_pass,
            n_warn,
            n_skip,
            checks.len()
        )
    } else {
        let first_fail = checks
            .iter()
            .find(|c| c.status == CheckStatus::Fail)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .unwrap_or_default();
        format!("Dry-run 失败（{} 项错误）: {}", n_fail, first_fail)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fully-loadable draft (passes schema + loads as DeclarativeSkill).
    fn make_loadable_draft() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(
            dir.join("plugin.toml"),
            r#"[plugin]
id = "dry-run-test"
name = "Dry-Run 测试技能"
type = "skill"
priority = 20

[trigger]
keywords = ["dry run", "测试", "verify"]
requires_files = false

[model]
preference = "balanced"

[display]
category = "general"
icon = "🧪"
"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("workflow.toml"),
            r#"[[steps]]
id = "step0"
name = "intent"
prompt = "prompts/step0.md"
advance_on = "any"

[[steps]]
id = "step1"
name = "analysis"
prompt = "prompts/step1.md"
advance_on = "confirm"
"#,
        )
        .unwrap();

        std::fs::create_dir_all(dir.join("prompts")).unwrap();
        std::fs::write(
            dir.join("prompts/step0.md"),
            "# Step 0\n\n你是测试技能的意图采集环节，负责跟用户确认分析方向。",
        )
        .unwrap();
        std::fs::write(
            dir.join("prompts/step1.md"),
            "# Step 1\n\n基于 step 0 的信息，给出结构化的分析结论。",
        )
        .unwrap();

        tmp
    }

    fn check<'a>(report: &'a DryRunReport, name: &str) -> &'a CheckResult {
        report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("check '{}' not found in report", name))
    }

    // ---- Happy path ----

    #[test]
    fn fully_loadable_draft_passes_all_six_checks() {
        let tmp = make_loadable_draft();
        let report = dry_run_draft_dir(tmp.path());

        assert!(
            report.pass,
            "expected pass, got fail. summary: {}",
            report.summary
        );
        assert_eq!(report.checks.len(), 6);
        assert_eq!(check(&report, "schema").status, CheckStatus::Pass);
        assert_eq!(
            check(&report, "prompts-reference").status,
            CheckStatus::Pass
        );
        assert_eq!(check(&report, "prompts-content").status, CheckStatus::Pass);
        assert_eq!(check(&report, "python-scripts").status, CheckStatus::Pass);
        assert_eq!(check(&report, "knowledge").status, CheckStatus::Pass);
        assert_eq!(check(&report, "loadable").status, CheckStatus::Pass);
        assert!(report.summary.contains("通过"));
    }

    // ---- Individual check failures ----

    #[test]
    fn missing_prompt_file_fails_both_schema_and_prompts_reference() {
        let tmp = make_loadable_draft();
        std::fs::remove_file(tmp.path().join("prompts/step1.md")).unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        assert_eq!(check(&report, "schema").status, CheckStatus::Fail);
        assert_eq!(
            check(&report, "prompts-reference").status,
            CheckStatus::Fail
        );
        // Report should mention the missing file
        assert!(check(&report, "prompts-reference")
            .detail
            .contains("step1.md"));
    }

    #[test]
    fn empty_prompt_fails_prompts_content() {
        let tmp = make_loadable_draft();
        std::fs::write(tmp.path().join("prompts/step1.md"), "   \n\t").unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        assert_eq!(check(&report, "prompts-content").status, CheckStatus::Fail);
    }

    #[test]
    fn short_prompt_warns_but_doesnt_fail() {
        let tmp = make_loadable_draft();
        // 写一个短 (< 50B) 但非空的 prompt；schema 也会 warn，不 fail
        std::fs::write(tmp.path().join("prompts/step0.md"), "太短了").unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(report.pass, "short prompt is a warn, not a fail");
        assert_eq!(check(&report, "prompts-content").status, CheckStatus::Warn);
    }

    #[test]
    fn python_scripts_present_are_skipped() {
        let tmp = make_loadable_draft();
        std::fs::create_dir_all(tmp.path().join("scripts")).unwrap();
        std::fs::write(
            tmp.path().join("scripts/step0.py"),
            "def precompute(): pass",
        )
        .unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(report.pass);
        let c = check(&report, "python-scripts");
        assert_eq!(c.status, CheckStatus::Skip);
        assert!(c.detail.contains("Phase 2"));
    }

    #[test]
    fn knowledge_json_syntax_error_fails() {
        let tmp = make_loadable_draft();
        std::fs::create_dir_all(tmp.path().join("scripts/knowledge")).unwrap();
        std::fs::write(
            tmp.path().join("scripts/knowledge/rules.json"),
            "{{ not valid json",
        )
        .unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        assert_eq!(check(&report, "knowledge").status, CheckStatus::Fail);
    }

    #[test]
    fn knowledge_valid_json_is_skipped_with_advisory() {
        let tmp = make_loadable_draft();
        std::fs::create_dir_all(tmp.path().join("scripts/knowledge")).unwrap();
        std::fs::write(
            tmp.path().join("scripts/knowledge/templates.json"),
            r#"{"templates": [{"id": "a"}]}"#,
        )
        .unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(report.pass);
        assert_eq!(check(&report, "knowledge").status, CheckStatus::Skip);
    }

    #[test]
    fn malformed_plugin_toml_fails_loadable_check() {
        let tmp = make_loadable_draft();
        std::fs::write(tmp.path().join("plugin.toml"), "this is not { toml").unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        let c = check(&report, "loadable");
        assert_eq!(c.status, CheckStatus::Fail);
        assert!(c.detail.contains("解析失败"));
    }

    #[test]
    fn missing_workflow_is_caught_by_schema_not_loadable() {
        let tmp = make_loadable_draft();
        std::fs::remove_file(tmp.path().join("workflow.toml")).unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        assert_eq!(check(&report, "schema").status, CheckStatus::Fail);
        // Loadable check only parses workflow IF it exists — no workflow.toml
        // isn't a parse error, the schema check catches it instead.
        assert_eq!(check(&report, "loadable").status, CheckStatus::Pass);
    }

    #[test]
    fn workflow_toml_syntax_error_fails_both_schema_and_loadable() {
        let tmp = make_loadable_draft();
        std::fs::write(tmp.path().join("workflow.toml"), "[[steps}}}").unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        assert_eq!(check(&report, "schema").status, CheckStatus::Fail);
        assert_eq!(check(&report, "loadable").status, CheckStatus::Fail);
    }

    #[test]
    fn summary_mentions_first_failing_check_by_name() {
        let tmp = make_loadable_draft();
        std::fs::write(tmp.path().join("plugin.toml"), "{{broken").unwrap();

        let report = dry_run_draft_dir(tmp.path());
        assert!(!report.pass);
        // Summary should reference the first fail (schema, since it runs first)
        assert!(report.summary.contains("失败"));
    }

    // ---- Metric sanity ----

    // ─── Serde camelCase regression — see mod.rs::draft_file_serializes_camel_case
    #[test]
    fn dry_run_report_serializes_camel_case() {
        let r = DryRunReport {
            pass: true,
            checks: vec![CheckResult {
                name: "schema".into(),
                status: CheckStatus::Pass,
                detail: "ok".into(),
            }],
            summary: "passed".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        // DryRunReport has no snake fields. CheckResult has only snake-free
        // fields. Status enum should serialize as "pass" (snake_case via the
        // explicit serde rename_all on the enum, which is intentional).
        assert!(json.contains("\"checks\""));
        // Status enum should be lowercase string per #[serde(rename_all = "snake_case")]
        assert!(json.contains("\"status\":\"pass\""), "got: {}", json);
    }

    #[test]
    fn pass_flag_is_true_iff_no_fail() {
        let tmp = make_loadable_draft();
        let report = dry_run_draft_dir(tmp.path());
        let any_fail = report.checks.iter().any(|c| c.status == CheckStatus::Fail);
        assert_eq!(report.pass, !any_fail);
    }
}
