//! Skill draft schema validation (M2 T3).
//!
//! Reads a draft's `plugin.toml` + `workflow.toml` + referenced prompt files
//! and produces a structured [`ValidationReport`]. The report is designed to
//! be fed back to the LLM for automatic repair:
//!
//! - Errors block commit and must be fixed.
//! - Warnings don't block (e.g. missing optional fields, very short prompts).
//! - `summary` is a short natural-language sentence for UI / LLM context.
//!
//! Schema rules live in one place — `RULES` / the `*_SPEC` constants below —
//! so the LLM can quote them back to itself in few-shot prompts.
//!
//! ## Why we don't use `#[serde(deny_unknown_fields)]`
//!
//! Phase 4 (v0.8.0) plans to add a `[playwright]` section for browser-based
//! skills. Using `deny_unknown_fields` now would reject future-spec drafts.
//! Instead we allow extra fields at parse time and validate the fields we
//! care about by name.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::draft_dir;

// ---------------------------------------------------------------------------
// Rule constants (exposed so tests & prompts can reference them)
// ---------------------------------------------------------------------------

const PLUGIN_ID_MIN: usize = 3;
const PLUGIN_ID_MAX: usize = 40;
const KEYWORD_MIN: usize = 3;
const KEYWORD_MAX: usize = 20;
const KEYWORD_CHAR_MIN: usize = 2;
const KEYWORD_CHAR_MAX: usize = 30;
const WORKFLOW_STEP_MIN: usize = 2;
const WORKFLOW_STEP_MAX: usize = 10;
const ICON_CHAR_MIN: usize = 1;
const ICON_CHAR_MAX: usize = 4; // covers emoji + variation selector + ZWJ pairs
const PROMPT_MIN_BYTES: usize = 50; // below this → warning

pub(crate) const VALID_CATEGORIES: &[&str] =
    &["general", "hr", "finance", "legal", "sales", "ops"];

pub(crate) const VALID_MODEL_PREFERENCES: &[&str] =
    &["fast", "balanced", "deep_reasoning"];

pub(crate) const VALID_ADVANCE_ON: &[&str] = &["any", "confirm", "auto"];

/// Built-in plugin IDs that user-created skills must not clash with.
/// Sourced from `code/src-tauri/plugins/*/plugin.toml` as of Phase 10.
///
/// TODO(skill-smith M4): replace with a live query against `SkillRegistry`
/// via managed state to stay fresh as new built-ins land.
pub(crate) const RESERVED_PLUGIN_IDS: &[&str] = &[
    // HR data analysis (5)
    "recruitment-funnel",
    "talent-9box",
    "engagement-survey",
    "comp-analysis-v2",
    "salary-benchmarking",
    // HR consulting (4)
    "perf-system-design",
    "org-diagnosis",
    "labor-compliance",
    "pa-maturity",
    // Legal (2)
    "contract-review",
    "policy-compliance-audit",
    // Finance (3)
    "finance-analysis",
    "budget-analysis",
    "biz-proposal",
    // Sales (2)
    "sales-analysis",
    "customer-segmentation",
    // Ops (2)
    "ops-analysis",
    "user-behavior",
    // General tools (3)
    "biz-writing",
    "okr-coach",
    "survey-analysis",
    // Skill-smith itself (reserved; T1 adds it as built-in)
    "skill-smith",
];

// ---------------------------------------------------------------------------
// Public report types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationError>,
    pub summary: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ValidationError {
    /// The source file, e.g. `"plugin.toml"` / `"workflow.toml"` / `"prompts/step0.md"`.
    pub file: String,
    /// Dotted/bracketed path to the offending field, e.g. `"trigger.keywords[0]"`.
    pub path: String,
    /// Short rule label, e.g. `"length: 2-30"` or `"enum"`.
    pub rule: String,
    /// What we actually saw (stringified for the LLM).
    pub actual: String,
    /// Human-readable message (Chinese primary, English parenthetical).
    pub message: String,
    /// Optional hint string a downstream LLM can use to fix it.
    pub fix_hint: Option<String>,
}

// ---------------------------------------------------------------------------
// Internal TOML schemas — used only for parsing, not exposed
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct PluginManifest {
    plugin: Option<PluginSection>,
    trigger: Option<TriggerSection>,
    model: Option<ModelSection>,
    display: Option<DisplaySection>,
    // `prompts`, `defaults`, `playwright` etc. are intentionally ignored —
    // we only validate semantics, not presence of optional sections.
}

#[derive(Deserialize)]
struct PluginSection {
    id: Option<String>,
    name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct TriggerSection {
    keywords: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ModelSection {
    preference: Option<String>,
}

#[derive(Deserialize)]
struct DisplaySection {
    category: Option<String>,
    icon: Option<String>,
}

#[derive(Deserialize, Default)]
struct WorkflowManifest {
    #[serde(default)]
    steps: Vec<WorkflowStep>,
}

#[derive(Deserialize, Debug)]
struct WorkflowStep {
    id: Option<String>,
    prompt: Option<String>,
    advance_on: Option<String>,
}

// ---------------------------------------------------------------------------
// Command entry point
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn validate_skill_draft(
    app: AppHandle,
    draft_id: String,
) -> Result<ValidationReport, String> {
    let dir = draft_dir(&app, &draft_id)?;
    if !dir.is_dir() {
        return Err(format!("Draft '{}' not found", draft_id));
    }
    Ok(validate_draft_dir(&dir))
}

/// Sync validation against an on-disk draft directory.
/// Split out from the Tauri command for unit testing without an AppHandle.
pub(crate) fn validate_draft_dir(dir: &Path) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    validate_plugin_manifest(dir, &mut errors);
    let workflow = validate_workflow_manifest(dir, &mut errors);

    // Cross-file check: every workflow step's prompt file must exist and be non-empty.
    if let Some(wf) = workflow {
        validate_prompt_files(dir, &wf, &mut errors, &mut warnings);
    }

    let valid = errors.is_empty();
    let summary = build_summary(valid, &errors, &warnings);

    ValidationReport {
        valid,
        errors,
        warnings,
        summary,
    }
}

// ---------------------------------------------------------------------------
// plugin.toml validation
// ---------------------------------------------------------------------------

fn validate_plugin_manifest(dir: &Path, errors: &mut Vec<ValidationError>) {
    let path = dir.join("plugin.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        errors.push(ValidationError {
            file: "plugin.toml".into(),
            path: "(file)".into(),
            rule: "exists".into(),
            actual: "missing".into(),
            message: "缺少 plugin.toml (plugin.toml missing)".into(),
            fix_hint: Some("在 draft 根目录创建 plugin.toml".into()),
        });
        return;
    };

    let manifest: PluginManifest = match toml::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            errors.push(ValidationError {
                file: "plugin.toml".into(),
                path: "(root)".into(),
                rule: "toml-syntax".into(),
                actual: e.to_string(),
                message: format!("plugin.toml TOML 语法错误 ({})", e),
                fix_hint: Some("检查 TOML 语法：数组用 [] 包裹，字符串用双引号".into()),
            });
            return;
        }
    };

    validate_plugin_section(&manifest.plugin, errors);
    validate_trigger_section(&manifest.trigger, errors);
    validate_model_section(&manifest.model, errors);
    validate_display_section(&manifest.display, errors);
}

fn validate_plugin_section(section: &Option<PluginSection>, errors: &mut Vec<ValidationError>) {
    let Some(plugin) = section else {
        errors.push(err(
            "plugin.toml",
            "plugin",
            "exists",
            "missing",
            "缺少 [plugin] 段 ([plugin] section missing)",
            Some("补充 [plugin] 段，至少包含 id / name / type"),
        ));
        return;
    };

    // plugin.id
    match &plugin.id {
        None => errors.push(err(
            "plugin.toml",
            "plugin.id",
            "exists",
            "missing",
            "缺少 plugin.id",
            Some("plugin.id 必须是小写字母开头，3-40 字符，只含字母/数字/连字符"),
        )),
        Some(id) => {
            if !is_valid_plugin_id(id) {
                errors.push(err(
                    "plugin.toml",
                    "plugin.id",
                    "format",
                    id,
                    "plugin.id 格式非法（需小写字母开头，3-40 字符，只含字母/数字/连字符）",
                    Some("示例：my-analysis-skill"),
                ));
            } else if RESERVED_PLUGIN_IDS.contains(&id.as_str()) {
                errors.push(err(
                    "plugin.toml",
                    "plugin.id",
                    "reserved",
                    id,
                    &format!("plugin.id '{}' 与内置技能冲突 (reserved by built-in skill)", id),
                    Some("换一个不同的 id，建议加前缀如 'custom-' 或公司名缩写"),
                ));
            }
        }
    }

    // plugin.name
    match &plugin.name {
        None => errors.push(err(
            "plugin.toml",
            "plugin.name",
            "exists",
            "missing",
            "缺少 plugin.name",
            Some("plugin.name 是用户看到的技能名称，2-40 字符"),
        )),
        Some(n) => {
            let char_count = n.chars().count();
            if !(2..=40).contains(&char_count) {
                errors.push(err(
                    "plugin.toml",
                    "plugin.name",
                    "length: 2-40",
                    n,
                    &format!("plugin.name 长度非法（{}字符，需 2-40）", char_count),
                    None,
                ));
            }
        }
    }

    // plugin.type (must be exactly "skill")
    match &plugin.kind {
        None => errors.push(err(
            "plugin.toml",
            "plugin.type",
            "exists",
            "missing",
            "缺少 plugin.type",
            Some("plugin.type 必须为 \"skill\""),
        )),
        Some(k) if k != "skill" => errors.push(err(
            "plugin.toml",
            "plugin.type",
            "enum: skill",
            k,
            &format!("plugin.type 必须为 \"skill\"（当前 \"{}\"）", k),
            Some("改为 type = \"skill\""),
        )),
        _ => {}
    }
}

fn validate_trigger_section(section: &Option<TriggerSection>, errors: &mut Vec<ValidationError>) {
    let Some(trigger) = section else {
        errors.push(err(
            "plugin.toml",
            "trigger",
            "exists",
            "missing",
            "缺少 [trigger] 段",
            Some("补充 [trigger] 段，至少包含 keywords 数组"),
        ));
        return;
    };

    let Some(keywords) = &trigger.keywords else {
        errors.push(err(
            "plugin.toml",
            "trigger.keywords",
            "exists",
            "missing",
            "缺少 trigger.keywords",
            Some(&format!(
                "提供 {}-{} 个触发关键词（中英文均可）",
                KEYWORD_MIN, KEYWORD_MAX
            )),
        ));
        return;
    };

    if !(KEYWORD_MIN..=KEYWORD_MAX).contains(&keywords.len()) {
        errors.push(err(
            "plugin.toml",
            "trigger.keywords",
            &format!("count: {}-{}", KEYWORD_MIN, KEYWORD_MAX),
            &format!("{} items", keywords.len()),
            &format!(
                "trigger.keywords 数量非法（{} 个，需 {}-{}）",
                keywords.len(),
                KEYWORD_MIN,
                KEYWORD_MAX
            ),
            Some("每个关键词是用户可能说出的触发短语，中英文混合更好"),
        ));
    }

    for (i, kw) in keywords.iter().enumerate() {
        let char_count = kw.chars().count();
        if !(KEYWORD_CHAR_MIN..=KEYWORD_CHAR_MAX).contains(&char_count) {
            errors.push(err(
                "plugin.toml",
                &format!("trigger.keywords[{}]", i),
                &format!("length: {}-{}", KEYWORD_CHAR_MIN, KEYWORD_CHAR_MAX),
                kw,
                &format!(
                    "trigger.keywords[{}] 长度非法（{}字符，需 {}-{}）",
                    i, char_count, KEYWORD_CHAR_MIN, KEYWORD_CHAR_MAX
                ),
                None,
            ));
        }
    }
}

fn validate_model_section(section: &Option<ModelSection>, errors: &mut Vec<ValidationError>) {
    // [model] section is optional — only validate if present
    let Some(model) = section else { return };
    if let Some(pref) = &model.preference {
        if !VALID_MODEL_PREFERENCES.contains(&pref.as_str()) {
            errors.push(err(
                "plugin.toml",
                "model.preference",
                "enum",
                pref,
                &format!(
                    "model.preference 非法 '{}'，需为 {:?}",
                    pref, VALID_MODEL_PREFERENCES
                ),
                Some("常见选择：deep_reasoning（需要推理）/ fast（快速响应）/ balanced（平衡）"),
            ));
        }
    }
}

fn validate_display_section(section: &Option<DisplaySection>, errors: &mut Vec<ValidationError>) {
    let Some(display) = section else {
        errors.push(err(
            "plugin.toml",
            "display",
            "exists",
            "missing",
            "缺少 [display] 段",
            Some("补充 [display] 段，至少包含 category 和 icon"),
        ));
        return;
    };

    // category
    match &display.category {
        None => errors.push(err(
            "plugin.toml",
            "display.category",
            "exists",
            "missing",
            "缺少 display.category",
            Some(&format!("枚举值之一：{:?}", VALID_CATEGORIES)),
        )),
        Some(c) if !VALID_CATEGORIES.contains(&c.as_str()) => errors.push(err(
            "plugin.toml",
            "display.category",
            "enum",
            c,
            &format!("display.category 非法 '{}'，需为 {:?}", c, VALID_CATEGORIES),
            None,
        )),
        _ => {}
    }

    // icon (single emoji, 1-4 unicode scalar values)
    match &display.icon {
        None => errors.push(err(
            "plugin.toml",
            "display.icon",
            "exists",
            "missing",
            "缺少 display.icon",
            Some("放一个 emoji，例如 \"💰\" \"📊\" \"🛠️\""),
        )),
        Some(i) => {
            let count = i.chars().count();
            if !(ICON_CHAR_MIN..=ICON_CHAR_MAX).contains(&count) {
                errors.push(err(
                    "plugin.toml",
                    "display.icon",
                    &format!("length: {}-{} chars", ICON_CHAR_MIN, ICON_CHAR_MAX),
                    i,
                    &format!(
                        "display.icon 应为单个 emoji（{} Unicode chars，限 {}-{}）",
                        count, ICON_CHAR_MIN, ICON_CHAR_MAX
                    ),
                    Some("示例：\"💰\" \"📊\" \"🛠️\""),
                ));
            }
            // First char should not be ASCII — catches icon = "A" etc.
            if let Some(first) = i.chars().next() {
                if first.is_ascii() && count == 1 {
                    errors.push(err(
                        "plugin.toml",
                        "display.icon",
                        "non-ascii",
                        i,
                        "display.icon 不应为 ASCII 字符",
                        Some("放一个真正的 emoji"),
                    ));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// workflow.toml validation
// ---------------------------------------------------------------------------

fn validate_workflow_manifest(
    dir: &Path,
    errors: &mut Vec<ValidationError>,
) -> Option<WorkflowManifest> {
    let path = dir.join("workflow.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        errors.push(err(
            "workflow.toml",
            "(file)",
            "exists",
            "missing",
            "缺少 workflow.toml (workflow.toml missing)",
            Some("在 draft 根目录创建 workflow.toml"),
        ));
        return None;
    };

    let manifest: WorkflowManifest = match toml::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            errors.push(err(
                "workflow.toml",
                "(root)",
                "toml-syntax",
                &e.to_string(),
                &format!("workflow.toml TOML 语法错误 ({})", e),
                Some("[[steps]] 是数组 table 语法，每个 step 前面都要有 [[steps]]"),
            ));
            return None;
        }
    };

    // Step count
    if !(WORKFLOW_STEP_MIN..=WORKFLOW_STEP_MAX).contains(&manifest.steps.len()) {
        errors.push(err(
            "workflow.toml",
            "steps",
            &format!("count: {}-{}", WORKFLOW_STEP_MIN, WORKFLOW_STEP_MAX),
            &format!("{} steps", manifest.steps.len()),
            &format!(
                "workflow steps 数量非法（{} 步，需 {}-{}）",
                manifest.steps.len(),
                WORKFLOW_STEP_MIN,
                WORKFLOW_STEP_MAX
            ),
            None,
        ));
    }

    // Per-step validation + id uniqueness
    let mut seen_ids: Vec<String> = Vec::new();
    for (i, step) in manifest.steps.iter().enumerate() {
        match &step.id {
            None => errors.push(err(
                "workflow.toml",
                &format!("steps[{}].id", i),
                "exists",
                "missing",
                &format!("steps[{}] 缺少 id", i),
                Some("每个 step 必须有唯一 id（如 \"step0\"）"),
            )),
            Some(id) => {
                if id.is_empty() {
                    errors.push(err(
                        "workflow.toml",
                        &format!("steps[{}].id", i),
                        "non-empty",
                        id,
                        &format!("steps[{}].id 不能为空", i),
                        None,
                    ));
                } else if seen_ids.iter().any(|prev| prev == id) {
                    errors.push(err(
                        "workflow.toml",
                        &format!("steps[{}].id", i),
                        "unique",
                        id,
                        &format!("steps[{}].id '{}' 与前面重复", i, id),
                        Some("每个 step 的 id 必须唯一"),
                    ));
                } else {
                    seen_ids.push(id.clone());
                }
            }
        }

        if step.prompt.is_none() {
            errors.push(err(
                "workflow.toml",
                &format!("steps[{}].prompt", i),
                "exists",
                "missing",
                &format!("steps[{}] 缺少 prompt 路径", i),
                Some("prompt 指向 prompts/ 下的 .md 文件，如 \"prompts/step0.md\""),
            ));
        }

        if let Some(ad) = &step.advance_on {
            if !VALID_ADVANCE_ON.contains(&ad.as_str()) {
                errors.push(err(
                    "workflow.toml",
                    &format!("steps[{}].advance_on", i),
                    "enum",
                    ad,
                    &format!(
                        "steps[{}].advance_on 非法 '{}'，需为 {:?}",
                        i, ad, VALID_ADVANCE_ON
                    ),
                    None,
                ));
            }
        }
    }

    Some(manifest)
}

// ---------------------------------------------------------------------------
// Prompt file cross-reference
// ---------------------------------------------------------------------------

fn validate_prompt_files(
    dir: &Path,
    workflow: &WorkflowManifest,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<ValidationError>,
) {
    for (i, step) in workflow.steps.iter().enumerate() {
        let Some(rel_prompt) = &step.prompt else { continue };

        // Defensive: prompt path must not contain traversal
        if rel_prompt.contains("..") || rel_prompt.starts_with('/') {
            errors.push(err(
                "workflow.toml",
                &format!("steps[{}].prompt", i),
                "path-safety",
                rel_prompt,
                &format!(
                    "steps[{}].prompt '{}' 含非法路径字符（'..' 或绝对路径）",
                    i, rel_prompt
                ),
                None,
            ));
            continue;
        }

        let prompt_path: PathBuf = dir.join(rel_prompt);
        let file_label = rel_prompt.clone();

        if !prompt_path.is_file() {
            errors.push(err(
                &file_label,
                "(file)",
                "exists",
                "missing",
                &format!(
                    "workflow 引用的 prompt 文件不存在：{} (referenced by steps[{}])",
                    rel_prompt, i
                ),
                Some("在 prompts/ 下创建该文件"),
            ));
            continue;
        }

        match std::fs::read_to_string(&prompt_path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    errors.push(err(
                        &file_label,
                        "(content)",
                        "non-empty",
                        "empty",
                        &format!("prompt 文件 {} 为空", rel_prompt),
                        Some("填入该步骤的 system prompt"),
                    ));
                } else if content.len() < PROMPT_MIN_BYTES {
                    warnings.push(err(
                        &file_label,
                        "(content)",
                        &format!("length: >={}B", PROMPT_MIN_BYTES),
                        &format!("{} bytes", content.len()),
                        &format!(
                            "prompt 文件 {} 很短（{}字节），可能不足以指导 LLM",
                            rel_prompt,
                            content.len()
                        ),
                        Some("扩充到至少一段完整的角色 + 任务描述"),
                    ));
                }
            }
            Err(e) => {
                errors.push(err(
                    &file_label,
                    "(io)",
                    "readable",
                    &e.to_string(),
                    &format!("读取 prompt 文件失败：{}", e),
                    None,
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_valid_plugin_id(id: &str) -> bool {
    if !(PLUGIN_ID_MIN..=PLUGIN_ID_MAX).contains(&id.len()) {
        return false;
    }
    let mut chars = id.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_lowercase() {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn err(
    file: &str,
    path: &str,
    rule: &str,
    actual: &str,
    message: &str,
    fix_hint: Option<&str>,
) -> ValidationError {
    ValidationError {
        file: file.to_string(),
        path: path.to_string(),
        rule: rule.to_string(),
        actual: actual.to_string(),
        message: message.to_string(),
        fix_hint: fix_hint.map(|s| s.to_string()),
    }
}

fn build_summary(
    valid: bool,
    errors: &[ValidationError],
    warnings: &[ValidationError],
) -> String {
    if valid && warnings.is_empty() {
        return "校验通过 (validation passed)".to_string();
    }
    if valid {
        return format!(
            "校验通过，但有 {} 条警告 (validation passed with {} warning(s))",
            warnings.len(),
            warnings.len()
        );
    }
    let first = errors
        .first()
        .map(|e| format!("{}: {}", e.path, e.message))
        .unwrap_or_default();
    format!(
        "校验失败 ({} error(s), {} warning(s))：{}",
        errors.len(),
        warnings.len(),
        first
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fully-valid draft directory in a tempdir and return the root.
    fn make_valid_draft() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(
            dir.join("plugin.toml"),
            r#"
[plugin]
id = "my-custom-skill"
name = "我的技能"
type = "skill"
priority = 20

[trigger]
keywords = ["我的技能", "自定义分析", "custom skill"]
requires_files = false

[model]
preference = "deep_reasoning"

[display]
category = "hr"
icon = "🛠️"
short_description = "test"
"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("workflow.toml"),
            r#"
[[steps]]
id = "step0"
name = "采集"
prompt = "prompts/step0.md"
advance_on = "any"

[[steps]]
id = "step1"
name = "分析"
prompt = "prompts/step1.md"
advance_on = "confirm"
"#,
        )
        .unwrap();

        std::fs::create_dir_all(dir.join("prompts")).unwrap();
        std::fs::write(
            dir.join("prompts/step0.md"),
            "# Step 0\n\n你是一个数据分析助手，负责采集用户输入的原始数据并验证格式。",
        )
        .unwrap();
        std::fs::write(
            dir.join("prompts/step1.md"),
            "# Step 1\n\n基于 step 0 的数据，执行分析并生成结构化结论报告。",
        )
        .unwrap();

        tmp
    }

    // ---- 12 required test cases from M2 T3 spec ----

    #[test]
    fn case01_uppercase_plugin_id_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace("my-custom-skill", "My-Custom-Skill");
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "plugin.id" && e.rule == "format"));
    }

    #[test]
    fn case02_reserved_plugin_id_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace("my-custom-skill", "comp-analysis-v2");
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "plugin.id" && e.rule == "reserved"));
    }

    #[test]
    fn case03_empty_keywords_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace(
                r#"keywords = ["我的技能", "自定义分析", "custom skill"]"#,
                "keywords = []",
            );
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "trigger.keywords" && e.rule.starts_with("count")));
    }

    #[test]
    fn case04_too_few_keywords_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace(
                r#"keywords = ["我的技能", "自定义分析", "custom skill"]"#,
                r#"keywords = ["单一"]"#,
            );
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "trigger.keywords" && e.rule.starts_with("count")));
    }

    #[test]
    fn case05_invalid_category_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace(r#"category = "hr""#, r#"category = "custom""#);
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "display.category" && e.rule == "enum"));
    }

    #[test]
    fn case06_ascii_icon_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace(r#"icon = "🛠️""#, r#"icon = "X""#);
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.path == "display.icon"));
    }

    #[test]
    fn case07_invalid_model_preference_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let bad = std::fs::read_to_string(dir.join("plugin.toml"))
            .unwrap()
            .replace(r#"preference = "deep_reasoning""#, r#"preference = "creative""#);
        std::fs::write(dir.join("plugin.toml"), bad).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "model.preference" && e.rule == "enum"));
    }

    #[test]
    fn case08_only_one_step_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        std::fs::write(
            dir.join("workflow.toml"),
            r#"
[[steps]]
id = "step0"
name = "仅一步"
prompt = "prompts/step0.md"
"#,
        )
        .unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "steps" && e.rule.starts_with("count")));
    }

    #[test]
    fn case09_duplicate_step_ids_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        std::fs::write(
            dir.join("workflow.toml"),
            r#"
[[steps]]
id = "step0"
name = "A"
prompt = "prompts/step0.md"

[[steps]]
id = "step0"
name = "B"
prompt = "prompts/step1.md"
"#,
        )
        .unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.path == "steps[1].id" && e.rule == "unique"));
    }

    #[test]
    fn case10_missing_prompt_file_is_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        std::fs::remove_file(dir.join("prompts/step1.md")).unwrap();

        let report = validate_draft_dir(dir);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.file == "prompts/step1.md" && e.rule == "exists"));
    }

    #[test]
    fn case11_empty_prompt_is_warning() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        // Write a < 50 byte prompt (warning, not error)
        std::fs::write(dir.join("prompts/step0.md"), "太短了").unwrap();

        let report = validate_draft_dir(dir);
        // Still valid (warnings don't block)
        assert!(report.valid, "short prompt should be warning, got: {:?}", report);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.file == "prompts/step0.md" && w.rule.starts_with("length")));
    }

    #[test]
    fn case12_all_valid_returns_passed() {
        let tmp = make_valid_draft();
        let report = validate_draft_dir(tmp.path());
        assert!(
            report.valid && report.errors.is_empty(),
            "expected valid, got errors: {:?}",
            report.errors
        );
        assert!(report.warnings.is_empty());
        assert!(report.summary.contains("校验通过"));
    }

    // ---- extra robustness tests ----

    #[test]
    fn playwright_section_is_tolerated_not_rejected() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        let extended = std::fs::read_to_string(dir.join("plugin.toml")).unwrap()
            + "\n[playwright]\ndomain_whitelist = [\"example.com\"]\n";
        std::fs::write(dir.join("plugin.toml"), extended).unwrap();

        let report = validate_draft_dir(dir);
        assert!(report.valid, "unknown sections should be allowed for Phase 4 forward-compat");
    }

    #[test]
    fn missing_plugin_toml_reports_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("workflow.toml"), "# empty").unwrap();

        let report = validate_draft_dir(tmp.path());
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.file == "plugin.toml" && e.rule == "exists"));
    }

    #[test]
    fn malformed_plugin_toml_reports_syntax_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("plugin.toml"), "{{ not valid toml").unwrap();
        std::fs::write(tmp.path().join("workflow.toml"), "").unwrap();

        let report = validate_draft_dir(tmp.path());
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.file == "plugin.toml" && e.rule == "toml-syntax"));
    }

    #[test]
    fn is_valid_plugin_id_rules() {
        assert!(is_valid_plugin_id("my-skill"));
        assert!(is_valid_plugin_id("abc"));
        assert!(is_valid_plugin_id("x1-y2-z3"));
        // Bad
        assert!(!is_valid_plugin_id("My-Skill")); // uppercase
        assert!(!is_valid_plugin_id("1skill")); // starts with digit
        assert!(!is_valid_plugin_id("-skill")); // starts with dash
        assert!(!is_valid_plugin_id("ab")); // too short
        assert!(!is_valid_plugin_id(&"x".repeat(41))); // too long
        assert!(!is_valid_plugin_id("my_skill")); // underscore
        assert!(!is_valid_plugin_id("my.skill")); // dot
    }

    #[test]
    fn summary_is_helpful_on_success() {
        let tmp = make_valid_draft();
        let report = validate_draft_dir(tmp.path());
        assert!(report.summary.contains("校验通过"));
    }

    #[test]
    fn summary_highlights_first_error() {
        let tmp = make_valid_draft();
        let dir = tmp.path();
        std::fs::remove_file(dir.join("plugin.toml")).unwrap();
        let report = validate_draft_dir(dir);
        assert!(report.summary.contains("plugin.toml") || report.summary.contains("校验失败"));
    }
}
