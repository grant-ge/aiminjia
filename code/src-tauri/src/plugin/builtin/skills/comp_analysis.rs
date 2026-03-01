//! Compensation Analysis Skill — 6-step structured analysis workflow.
//!
//! Migrated from orchestrator.rs detection + prompts.rs prompt selection +
//! tools.rs step filtering. The Skill now owns all business logic that was
//! previously scattered across these modules.

use async_trait::async_trait;

use crate::llm::prompts;
use crate::plugin::skill_trait::*;

pub struct CompAnalysisSkill;

#[async_trait]
impl Skill for CompAnalysisSkill {
    fn id(&self) -> &str { "comp-analysis" }
    fn display_name(&self) -> &str { "薪酬公平性分析" }
    fn description(&self) -> &str { "6-step compensation equity analysis workflow" }

    fn should_activate(
        &self,
        message: &str,
        has_files: bool,
        current_skill: &str,
    ) -> bool {
        // Only activate from the default daily-assistant skill
        if current_skill != "daily-assistant" {
            return false;
        }

        let text = message.to_lowercase();

        // Explicit analysis request keywords
        let explicit = [
            "薪酬分析", "薪酬诊断", "公平性分析", "薪酬公平",
            "开始分析", "帮我分析", "做一次分析", "深度分析",
            "compensation analysis", "pay equity", "salary analysis",
            "fairness analysis",
        ];
        if explicit.iter().any(|kw| text.contains(kw)) {
            return true;
        }

        // File upload + salary keywords
        if has_files {
            let salary = [
                "工资", "薪酬", "薪资", "工资表", "薪酬表",
                "salary", "compensation", "payroll", "wage",
            ];
            if salary.iter().any(|kw| text.contains(kw)) {
                return true;
            }
        }

        false
    }

    fn priority(&self) -> u32 {
        10 // higher than default
    }

    fn system_prompt(&self, state: &SkillState) -> String {
        let step_num = state.current_step.as_deref()
            .and_then(|s| s.strip_prefix("step"))
            .and_then(|n| n.parse::<u32>().ok());

        prompts::get_system_prompt(step_num)
    }

    fn tool_filter(&self, state: &SkillState) -> ToolFilter {
        match state.current_step.as_deref() {
            Some("step0") => ToolFilter::Only(vec![
                "analyze_file".into(),
                "save_analysis_note".into(),
            ]),
            Some("step1") => ToolFilter::Only(vec![
                "analyze_file".into(),
                "execute_python".into(),
                "save_analysis_note".into(),
                "update_progress".into(),
            ]),
            Some("step2") => ToolFilter::Only(vec![
                "execute_python".into(),
                "web_search".into(),
                "save_analysis_note".into(),
                "update_progress".into(),
            ]),
            Some("step3") => ToolFilter::Only(vec![
                "execute_python".into(),
                "web_search".into(),
                "save_analysis_note".into(),
                "update_progress".into(),
            ]),
            Some("step4") => ToolFilter::Only(vec![
                "execute_python".into(),
                "hypothesis_test".into(),
                "detect_anomalies".into(),
                "generate_chart".into(),
                "save_analysis_note".into(),
                "update_progress".into(),
            ]),
            Some("step5") => ToolFilter::Only(vec![
                "execute_python".into(),
                "generate_report".into(),
                "generate_chart".into(),
                "export_data".into(),
                "save_analysis_note".into(),
                "update_progress".into(),
            ]),
            _ => ToolFilter::All,
        }
    }

    fn model_preference(&self, _state: &SkillState) -> Option<ModelPreference> {
        Some(ModelPreference::Capability(ModelCapability::DeepReasoning))
    }

    fn max_iterations(&self, state: &SkillState) -> usize {
        match state.current_step.as_deref() {
            Some("step0") => 5,
            Some("step1") => 15,
            Some("step2") => 15,
            Some("step3") => 15,
            Some("step4") => 20, // fairness diagnosis is most complex
            Some("step5") => 15,
            _ => 10,
        }
    }

    fn token_budget(&self, _state: &SkillState) -> u32 {
        8192
    }

    fn workflow(&self) -> Option<WorkflowDefinition> {
        Some(WorkflowDefinition {
            initial_step: "step0".into(),
            steps: vec![
                WorkflowStep { id: "step0".into(), display_name: "分析方向确认".into(), requires_confirmation: true },
                WorkflowStep { id: "step1".into(), display_name: "数据清洗".into(), requires_confirmation: true },
                WorkflowStep { id: "step2".into(), display_name: "岗位归一化".into(), requires_confirmation: true },
                WorkflowStep { id: "step3".into(), display_name: "职级推断".into(), requires_confirmation: true },
                WorkflowStep { id: "step4".into(), display_name: "公平性诊断".into(), requires_confirmation: true },
                WorkflowStep { id: "step5".into(), display_name: "行动方案".into(), requires_confirmation: true },
            ],
        })
    }

    fn on_step_complete(&self, state: &mut SkillState, user_message: &str) -> StepAction {
        let text = user_message.trim();

        // Check abort (always checked first)
        if is_abort_keyword(text) {
            return StepAction::Abort;
        }

        match state.current_step.as_deref() {
            Some("step0") => {
                // Step 0 (direction confirmation): ANY non-abort response advances.
                // The user's response IS the analysis direction.
                StepAction::AdvanceToStep("step1".into())
            }
            Some("step5") => {
                // Final step: confirm → finish, feedback → re-run
                if is_confirm_keyword(text) {
                    StepAction::Finish
                } else {
                    StepAction::WaitForUser
                }
            }
            Some(step) => {
                // Steps 1–4: confirm → advance, feedback → re-run
                if is_confirm_keyword(text) {
                    if let Some(next) = next_step(step) {
                        StepAction::AdvanceToStep(next)
                    } else {
                        StepAction::Finish
                    }
                } else {
                    StepAction::WaitForUser
                }
            }
            None => StepAction::AdvanceToStep("step0".into()),
        }
    }
}

fn next_step(current: &str) -> Option<String> {
    match current {
        "step0" => Some("step1".into()),
        "step1" => Some("step2".into()),
        "step2" => Some("step3".into()),
        "step3" => Some("step4".into()),
        "step4" => Some("step5".into()),
        _ => None,
    }
}

fn is_confirm_keyword(text: &str) -> bool {
    if text.chars().count() > 20 {
        return false;
    }
    let stripped = text
        .trim_end_matches(|c: char| {
            matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '~' | '～' | '，' | ',' | '、')
        })
        .to_lowercase();
    let phrases = [
        "确认", "继续", "好的", "可以", "没问题", "好", "行", "对",
        "是的", "确定", "通过", "下一步", "继续吧", "没有问题", "同意",
        "好的好的", "可以可以", "好的继续",
        "好的，继续", "可以，下一步", "可以，继续",
        "ok", "okay", "yes", "proceed", "continue", "confirm", "next",
        "lgtm", "looks good",
        "开始", "开始分析", "开始吧", "start",
    ];
    phrases.iter().any(|p| stripped == *p)
}

fn is_abort_keyword(text: &str) -> bool {
    if text.chars().count() > 20 {
        return false;
    }
    let stripped = text
        .trim_end_matches(|c: char| {
            matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '~' | '～' | '，' | ',' | '、')
        })
        .to_lowercase();
    let phrases = [
        "算了", "不分析了", "取消", "取消分析", "退出", "退出分析",
        "停止", "停止分析", "不做了", "不用了", "算了吧", "放弃",
        "cancel", "abort", "stop", "exit", "quit", "nevermind",
        "no", "no thanks", "don't analyze", "skip",
    ];
    phrases.iter().any(|p| stripped == *p)
}
