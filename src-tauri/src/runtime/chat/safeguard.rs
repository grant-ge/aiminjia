use std::collections::BTreeSet;
use std::path::{Component, Path};

use once_cell::sync::Lazy;
use regex::Regex;

/// 迭代保护策略的决策结果
#[derive(Debug)]
pub enum SafeguardAction {
    /// 无需干预，正常继续
    Continue,
    /// 注入提示消息后继续（要求 LLM 输出文字）
    InjectPromptAndContinue(String),
}

/// 检查当前迭代是否需要触发保护机制。
///
/// 对应统一主循环中的文本兜底保护。
///
/// 参数：
/// - `iteration`                  当前迭代索引（0-based，对应原代码中的 `iteration`）
/// - `max_iterations`             本步骤迭代上限
/// - `full_content`               到目前为止 LLM 已输出的文字内容（空表示仅调工具，无文字输出）
pub fn check_iteration(
    iteration: usize,
    max_iterations: usize,
    full_content: &str,
) -> SafeguardAction {
    if full_content.is_empty() && iteration >= max_iterations.saturating_sub(3) {
        return SafeguardAction::InjectPromptAndContinue(
            "已接近处理上限。请停止扩展探索，优先交付用户要求的最终产物；如果用户要求文件、报告、脚本、配置或数据输出，请立即创建或更新对应文件，并验证文件存在、非空、路径正确。只有无法交付时，才用文字说明阻塞原因和已完成的部分。".to_string(),
        );
    }

    SafeguardAction::Continue
}

static CODE_SPAN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"`([^`\r\n]{1,180})`").expect("valid code span regex"));
const DELIVERY_GUARD_TOOL_GRACE_ITERATIONS: usize = 1;

static BARE_FILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?ix)
        ([A-Z0-9_.-]+(?:[/\\][A-Z0-9_.-]+)*\.
            (?:md|markdown|txt|json|jsonl|csv|tsv|ya?ml|toml|py|js|ts|tsx|jsx|rs|go|java|kt|sh|bash|ps1|sql|html?|css|xml|pdf|docx|xlsx|pptx|png|jpe?g|webp|gif|svg|dot|env|template)
        )
        "#,
    )
    .expect("valid bare file regex")
});

fn looks_like_creation_request(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "create",
        "write",
        "update",
        "modify",
        "save",
        "generate",
        "output",
        "export",
        "produce",
        "status report",
        "report to",
        "file in",
        "file called",
        "file named",
        "workspace root",
        "创建",
        "新建",
        "写入",
        "保存",
        "生成",
        "输出",
        "导出",
        "报告",
        "文件",
        "产物",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(crate) fn normalize_workspace_file_candidate(raw: &str) -> Option<String> {
    let candidate = raw
        .trim()
        .trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"'
                    | '\''
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '，'
                    | '。'
                    | ','
                    | ';'
                    | '；'
                    | ':'
                    | '：'
            )
        })
        .replace('\\', "/");
    if candidate.is_empty()
        || candidate.ends_with('/')
        || candidate.starts_with('/')
        || candidate.contains("://")
        || candidate.contains('\0')
        || candidate.len() > 180
    {
        return None;
    }
    let path = Path::new(&candidate);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    let file_name = path.file_name()?.to_string_lossy();
    if !file_name.contains('.') {
        return None;
    }
    Some(candidate)
}

fn char_boundary_before(text: &str, mut idx: usize) -> usize {
    idx = idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn char_boundary_after(text: &str, mut idx: usize) -> usize {
    idx = idx.min(text.len());
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

fn output_context_for_match(request: &str, start: usize, end: usize) -> bool {
    let before_start = char_boundary_before(request, start.saturating_sub(96));
    let after_end = char_boundary_after(request, end.saturating_add(96));
    let before = request
        .get(before_start..start)
        .unwrap_or_default()
        .to_lowercase();
    let after = request
        .get(end..after_end)
        .unwrap_or_default()
        .to_lowercase();
    let immediate_before_start = char_boundary_before(request, start.saturating_sub(24));
    let immediate_before = request
        .get(immediate_before_start..start)
        .unwrap_or_default()
        .to_lowercase();

    let source_markers = [
        "from ",
        "using ",
        "based on ",
        "read ",
        "load ",
        "source ",
        "input ",
        "credential in ",
        "hardcoded credential in ",
        "从",
        "读取",
        "基于",
        "来源",
    ];

    if source_markers
        .iter()
        .any(|marker| immediate_before.contains(marker))
    {
        return false;
    }

    let output_markers = [
        "create",
        "write",
        "update",
        "modify",
        "append",
        "save",
        "generate",
        "output",
        "export",
        "produce",
        "deliver",
        "implement",
        "implementation",
        "target file",
        "target files",
        "required file",
        "required files",
        "output a",
        "output an",
        "visualization",
        "validated json",
        "report to",
        "file called",
        "file named",
        "called ",
        "named ",
        "at ",
        "as a",
        "to ",
        "创建",
        "新建",
        "写入",
        "保存",
        "生成",
        "输出",
        "导出",
    ];
    let last_output_marker = output_markers
        .iter()
        .filter_map(|marker| before.rfind(marker))
        .max();
    let last_source_marker = source_markers
        .iter()
        .filter_map(|marker| before.rfind(marker))
        .max();

    if let Some(last_source_marker) = last_source_marker {
        if last_output_marker.map_or(true, |last_output_marker| {
            last_source_marker > last_output_marker
        }) {
            return false;
        }
    }

    last_output_marker.is_some()
        || after.contains("workspace root")
        || after.contains("工作区根")
        || after.contains("根目录")
}

/// Extract explicit file targets from a user request when the request appears to
/// require creating/updating files. The result is intentionally conservative and
/// only includes relative workspace paths.
pub fn extract_requested_file_targets(request: &str) -> Vec<String> {
    if !looks_like_creation_request(request) {
        return Vec::new();
    }

    let mut targets = BTreeSet::new();
    for caps in CODE_SPAN_RE.captures_iter(request) {
        let Some(m) = caps.get(1) else {
            continue;
        };
        if !output_context_for_match(request, m.start(), m.end()) {
            continue;
        }
        if let Some(value) = normalize_workspace_file_candidate(m.as_str()) {
            targets.insert(value);
        }
    }
    for caps in BARE_FILE_RE.captures_iter(request) {
        let Some(m) = caps.get(1) else {
            continue;
        };
        if !output_context_for_match(request, m.start(), m.end()) {
            continue;
        }
        if let Some(value) = normalize_workspace_file_candidate(m.as_str()) {
            targets.insert(value);
        }
    }
    drop_shadowed_bare_targets(targets)
}

fn drop_shadowed_bare_targets(targets: BTreeSet<String>) -> Vec<String> {
    let shadowed_basenames: BTreeSet<String> = targets
        .iter()
        .filter(|target| target.contains('/'))
        .filter_map(|target| Path::new(target).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .collect();

    targets
        .into_iter()
        .filter(|target| {
            if target.contains('/') {
                return true;
            }
            let Some(file_name) = Path::new(target).file_name() else {
                return true;
            };
            !shadowed_basenames.contains(file_name.to_string_lossy().as_ref())
        })
        .collect()
}

pub fn missing_requested_file_targets(targets: &[String], workspace_root: &Path) -> Vec<String> {
    targets
        .iter()
        .filter(|target| {
            let path = workspace_root.join(target);
            match std::fs::metadata(&path) {
                Ok(meta) => !meta.is_file() || meta.len() == 0,
                Err(_) => true,
            }
        })
        .cloned()
        .collect()
}

fn file_content_looks_placeholder(content: &str) -> bool {
    let lower = content.to_lowercase();
    [
        "to be filled",
        "full analysis pending",
        "diagnostic in progress",
        "⏳ pending",
        "pending | need to",
        "need to read",
        "need to verify",
        "need to run",
        "pdf text extraction in progress",
        "awaiting pdf text extraction",
        "will be populated once",
        "this file is being written iteratively",
        "extraction in progress",
        "not yet verified",
        "not yet completed",
        "todo:",
        "tbd",
        "待填写",
        "待补充",
        "待确认",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub fn unready_requested_file_targets(targets: &[String], workspace_root: &Path) -> Vec<String> {
    targets
        .iter()
        .filter(|target| {
            let path = workspace_root.join(target);
            let Ok(meta) = std::fs::metadata(&path) else {
                return true;
            };
            if !meta.is_file() || meta.len() == 0 {
                return true;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => file_content_looks_placeholder(&content),
                Err(_) => false,
            }
        })
        .cloned()
        .collect()
}

pub fn delivery_guard_prompt(missing_targets: &[String]) -> String {
    let list = missing_targets
        .iter()
        .map(|target| format!("- `{target}`"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<system-reminder>\n原始请求包含明确命名的文件产物，但当前工作区中以下文件仍不存在、为空，或明显还是 Pending/TODO/To be filled 占位骨架：\n{list}\n\n下一步必须优先调用 Write、Edit 或等价文件写入/生成工具，在用户指定路径创建或更新这些文件。若目标是 PNG/PDF/XLSX 等二进制或图片产物，可以调用 Bash、PowerShell 或 ShellTask 运行明确写入目标路径的生成命令，并随后验证文件存在、非空。若当前输入和规则足以计算/解析/转换，直接生成真实结果；不要为 JSON/CSV/配置/API payload 写 null、status: computing、TODO 或 placeholder 占位。确实阻塞时，可以写结构化阻塞原因，但不能停留在“待填写/继续分析”的空骨架。不要继续扩大阅读、搜索、TaskCreate 或总结，直到这些命名文件至少包含可交付内容。\n</system-reminder>"
    )
}

pub fn delivery_guard_failed_tool_prompt(missing_targets: &[String]) -> String {
    let list = missing_targets
        .iter()
        .map(|target| format!("- `{target}`"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<system-reminder>\n上一轮工具调用本来要生成命名文件，但工具执行失败后以下目标文件仍不存在、为空，或仍是 Pending/TODO/To be filled 占位骨架：\n{list}\n\n下一步必须恢复交付，不要直接总结失败。若失败原因是缺少 matplotlib/Pillow/pdf 工具、pip 不可用、命令不存在或环境受限，请立刻改用已安装工具、Python 标准库、SVG/CSV/文本降级实现，或把明确阻塞原因写入目标文件；PNG/PDF/XLSX 等二进制产物仍要优先尝试可实际写入目标路径的兜底生成命令，并验证文件存在、非空。结构化结果文件不要用 null/status/TODO 伪装成进展；除非真的阻塞，否则必须写真实字段值。不要重复同一个失败命令，不要只说“继续处理/还要生成”。\n</system-reminder>"
    )
}

pub fn delivery_guard_text_only_prompt(missing_targets: &[String]) -> String {
    let list = missing_targets
        .iter()
        .map(|target| format!("- `{target}`"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<system-reminder>\n上一轮已经要求先交付命名文件，但你只输出了文字，没有调用写入或生成工具；本轮不能以口头计划、总结或道歉结束。以下目标文件仍不存在、为空，或仍是 Pending/TODO/To be filled 占位骨架：\n{list}\n\n下一轮必须调用 Write、Edit，或调用 Bash/PowerShell/ShellTask 运行会直接写入目标路径的生成命令，把这些路径更新为可检查的内容。若输入和规则已经足够，直接写真实结果；不要把 null、status: computing、TODO 或 placeholder 写进结构化产物当作临时完成。确实阻塞时，可以基于已知事实和阻塞原因写可用版本；不要再只说“我将创建/我会生成/继续分析”。\n</system-reminder>"
    )
}

pub fn maybe_delivery_guard_prompt(
    targets: &[String],
    workspace_root: &Path,
    guard_count: usize,
    iteration: usize,
) -> Option<String> {
    maybe_delivery_guard_prompt_inner(targets, workspace_root, guard_count, iteration, false)
}

pub fn maybe_delivery_guard_prompt_after_tool_round(
    targets: &[String],
    workspace_root: &Path,
    guard_count: usize,
    iteration: usize,
) -> Option<String> {
    maybe_delivery_guard_prompt_inner(targets, workspace_root, guard_count, iteration, true)
}

fn maybe_delivery_guard_prompt_inner(
    targets: &[String],
    workspace_root: &Path,
    guard_count: usize,
    iteration: usize,
    apply_tool_grace: bool,
) -> Option<String> {
    if targets.is_empty() || guard_count >= 3 {
        return None;
    }
    if apply_tool_grace && guard_count == 0 && iteration < DELIVERY_GUARD_TOOL_GRACE_ITERATIONS {
        return None;
    }
    let missing = unready_requested_file_targets(targets, workspace_root);
    if missing.is_empty() {
        None
    } else {
        Some(delivery_guard_prompt(&missing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_no_action_when_far_from_limit() {
        let action = check_iteration(0, 10, "some content");
        assert!(matches!(action, SafeguardAction::Continue));
    }

    #[test]
    fn daily_injects_when_near_limit_and_no_content() {
        // iteration 7 >= max_iterations(10) - 3 = 7, full_content empty
        let action = check_iteration(7, 10, "");
        match action {
            SafeguardAction::InjectPromptAndContinue(message) => {
                assert!(message.contains("优先交付用户要求的最终产物"));
                assert!(message.contains("验证文件存在、非空、路径正确"));
            }
            SafeguardAction::Continue => panic!("expected safeguard prompt near iteration limit"),
        }
    }

    #[test]
    fn daily_no_inject_when_near_limit_but_has_content() {
        let action = check_iteration(7, 10, "some text");
        assert!(matches!(action, SafeguardAction::Continue));
    }

    #[test]
    fn extracts_named_file_targets_for_creation_requests() {
        let targets = extract_requested_file_targets(
            "Create `SKILL.md` in the workspace root and write a status report to `diagnosis-report.md`.",
        );
        assert_eq!(
            targets,
            vec!["SKILL.md".to_string(), "diagnosis-report.md".to_string()]
        );
    }

    #[test]
    fn extracts_named_html_target_from_media_artifact_request() {
        let targets = extract_requested_file_targets(
            "Please first view the image, then generate `output/output.html` reproducing the score using inline SVG. Save the result to `output/output.html`.",
        );
        assert_eq!(targets, vec!["output/output.html".to_string()]);
    }

    #[test]
    fn extracts_target_files_from_should_be_phrase() {
        let targets = extract_requested_file_targets(
            "The target files should be `svpwm_output/svpwm.c` and `svpwm_output/svpwm.h`.",
        );
        assert_eq!(
            targets,
            vec![
                "svpwm_output/svpwm.c".to_string(),
                "svpwm_output/svpwm.h".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_parser_outputs_from_implementation_prompt() {
        let targets = extract_requested_file_targets(
            "You should implement a function in your parser `solution.py` and output a validated JSON graph `dialogue.json` and visualization `dialogue.dot`.",
        );
        assert_eq!(
            targets,
            vec![
                "dialogue.dot".to_string(),
                "dialogue.json".to_string(),
                "solution.py".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_file_called_report_target_without_source_file() {
        let targets = extract_requested_file_targets(
            "I have a transcript file `transcript.md` from NASA. Please read the transcript and identify controversial statements in a file called `controversy_analysis.md`.",
        );
        assert_eq!(targets, vec!["controversy_analysis.md".to_string()]);
    }

    #[test]
    fn extracts_hidden_secret_management_targets() {
        let targets = extract_requested_file_targets(
            "Create a `.secrets/` directory. Create `.secrets/.env.template` from `.env.example`. Create `.secrets/README.md` using `old_notes.txt` and `security_config.json`. Update `.gitignore`. Update `SECURITY.md`. Flag the hardcoded credential in `config.json` in the README.",
        );
        assert_eq!(
            targets,
            vec![
                ".gitignore".to_string(),
                ".secrets/.env.template".to_string(),
                ".secrets/README.md".to_string(),
                "SECURITY.md".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_csv_and_png_chart_targets() {
        let targets = extract_requested_file_targets(
            "Save a CSV at `output/thinking_relative_impact.csv` and generate a vertical bar chart at `output/relative_gain_bar.png` visualizing the sorted relative percentage increases.",
        );
        assert_eq!(
            targets,
            vec![
                "output/relative_gain_bar.png".to_string(),
                "output/thinking_relative_impact.csv".to_string(),
            ]
        );
    }

    #[test]
    fn prefers_directory_qualified_output_over_section_heading_basename() {
        let request = r#"
Save the results into `output/` with the following files:
   - `output/scheduled.ics`
   - `output/unscheduled.json`
   - `output/decision_log.md`

### `scheduled.ics`
Must be a valid iCalendar file.

### `unscheduled.json`
A JSON array.

### `decision_log.md`
Use exactly the following structure.
"#;
        let targets = extract_requested_file_targets(request);
        assert_eq!(
            targets,
            vec![
                "output/decision_log.md".to_string(),
                "output/scheduled.ics".to_string(),
                "output/unscheduled.json".to_string(),
            ]
        );
    }

    #[test]
    fn ignores_file_mentions_without_creation_intent() {
        let targets = extract_requested_file_targets("What does `package.json` do?");
        assert!(targets.is_empty());
    }

    #[test]
    fn reports_missing_targets_as_prompt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("present.md"), "ok").unwrap();
        let targets = vec!["present.md".to_string(), "missing.md".to_string()];
        let prompt = maybe_delivery_guard_prompt(&targets, dir.path(), 0, 0)
            .expect("missing target should prompt");
        assert!(prompt.contains("missing.md"));
        assert!(!prompt.contains("present.md"));
        assert!(prompt.contains("必须优先调用 Write"));
    }

    #[test]
    fn delays_first_tool_round_prompt_during_initial_exploration() {
        let dir = tempfile::tempdir().unwrap();
        let targets = vec!["diagnosis-report.md".to_string()];

        let prompt = maybe_delivery_guard_prompt_after_tool_round(&targets, dir.path(), 0, 0);

        assert!(prompt.is_none());
    }

    #[test]
    fn prompts_after_one_followup_tool_round() {
        let dir = tempfile::tempdir().unwrap();
        let targets = vec!["diagnosis-report.md".to_string()];

        let prompt = maybe_delivery_guard_prompt_after_tool_round(&targets, dir.path(), 0, 1)
            .expect("missing target should prompt after exploration grace");

        assert!(prompt.contains("diagnosis-report.md"));
    }

    #[test]
    fn repeats_prompt_when_guarded_target_is_still_missing() {
        let dir = tempfile::tempdir().unwrap();
        let targets = vec!["diagnosis-report.md".to_string()];
        let prompt = maybe_delivery_guard_prompt(&targets, dir.path(), 1, 1)
            .expect("missing target should keep prompting after first guard");
        assert!(prompt.contains("diagnosis-report.md"));
    }

    #[test]
    fn tool_round_prompt_repeats_after_first_guard() {
        let dir = tempfile::tempdir().unwrap();
        let targets = vec!["diagnosis-report.md".to_string()];

        let prompt = maybe_delivery_guard_prompt_after_tool_round(&targets, dir.path(), 1, 1)
            .expect("missing target should keep prompting once guard has started");

        assert!(prompt.contains("diagnosis-report.md"));
    }

    #[test]
    fn text_only_prompt_rejects_plan_without_file_write() {
        let prompt = delivery_guard_text_only_prompt(&["output/output.html".to_string()]);
        assert!(prompt.contains("output/output.html"));
        assert!(prompt.contains("只输出了文字"));
        assert!(prompt.contains("必须调用 Write"));
        assert!(prompt.contains("我将创建"));
    }

    #[test]
    fn treats_placeholder_files_as_unready() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("diagnosis-report.md"),
            "## Status\nDiagnostic in Progress\n\n> To be filled in after analysis.",
        )
        .unwrap();
        let targets = vec!["diagnosis-report.md".to_string()];
        let prompt = maybe_delivery_guard_prompt(&targets, dir.path(), 0, 0)
            .expect("placeholder target should prompt");
        assert!(prompt.contains("diagnosis-report.md"));
        assert!(prompt.contains("占位骨架"));
    }

    #[test]
    fn treats_pdf_extraction_stub_as_unready() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("contract_analysis.md"),
            "# Contract Analysis\n\n> Status: Partial — PDF text extraction in progress.\n\n## Key Dates\n\nAwaiting PDF text extraction — section will be populated once the document text is read.",
        )
        .unwrap();
        let targets = vec!["contract_analysis.md".to_string()];
        let prompt = maybe_delivery_guard_prompt(&targets, dir.path(), 0, 0)
            .expect("PDF extraction stub should prompt");
        assert!(prompt.contains("contract_analysis.md"));
        assert!(prompt.contains("占位骨架"));
    }

    #[test]
    fn does_not_treat_source_file_as_output_target() {
        let targets = extract_requested_file_targets(
            "Read `input.csv` and create `report.md` with the findings.",
        );
        assert_eq!(targets, vec!["report.md".to_string()]);
    }
}
