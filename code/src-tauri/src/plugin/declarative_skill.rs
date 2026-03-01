//! Declarative Skill — loads a Skill from TOML + Markdown prompt files.
//!
//! Teams can create Skills without writing Rust by placing a plugin.toml,
//! workflow.toml, and prompt .md files in a plugin directory.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::manifest::{PluginManifest, WorkflowManifest};
use super::skill_trait::*;

/// A Skill loaded from TOML + Markdown files.
pub struct DeclarativeSkill {
    id: String,
    name: String,
    description: String,
    keywords: Vec<String>,
    requires_files: bool,
    model_pref: Option<ModelPreference>,
    max_iter: usize,
    budget: u32,
    base_prompt: String,
    step_prompts: HashMap<String, String>,
    workflow: Option<WorkflowDefinition>,
    step_configs: HashMap<String, StepToolConfig>,
}

struct StepToolConfig {
    tools_only: Option<Vec<String>>,
    tools_exclude: Option<Vec<String>>,
    max_iterations: Option<usize>,
}

impl DeclarativeSkill {
    /// Load a declarative Skill from a plugin directory.
    pub fn load(manifest: &PluginManifest, plugin_dir: &Path) -> Result<Self, String> {
        let trigger = manifest.trigger.as_ref();
        let keywords = trigger.map(|t| t.keywords.clone()).unwrap_or_default();
        let requires_files = trigger.map(|t| t.requires_files).unwrap_or(false);

        let model_pref = manifest.model.as_ref()
            .and_then(|m| m.preference.as_deref())
            .map(|p| match p {
                "deep_reasoning" => ModelPreference::Capability(ModelCapability::DeepReasoning),
                "cost_efficient" => ModelPreference::Capability(ModelCapability::CostEfficient),
                "long_context" => ModelPreference::Capability(ModelCapability::LongContext),
                "code_generation" => ModelPreference::Capability(ModelCapability::CodeGeneration),
                "instruction_following" => ModelPreference::Capability(ModelCapability::InstructionFollowing),
                other => ModelPreference::Provider(other.to_string()),
            });

        let defaults = manifest.defaults.as_ref();
        let max_iter = defaults.and_then(|d| d.max_iterations).unwrap_or(10);
        let budget = defaults.and_then(|d| d.token_budget).unwrap_or(4096);

        // Load base prompt
        let base_prompt = Self::load_prompt(plugin_dir, "base.md");

        // Load workflow and step prompts
        let workflow_path = plugin_dir.join("workflow.toml");
        let (workflow, step_prompts, step_configs) = if workflow_path.exists() {
            let workflow_content = std::fs::read_to_string(&workflow_path)
                .map_err(|e| format!("Failed to read workflow.toml: {}", e))?;
            let wf_manifest: WorkflowManifest = toml::from_str(&workflow_content)
                .map_err(|e| format!("Invalid workflow.toml: {}", e))?;

            let mut prompts = HashMap::new();
            let mut configs = HashMap::new();
            let mut steps = Vec::new();

            for step in &wf_manifest.steps {
                if let Some(prompt_path) = &step.prompt {
                    let prompt = Self::load_prompt(plugin_dir, prompt_path);
                    if !prompt.is_empty() {
                        prompts.insert(step.id.clone(), prompt);
                    }
                }
                configs.insert(step.id.clone(), StepToolConfig {
                    tools_only: step.tools_only.clone(),
                    tools_exclude: step.tools_exclude.clone(),
                    max_iterations: step.max_iterations,
                });
                steps.push(WorkflowStep {
                    id: step.id.clone(),
                    display_name: step.name.clone(),
                    requires_confirmation: step.requires_confirmation,
                });
            }

            let initial = steps.first().map(|s| s.id.clone()).unwrap_or_default();
            let wf = WorkflowDefinition { steps, initial_step: initial };
            (Some(wf), prompts, configs)
        } else {
            (None, HashMap::new(), HashMap::new())
        };

        Ok(Self {
            id: manifest.plugin.id.clone(),
            name: manifest.plugin.name.clone(),
            description: format!("{} (plugin)", manifest.plugin.name),
            keywords,
            requires_files,
            model_pref,
            max_iter,
            budget,
            base_prompt,
            step_prompts,
            workflow,
            step_configs,
        })
    }

    fn load_prompt(plugin_dir: &Path, rel_path: &str) -> String {
        let path = plugin_dir.join(rel_path);
        match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => {
                log::warn!(
                    "Prompt file not found: {:?} — skill may have empty system prompt",
                    path
                );
                String::new()
            }
        }
    }

    fn next_step_id(&self, current: &str) -> Option<String> {
        let wf = self.workflow.as_ref()?;
        let idx = wf.steps.iter().position(|s| s.id == current)?;
        wf.steps.get(idx + 1).map(|s| s.id.clone())
    }
}

#[async_trait]
impl Skill for DeclarativeSkill {
    fn id(&self) -> &str { &self.id }
    fn display_name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }

    fn should_activate(&self, message: &str, has_files: bool, current_skill: &str) -> bool {
        if current_skill != "daily-assistant" {
            return false;
        }
        if self.requires_files && !has_files {
            return false;
        }
        let lower = message.to_lowercase();
        self.keywords.iter().any(|kw| lower.contains(&kw.to_lowercase()))
    }

    fn system_prompt(&self, state: &SkillState) -> String {
        let step_prompt = state.current_step.as_deref()
            .and_then(|step| self.step_prompts.get(step))
            .cloned()
            .unwrap_or_default();

        if step_prompt.is_empty() {
            self.base_prompt.clone()
        } else {
            format!("{}\n\n{}", self.base_prompt, step_prompt)
        }
    }

    fn tool_filter(&self, state: &SkillState) -> ToolFilter {
        if let Some(step) = state.current_step.as_deref() {
            if let Some(config) = self.step_configs.get(step) {
                if let Some(only) = &config.tools_only {
                    return ToolFilter::Only(only.clone());
                }
                if let Some(exclude) = &config.tools_exclude {
                    return ToolFilter::Exclude(exclude.clone());
                }
            }
        }
        ToolFilter::All
    }

    fn model_preference(&self, _state: &SkillState) -> Option<ModelPreference> {
        self.model_pref.clone()
    }

    fn max_iterations(&self, state: &SkillState) -> usize {
        if let Some(step) = state.current_step.as_deref() {
            if let Some(config) = self.step_configs.get(step) {
                if let Some(mi) = config.max_iterations {
                    return mi;
                }
            }
        }
        self.max_iter
    }

    fn token_budget(&self, _state: &SkillState) -> u32 {
        self.budget
    }

    fn workflow(&self) -> Option<WorkflowDefinition> {
        self.workflow.clone()
    }

    fn on_step_complete(&self, state: &mut SkillState, user_message: &str) -> StepAction {
        // Check for abort keywords
        let lower = user_message.trim().to_lowercase();
        let abort_phrases = ["算了", "取消", "退出", "停止", "cancel", "abort", "stop", "quit"];
        if lower.chars().count() <= 20 && abort_phrases.iter().any(|p| lower.contains(p)) {
            return StepAction::Abort;
        }

        // Simple confirmation detection
        let confirm_phrases = ["确认", "继续", "好的", "可以", "ok", "yes", "next", "continue", "开始"];
        let is_confirm = lower.chars().count() <= 20
            && confirm_phrases.iter().any(|p| lower.contains(p));

        if is_confirm {
            if let Some(current) = state.current_step.as_deref() {
                if let Some(next) = self.next_step_id(current) {
                    return StepAction::AdvanceToStep(next);
                } else {
                    return StepAction::Finish;
                }
            }
        }

        StepAction::WaitForUser
    }
}
