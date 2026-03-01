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

    /// Called when the Skill is deactivated.
    fn on_deactivate(&self, _state: &SkillState) {}
}
