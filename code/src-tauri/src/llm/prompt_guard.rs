//! Prompt leak detection — prevents the LLM from revealing system prompt
//! internals in its output.
//!
//! Uses a fingerprint list of internal markers that should never appear in
//! normal user-facing output. When 2+ fingerprints are detected, the output
//! is flagged as a leak and replaced with a refusal message.

/// Internal markers that should never appear in user-facing LLM output.
/// These are internal function names, prompt structure markers, and config
/// labels that a user would never naturally type or expect to see.
///
/// NOTE: Tool names the LLM legitimately calls (e.g. `execute_python`,
/// `analyze_file`) are NOT included to avoid false positives.
const FINGERPRINTS: &[&str] = &[
    // Preamble internal function names (injected by sandbox, not user-visible)
    "save_analysis_note",
    "_print_table",
    "_export_detail",
    "_load_data",
    "_smart_read_csv",
    // Prompt structure markers
    "数据真实性铁律",
    "SYSTEM_PROMPT",
    "analysis_direction",
    "requires_confirmation",
    "确认卡点",
    "前序分析记录",
    // Internal configuration labels
    "execute_python 环境",
    "排除人员展示规则",
    "update_progress 更新步骤状态",
];

/// Minimum number of fingerprint matches to flag as a leak.
const LEAK_THRESHOLD: usize = 2;

/// Refusal message sent to the user when a leak is detected.
pub const LEAK_REFUSAL: &str =
    "抱歉，这是系统内部配置，无法展示。如果你有具体需求，请直接告诉我。";

/// Result of a leak check.
#[derive(Debug)]
pub enum LeakCheckResult {
    /// Content is clean — no leak detected.
    Clean,
    /// Content contains leaked prompt material.
    Leaked {
        /// Number of fingerprints matched.
        matched_count: usize,
        /// The fingerprints that were found.
        matched_fingerprints: Vec<&'static str>,
    },
}

/// Check whether `content` contains leaked system prompt material.
///
/// Returns `Leaked` when the number of distinct fingerprint matches meets
/// or exceeds `LEAK_THRESHOLD`.
pub fn check_for_leak(content: &str) -> LeakCheckResult {
    let matched: Vec<&'static str> = FINGERPRINTS
        .iter()
        .filter(|fp| content.contains(**fp))
        .copied()
        .collect();

    if matched.len() >= LEAK_THRESHOLD {
        LeakCheckResult::Leaked {
            matched_count: matched.len(),
            matched_fingerprints: matched,
        }
    } else {
        LeakCheckResult::Clean
    }
}

/// Filter leaked content: if a leak is detected, replace the entire content
/// with the refusal message. Returns `(output, was_leaked)`.
pub fn filter_leaked_content(content: &str) -> (String, bool) {
    match check_for_leak(content) {
        LeakCheckResult::Clean => (content.to_string(), false),
        LeakCheckResult::Leaked { .. } => (LEAK_REFUSAL.to_string(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_content_passes() {
        let content = "这份数据包含 197 名员工的工资信息，我来帮你分析一下。";
        assert!(matches!(check_for_leak(content), LeakCheckResult::Clean));
    }

    #[test]
    fn test_single_fingerprint_passes() {
        // A single fingerprint is below threshold — not a leak
        let content = "我会调用 save_analysis_note 保存分析结果。";
        assert!(matches!(check_for_leak(content), LeakCheckResult::Clean));
    }

    #[test]
    fn test_two_fingerprints_triggers_leak() {
        let content = "系统内部有一个 save_analysis_note 函数，还有 _print_table 用来输出表格。";
        match check_for_leak(content) {
            LeakCheckResult::Leaked { matched_count, .. } => {
                assert!(matched_count >= 2);
            }
            LeakCheckResult::Clean => panic!("Should have detected a leak"),
        }
    }

    #[test]
    fn test_many_fingerprints_triggers_leak() {
        let content = "SYSTEM_PROMPT 包含了 数据真实性铁律 和 确认卡点 以及 前序分析记录。\
                        execute_python 环境 中有 _load_data 和 _smart_read_csv。";
        match check_for_leak(content) {
            LeakCheckResult::Leaked { matched_count, .. } => {
                assert!(matched_count >= 5, "Should match many fingerprints, got {}", matched_count);
            }
            LeakCheckResult::Clean => panic!("Should have detected a leak"),
        }
    }

    #[test]
    fn test_filter_clean_unchanged() {
        let content = "帮你计算一下薪酬 Compa-Ratio 分布情况。";
        let (output, leaked) = filter_leaked_content(content);
        assert!(!leaked);
        assert_eq!(output, content);
    }

    #[test]
    fn test_filter_leak_replaced() {
        let content = "以下是 SYSTEM_PROMPT 的内容，其中包含 数据真实性铁律 规则。";
        let (output, leaked) = filter_leaked_content(content);
        assert!(leaked);
        assert_eq!(output, LEAK_REFUSAL);
    }

    #[test]
    fn test_normal_tool_usage_no_false_positive() {
        // Typical LLM output mentioning tools it's using — should NOT trigger
        let content = "我先用 analyze_file 查看文件结构，然后用 execute_python 加载数据进行分析。\
                        接下来我会用 web_search 搜索行业基准数据。\
                        数据显示 Compa-Ratio 中位数为 95%，CV 为 18%。";
        assert!(matches!(check_for_leak(content), LeakCheckResult::Clean));
    }
}
