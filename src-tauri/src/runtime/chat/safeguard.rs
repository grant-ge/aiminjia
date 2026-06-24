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

static BARE_FILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?ix)
        ([A-Z0-9_.-]+(?:[/\\][A-Z0-9_.-]+)*\.
            (?:md|markdown|txt|json|jsonl|csv|tsv|ya?ml|toml|py|js|ts|tsx|jsx|rs|go|java|kt|sh|bash|ps1|sql|html?|css|xml|pdf|docx|xlsx|pptx)
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
        "save",
        "generate",
        "output",
        "export",
        "produce",
        "status report",
        "report to",
        "file in",
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
                    | '.'
                    | ';'
                    | '；'
                    | ':'
                    | '：'
            )
        })
        .replace('\\', "/");
    if candidate.is_empty()
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

    if [
        "from ",
        "using ",
        "based on ",
        "read ",
        "load ",
        "source ",
        "input ",
        "从",
        "读取",
        "基于",
        "来源",
    ]
    .iter()
    .any(|marker| immediate_before.contains(marker))
    {
        return false;
    }

    before.contains("create")
        || before.contains("write")
        || before.contains("save")
        || before.contains("generate")
        || before.contains("output")
        || before.contains("export")
        || before.contains("produce")
        || before.contains("report to")
        || before.contains("as a")
        || before.contains("to ")
        || before.contains("创建")
        || before.contains("新建")
        || before.contains("写入")
        || before.contains("保存")
        || before.contains("生成")
        || before.contains("输出")
        || before.contains("导出")
        || after.contains("file")
        || after.contains("workspace root")
        || after.contains("工作区根")
        || after.contains("根目录")
        || after.contains("文件")
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
    targets.into_iter().collect()
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
        "<system-reminder>\n原始请求包含明确命名的文件产物，但当前工作区中以下文件仍不存在、为空，或明显还是 Pending/TODO/To be filled 占位骨架：\n{list}\n\n下一步必须优先调用 Write、Edit 或等价文件写入工具，在用户指定路径创建或更新这些文件。可以写入部分诊断、已知事实、待验证项或阻塞原因，但不能停留在“待填写/继续分析”的空骨架。不要继续扩大阅读、搜索、TaskCreate 或总结，直到这些命名文件至少包含可交付内容。\n</system-reminder>"
    )
}

pub fn delivery_guard_blocking_prompt(missing_targets: &[String]) -> String {
    let list = missing_targets
        .iter()
        .map(|target| format!("- `{target}`"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<system-reminder>\n上一轮已经要求先交付命名文件，但你本轮仍准备调用非写入工具。系统已跳过这批工具调用，因为以下目标文件仍不存在、为空，或仍是 Pending/TODO/To be filled 占位骨架：\n{list}\n\n下一轮只调用 Write 或 Edit 更新这些路径的可交付内容；不要调用 Read、Glob、Bash、Skill、TaskCreate 或其它探索工具。可以写入部分诊断、已知事实、阻塞原因和手动动作，但不能只写“待补充/继续分析”。\n</system-reminder>"
    )
}

pub fn maybe_delivery_guard_prompt(
    targets: &[String],
    workspace_root: &Path,
    guard_count: usize,
    iteration: usize,
) -> Option<String> {
    if targets.is_empty() || guard_count >= 3 {
        return None;
    }
    let min_iteration = match guard_count {
        0 => 0,
        1 => 3,
        _ => 6,
    };
    if iteration < min_iteration {
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
    fn does_not_treat_source_file_as_output_target() {
        let targets = extract_requested_file_targets(
            "Read `input.csv` and create `report.md` with the findings.",
        );
        assert_eq!(targets, vec!["report.md".to_string()]);
    }
}
