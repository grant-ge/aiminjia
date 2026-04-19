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
            "请停止调用工具，直接用文字向用户总结目前的分析结果。".to_string(),
        );
    }

    SafeguardAction::Continue
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
        assert!(matches!(
            action,
            SafeguardAction::InjectPromptAndContinue(_)
        ));
    }

    #[test]
    fn daily_no_inject_when_near_limit_but_has_content() {
        let action = check_iteration(7, 10, "some text");
        assert!(matches!(action, SafeguardAction::Continue));
    }
}
