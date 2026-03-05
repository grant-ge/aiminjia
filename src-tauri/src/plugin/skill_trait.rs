//! Skill trait — vertical scenario capability packages.
//!
//! A Skill tells the core engine *how* to handle a type of conversation:
//! what prompt to use, which tools to allow, which model to prefer,
//! and how to manage a multi-step workflow.
#![allow(dead_code)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Skill runtime state (persisted per-conversation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillState {
    pub skill_id: String,
    pub current_step: Option<String>,
    #[serde(default)]
    pub step_status: HashMap<String, String>,
    #[serde(default)]
    pub custom_data: serde_json::Value,
}

impl SkillState {
    pub fn new(skill_id: &str) -> Self {
        Self {
            skill_id: skill_id.to_string(),
            current_step: None,
            step_status: HashMap::new(),
            custom_data: serde_json::Value::Null,
        }
    }
}

/// Tool filtering rule — which tools a Skill allows.
#[derive(Debug, Clone)]
pub enum ToolFilter {
    /// All registered tools.
    All,
    /// Only these tools (by name).
    Only(Vec<String>),
    /// All tools except these.
    Exclude(Vec<String>),
}

/// Action after a workflow step completes.
#[derive(Debug, Clone)]
pub enum StepAction {
    /// Wait for user's next message.
    WaitForUser,
    /// Advance to the specified step.
    AdvanceToStep(String),
    /// Skill finished — return to default Skill.
    Finish,
    /// User aborted the workflow.
    Abort,
}

/// Workflow definition — ordered steps with confirmation points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub steps: Vec<WorkflowStep>,
    pub initial_step: String,
}

/// A single step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub display_name: String,
    pub requires_confirmation: bool,
    #[serde(default)]
    pub advance_on: AdvanceMode,
}

/// How a step decides to advance to the next step.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum AdvanceMode {
    /// Any non-abort reply advances to next step.
    Any,
    /// Requires confirmation keyword to advance (default).
    #[default]
    Confirm,
}

/// Model preference for a Skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelPreference {
    /// Use a specific provider (e.g., "claude").
    Provider(String),
    /// Select by capability.
    Capability(ModelCapability),
}

/// Model capability categories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelCapability {
    InstructionFollowing,
    DeepReasoning,
    LongContext,
    CostEfficient,
    CodeGeneration,
}

/// Skill plugin interface.
///
/// A Skill packages everything the core engine needs to handle a
/// vertical scenario: prompt composition, tool filtering, model
/// preference, and optional multi-step workflow management.
#[async_trait]
pub trait Skill: Send + Sync + 'static {
    /// Unique identifier (e.g., "comp-analysis").
    fn id(&self) -> &str;

    /// Display name shown in the UI.
    fn display_name(&self) -> &str;

    /// Short description.
    fn description(&self) -> &str;

    // ── Activation ──

    /// Should this Skill activate for the given message?
    fn should_activate(
        &self,
        message: &str,
        has_files: bool,
        current_skill: &str,
    ) -> bool;

    /// Priority when multiple Skills match (higher wins).
    fn priority(&self) -> u32 {
        0
    }

    // ── Configuration ──

    /// System prompt for this Skill (may vary by step).
    fn system_prompt(&self, state: &SkillState) -> String;

    /// Which tools are available in this Skill/step.
    fn tool_filter(&self, state: &SkillState) -> ToolFilter;

    /// Preferred model (optional).
    fn model_preference(&self, _state: &SkillState) -> Option<ModelPreference> {
        None
    }

    /// Max agent loop iterations.
    fn max_iterations(&self, _state: &SkillState) -> usize {
        10
    }

    /// Output token budget.
    fn token_budget(&self, _state: &SkillState) -> u32 {
        4096
    }

    // ── Workflow (optional) ──

    /// Workflow definition. None = free-form conversation.
    fn workflow(&self) -> Option<WorkflowDefinition> {
        None
    }

    /// Called when a step completes to determine next action.
    fn on_step_complete(
        &self,
        _state: &mut SkillState,
        _user_message: &str,
    ) -> StepAction {
        StepAction::WaitForUser
    }

    /// Extract prompt for checkpoint extraction at step boundaries.
    /// Returns (base_extract_prompt, step_specific_prompt).
    /// Default: empty (checkpoint will skip extraction).
    fn extract_prompt(&self, _step_id: &str) -> (String, String) {
        (String::new(), String::new())
    }

    /// Returns tool names allowed for runtime guard (None = all allowed).
    ///
    /// Unlike `tool_filter()` which controls schema visibility, this method
    /// controls which tools can actually execute at runtime. The LLM sees all
    /// tool schemas (for KV cache stability), but calling a tool not in this
    /// list will be blocked with an error message.
    fn allowed_tool_names(&self, _state: &SkillState) -> Option<Vec<String>> {
        None // default: no restriction
    }

    /// Called when the Skill is deactivated.
    fn on_deactivate(&self, _state: &SkillState) {}
}

// ── Shared keyword detection ──

/// Strip trailing punctuation and lowercase for keyword matching.
fn normalize_for_keyword(text: &str) -> String {
    text.trim()
        .trim_end_matches(|c: char| {
            matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '~' | '～' | '，' | ',' | '、')
        })
        .to_lowercase()
}

/// Check if the user message is a confirmation keyword (exact match, max 20 chars).
pub fn is_confirm_keyword(text: &str) -> bool {
    if text.trim().chars().count() > 20 {
        return false;
    }
    let stripped = normalize_for_keyword(text);
    const PHRASES: &[&str] = &[
        "确认", "继续", "好的", "可以", "没问题", "好", "行", "对",
        "是的", "确定", "通过", "下一步", "继续吧", "没有问题", "同意",
        "好的好的", "可以可以", "好的继续",
        "好的，继续", "可以，下一步", "可以，继续",
        "ok", "okay", "yes", "proceed", "continue", "confirm", "next",
        "lgtm", "looks good",
        "开始", "开始分析", "开始吧", "start",
    ];
    PHRASES.iter().any(|p| stripped == *p)
}

/// Check if the user message is an abort keyword (exact match, max 30 chars).
pub fn is_abort_keyword(text: &str) -> bool {
    if text.trim().chars().count() > 30 {
        return false;
    }
    let stripped = normalize_for_keyword(text);
    const PHRASES: &[&str] = &[
        "算了", "不分析了", "取消", "取消分析", "退出", "退出分析",
        "停止", "停止分析", "不做了", "不用了", "算了吧", "放弃",
        "不需要了", "不需要分析", "不要分析了", "不用分析",
        "还是算了", "还是不用了", "先不分析了", "暂时不需要",
        "cancel", "abort", "stop", "exit", "quit", "nevermind",
        "no", "no thanks", "don't analyze", "skip", "not now",
        "no need", "never mind", "skip analysis",
    ];
    PHRASES.iter().any(|p| stripped == *p)
}
