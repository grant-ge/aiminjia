/// 迭代保护策略的决策结果
#[derive(Debug)]
pub enum SafeguardAction {
    /// 无需干预，正常继续
    Continue,
    /// 注入提示消息后继续（要求 LLM 输出文字）
    InjectPromptAndContinue(String),
    /// 注入提示消息并禁用工具后继续
    ForceNoToolsAndContinue(String),
}

/// 检查当前迭代是否需要触发保护机制。
///
/// 对应 chat_runtime_impl.rs Block 27（daily safeguard）和 Block 28（analysis 三阶段 safeguard）。
///
/// 参数：
/// - `iteration`                  当前迭代索引（0-based，对应原代码中的 `iteration`）
/// - `max_iterations`             本步骤迭代上限
/// - `full_content`               到目前为止 LLM 已输出的文字内容（空表示仅调工具，无文字输出）
/// - `is_analysis`                是否处于 analysis 模式
/// - `has_saved_note`             messages 中是否存在 role=="tool" 且 name=="save_analysis_note"
/// - `safeguard_phase1_injected`  Phase 1 提示是否已注入（调用方负责在收到对应 action 后置为 true）
/// - `force_no_tools`             force_no_tools 标志当前值（调用方负责在收到 ForceNoToolsAndContinue 后置为 true）
pub fn check_iteration(
    iteration: usize,
    max_iterations: usize,
    full_content: &str,
    is_analysis: bool,
    has_saved_note: bool,
    safeguard_phase1_injected: &mut bool,
    force_no_tools: bool,
) -> SafeguardAction {
    // Block 27: Daily mode safeguard
    // Condition: !is_analysis && full_content.is_empty() && iteration >= max_iterations - 3
    if !is_analysis && full_content.is_empty() && iteration >= max_iterations.saturating_sub(3) {
        return SafeguardAction::InjectPromptAndContinue(
            "请停止调用工具，直接用文字向用户总结目前的分析结果。".to_string(),
        );
    }

    // Block 28: Analysis mode safeguard (three phases)
    // Only applies when max_iterations >= 8 and iteration is within the last 6
    if is_analysis && max_iterations >= 8 && iteration >= max_iterations.saturating_sub(6) {
        let remaining = max_iterations.saturating_sub(iteration + 1);

        if !has_saved_note && !*safeguard_phase1_injected {
            // Phase 1 (inject once): save notes + summarize
            *safeguard_phase1_injected = true;
            return SafeguardAction::InjectPromptAndContinue(
                "⚠️ 你即将达到本步骤的迭代上限。请立即：\n\
                 1. 调用 save_analysis_note 保存当前分析结论\n\
                 2. 用文字向用户总结本步骤的分析结果\n\
                 不要再调用 execute_python，直接保存和总结。"
                    .to_string(),
            );
        } else if full_content.is_empty() && remaining <= 3 && !force_no_tools {
            // Phase 2 (inject once + arm hard cutoff): no text output approaching limit
            // Use remaining <= 3 (not 4) to give LLM at least 1 extra iteration
            // after Phase 1 to call save_analysis_note before we force text output.
            return SafeguardAction::ForceNoToolsAndContinue(
                "⚠️ 迭代即将用完且尚无文字输出。下一次迭代将禁用所有工具。\n\
                 请立即停止调用工具，直接用文字向用户总结本步骤的分析结果。"
                    .to_string(),
            );
            // Phase 3 is automatic: caller sets force_no_tools=true when receiving
            // ForceNoToolsAndContinue, which causes empty tool_defs in the next
            // stream_message call, physically preventing tool use.
        }
    }

    SafeguardAction::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_no_action_when_far_from_limit() {
        let mut injected = false;
        let action = check_iteration(0, 10, "some content", false, false, &mut injected, false);
        assert!(matches!(action, SafeguardAction::Continue));
    }

    #[test]
    fn daily_injects_when_near_limit_and_no_content() {
        let mut injected = false;
        // iteration 7 >= max_iterations(10) - 3 = 7, full_content empty
        let action = check_iteration(7, 10, "", false, false, &mut injected, false);
        assert!(matches!(
            action,
            SafeguardAction::InjectPromptAndContinue(_)
        ));
    }

    #[test]
    fn daily_no_inject_when_near_limit_but_has_content() {
        let mut injected = false;
        let action = check_iteration(7, 10, "some text", false, false, &mut injected, false);
        assert!(matches!(action, SafeguardAction::Continue));
    }

    #[test]
    fn analysis_phase1_injects_when_no_note_and_not_yet_injected() {
        let mut injected = false;
        // iteration 9 >= max_iterations(15) - 6 = 9, has_saved_note=false, not yet injected
        let action = check_iteration(9, 15, "", true, false, &mut injected, false);
        assert!(matches!(
            action,
            SafeguardAction::InjectPromptAndContinue(_)
        ));
        assert!(injected);
    }

    #[test]
    fn analysis_phase2_forces_no_tools_when_no_content_and_remaining_le_3() {
        let mut injected = true; // phase 1 already injected
                                 // iteration 12 → remaining = 15 - 13 = 2 (<= 3)
        let action = check_iteration(12, 15, "", true, true, &mut injected, false);
        assert!(matches!(
            action,
            SafeguardAction::ForceNoToolsAndContinue(_)
        ));
    }

    #[test]
    fn analysis_phase2_skipped_when_force_no_tools_already_armed() {
        let mut injected = true;
        let action = check_iteration(12, 15, "", true, true, &mut injected, true);
        assert!(matches!(action, SafeguardAction::Continue));
    }

    #[test]
    fn analysis_skipped_when_max_iterations_below_8() {
        let mut injected = false;
        // max_iterations=5 < 8, safeguard should not trigger
        let action = check_iteration(4, 5, "", true, false, &mut injected, false);
        assert!(matches!(action, SafeguardAction::Continue));
    }
}
