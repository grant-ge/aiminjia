//! Declarative Skill — loads a Skill from TOML + Markdown prompt files.
//!
//! Teams can create Skills without writing Rust by placing a plugin.toml,
//! workflow.toml, and prompt .md files in a plugin directory.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;

use super::manifest::{PluginManifest, WorkflowManifest};
use super::skill_trait::*;

/// A Skill loaded from TOML + Markdown files.
pub struct DeclarativeSkill {
    id: String,
    name: String,
    description: String,
    priority_val: u32,
    keywords: Vec<String>,
    file_keywords: Vec<String>,
    requires_files: bool,
    model_pref: Option<ModelPreference>,
    max_iter: usize,
    budget: u32,
    include_app_base: bool,
    base_prompt: String,
    step_prompts: HashMap<String, String>,
    workflow: Option<WorkflowDefinition>,
    step_configs: HashMap<String, StepToolConfig>,
    extract_base: String,
    extract_steps: HashMap<String, String>,
    /// Plugin directory path — needed to load precompute scripts at runtime.
    plugin_dir: std::path::PathBuf,
    /// Display metadata for UI skill cards.
    icon: String,
    short_desc: String,
    trigger: String,
}

struct StepToolConfig {
    tools_only: Option<Vec<String>>,
    tools_exclude: Option<Vec<String>>,
    max_iterations: Option<usize>,
    token_budget: Option<u32>,
    advance_on: AdvanceMode,
    /// Path to precompute Python script (relative to plugin dir).
    precompute: Option<String>,
    /// Tools for feedback/modify mode.
    tools_on_feedback: Option<Vec<String>>,
    /// Max iterations in feedback mode.
    max_iterations_feedback: Option<usize>,
}

impl DeclarativeSkill {
    /// Load a declarative Skill from a plugin directory.
    pub fn load(manifest: &PluginManifest, plugin_dir: &Path) -> Result<Self, String> {
        let trigger = manifest.trigger.as_ref();
        let keywords = trigger.map(|t| t.keywords.clone()).unwrap_or_default();
        let file_keywords = trigger.map(|t| t.file_keywords.clone()).unwrap_or_default();
        let requires_files = trigger.map(|t| t.requires_files).unwrap_or(false);

        let priority_val = manifest.plugin.priority.unwrap_or(0);
        let description = manifest.plugin.description.clone()
            .unwrap_or_else(|| format!("{} (plugin)", manifest.plugin.name));

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

        let include_app_base = manifest.prompts.as_ref()
            .map(|p| p.include_app_base)
            .unwrap_or(true);

        // Load plugin-local base prompt (lives in prompts/ subdirectory alongside step prompts)
        let base_prompt = Self::load_prompt(plugin_dir, "prompts/base.md");

        // Load extract prompts (for checkpoint extraction at step boundaries)
        let extract_base = Self::load_prompt(plugin_dir, "prompts/extract/base_extract.md");
        let mut extract_steps = HashMap::new();
        let extract_dir = plugin_dir.join("prompts/extract");
        if extract_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&extract_dir) {
                for entry in entries.flatten() {
                    let fname = entry.file_name();
                    let fname = fname.to_string_lossy();
                    if let Some(step_id) = fname.strip_prefix("extract_").and_then(|s| s.strip_suffix(".md")) {
                        let content = Self::load_prompt(plugin_dir, &format!("prompts/extract/{}", fname));
                        if !content.is_empty() {
                            extract_steps.insert(step_id.to_string(), content);
                        }
                    }
                }
            }
        }

        // Load display metadata for UI skill cards
        let display = manifest.display.as_ref();
        let icon = display.and_then(|d| d.icon.clone()).unwrap_or_default();
        let short_desc = display.and_then(|d| d.short_description.clone()).unwrap_or_default();
        let trigger = display.and_then(|d| d.trigger_text.clone()).unwrap_or_default();

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

                let advance_on = match step.advance_on.as_str() {
                    "any" => AdvanceMode::Any,
                    _ => AdvanceMode::Confirm,
                };

                configs.insert(step.id.clone(), StepToolConfig {
                    tools_only: step.tools_only.clone(),
                    tools_exclude: step.tools_exclude.clone(),
                    max_iterations: step.max_iterations,
                    token_budget: step.token_budget,
                    advance_on: advance_on.clone(),
                    precompute: step.precompute.clone(),
                    tools_on_feedback: step.tools_on_feedback.clone(),
                    max_iterations_feedback: step.max_iterations_feedback,
                });
                steps.push(WorkflowStep {
                    id: step.id.clone(),
                    display_name: step.name.clone(),
                    requires_confirmation: step.requires_confirmation,
                    advance_on,
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
            description,
            priority_val,
            keywords,
            file_keywords,
            requires_files,
            model_pref,
            max_iter,
            budget,
            include_app_base,
            base_prompt,
            step_prompts,
            workflow,
            step_configs,
            extract_base,
            extract_steps,
            plugin_dir: plugin_dir.to_path_buf(),
            icon,
            short_desc,
            trigger,
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

    fn is_last_step(&self, step_id: &str) -> bool {
        self.workflow.as_ref()
            .and_then(|wf| wf.steps.last())
            .map(|s| s.id == step_id)
            .unwrap_or(false)
    }
}

#[async_trait]
impl Skill for DeclarativeSkill {
    fn id(&self) -> &str { &self.id }
    fn display_name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn icon(&self) -> &str { &self.icon }
    fn short_description(&self) -> &str { &self.short_desc }
    fn trigger_text(&self) -> &str { &self.trigger }

    fn priority(&self) -> u32 { self.priority_val }

    fn should_activate(&self, message: &str, _has_files: bool, current_skill: &str) -> bool {
        if current_skill != "daily-assistant" {
            return false;
        }
        let lower = message.to_lowercase();

        // Only primary keywords trigger activation (explicit analysis requests).
        // Secondary file_keywords path removed: when users upload files with
        // casual mentions of salary/compensation, daily mode should parse first,
        // show a summary, and let the user decide whether to start full analysis.
        self.keywords.iter().any(|kw| lower.contains(&kw.to_lowercase()))
    }

    fn system_prompt(&self, state: &SkillState) -> String {
        // Build prompt: [app_base] + [plugin_base] + [step_prompt] + [tool restriction] + [date]
        let mut parts = Vec::new();

        if self.include_app_base {
            let app_base = crate::llm::prompts::get_base_prompt();
            if !app_base.is_empty() {
                parts.push(app_base);
            }
        }

        if !self.base_prompt.is_empty() {
            parts.push(self.base_prompt.clone());
        }

        if let Some(step) = state.current_step.as_deref() {
            if let Some(sp) = self.step_prompts.get(step) {
                if !sp.is_empty() {
                    parts.push(sp.clone());
                }
            }

            // Inject tool restriction instruction from workflow.toml tools_only
            if let Some(config) = self.step_configs.get(step) {
                if let Some(tools) = &config.tools_only {
                    parts.push(format!(
                        "## 本步骤可用工具\n仅使用以下工具：{}。不要调用其他工具。",
                        tools.join(", ")
                    ));
                }
            }
        }

        // Inject current date prominently
        let now = chrono::Local::now();
        let today = now.format("%Y年%m月%d日");
        let today_iso = now.format("%Y-%m-%d");
        parts.push(format!(
            "【当前时间】今天是 {}（{}）。你的回答中涉及时间时，以此日期为准。",
            today, today_iso
        ));

        parts.join("\n\n")
    }

    fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
        // Always expose all tool schemas to the LLM for KV cache prefix stability.
        // Runtime enforcement is handled by allowed_tool_names() + runtime guard.
        ToolFilter::All
    }

    fn allowed_tool_names(&self, state: &SkillState) -> Option<Vec<String>> {
        // Read tools_only from workflow.toml step config for runtime guard
        state.current_step.as_deref()
            .and_then(|step| self.step_configs.get(step))
            .and_then(|config| config.tools_only.clone())
    }

    fn model_preference(&self, _state: &SkillState) -> Option<ModelPreference> {
        self.model_pref.clone()
    }

    fn max_iterations(&self, state: &SkillState) -> usize {
        if let Some(step) = state.current_step.as_deref() {
            if let Some(config) = self.step_configs.get(step) {
                if let Some(mi) = config.max_iterations {
                    log::info!(
                        "[Skill:{}] max_iterations for step '{}': {} (step-specific)",
                        self.id, step, mi
                    );
                    return mi;
                }
            }
            log::info!(
                "[Skill:{}] max_iterations for step '{}': {} (default — step not found or no step override)",
                self.id, step, self.max_iter
            );
        } else {
            log::info!(
                "[Skill:{}] max_iterations: {} (no current_step)",
                self.id, self.max_iter
            );
        }
        self.max_iter
    }

    fn token_budget(&self, state: &SkillState) -> u32 {
        // Per-step budget takes priority over global
        if let Some(step) = state.current_step.as_deref() {
            if let Some(config) = self.step_configs.get(step) {
                if let Some(tb) = config.token_budget {
                    return tb;
                }
            }
        }
        self.budget
    }

    fn workflow(&self) -> Option<WorkflowDefinition> {
        self.workflow.clone()
    }

    fn extract_prompt(&self, step_id: &str) -> (String, String) {
        let step_specific = self.extract_steps.get(step_id).cloned().unwrap_or_default();
        (self.extract_base.clone(), step_specific)
    }

    fn on_step_enter(&self, state: &SkillState) -> Option<StepPrecompute> {
        let step = state.current_step.as_deref()?;
        let config = self.step_configs.get(step)?;
        let script_path = config.precompute.as_deref()?;

        // Read the precompute script from the plugin directory
        let full_path = self.plugin_dir.join(script_path);
        match std::fs::read_to_string(&full_path) {
            Ok(code) => {
                // Use step ID directly as cache key to avoid collisions
                let cache_key = format!("{}_precompute", step);
                log::info!(
                    "[Skill:{}] on_step_enter: loading precompute script '{}' ({} bytes) for step '{}'",
                    self.id, script_path, code.len(), step
                );
                Some(StepPrecompute {
                    python_code: code,
                    cache_key,
                })
            }
            Err(e) => {
                log::warn!(
                    "[Skill:{}] on_step_enter: failed to read precompute script '{}': {}",
                    self.id, full_path.display(), e
                );
                None
            }
        }
    }

    fn feedback_config(&self, state: &SkillState) -> Option<FeedbackConfig> {
        let step = state.current_step.as_deref()?;
        let config = self.step_configs.get(step)?;
        let tools = config.tools_on_feedback.as_ref()?;
        Some(FeedbackConfig {
            tools: tools.clone(),
            max_iterations: config.max_iterations_feedback.unwrap_or(3),
        })
    }

    fn on_step_complete(&self, state: &mut SkillState, user_message: &str) -> StepAction {
        let text = user_message.trim();

        // Abort always checked first (shared function)
        if is_abort_keyword(text) {
            return StepAction::Abort;
        }

        let current = match state.current_step.as_deref() {
            Some(s) => s.to_string(),
            None => {
                // No current step — advance to the initial step (matches old CompAnalysisSkill behavior)
                let initial = self.workflow.as_ref()
                    .map(|wf| wf.initial_step.clone())
                    .unwrap_or_else(|| "step0".to_string());
                return StepAction::AdvanceToStep(initial);
            }
        };

        // Get the advance mode for the current step
        let advance_mode = self.step_configs.get(&current)
            .map(|c| &c.advance_on)
            .cloned()
            .unwrap_or_default();

        match advance_mode {
            AdvanceMode::Any => {
                // Any non-abort reply advances
                if self.is_last_step(&current) {
                    StepAction::Finish
                } else if let Some(next) = self.next_step_id(&current) {
                    StepAction::AdvanceToStep(next)
                } else {
                    StepAction::Finish
                }
            }
            AdvanceMode::Confirm => {
                // Requires confirmation keyword
                if is_confirm_keyword(text) {
                    if self.is_last_step(&current) {
                        StepAction::Finish
                    } else if let Some(next) = self.next_step_id(&current) {
                        StepAction::AdvanceToStep(next)
                    } else {
                        StepAction::Finish
                    }
                } else {
                    StepAction::WaitForUser
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::parse_plugin_manifest;

    /// Load the comp-analysis-v2 plugin and verify all precompute fields are parsed.
    #[test]
    fn test_load_comp_analysis_v2() {
        let plugin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("plugins/comp-analysis-v2");
        if !plugin_dir.exists() {
            return;
        }

        let plugin_toml = std::fs::read_to_string(plugin_dir.join("plugin.toml")).unwrap();
        let manifest = parse_plugin_manifest(&plugin_toml).unwrap();
        assert_eq!(manifest.plugin.id, "comp-analysis-v2");

        let skill = DeclarativeSkill::load(&manifest, &plugin_dir).unwrap();
        assert_eq!(skill.id(), "comp-analysis-v2");
        assert_eq!(skill.priority(), 20);

        // Verify workflow has 6 steps
        let wf = skill.workflow().expect("should have workflow");
        assert_eq!(wf.steps.len(), 6);
        assert_eq!(wf.initial_step, "step0");

        // Step 0: no precompute
        let state0 = SkillState { current_step: Some("step0".into()), ..SkillState::new("comp-analysis-v2") };
        assert!(skill.on_step_enter(&state0).is_none(), "step0 should have no precompute");
        assert!(skill.feedback_config(&state0).is_none(), "step0 should have no feedback config");

        // Step 1: has precompute + feedback config
        let state1 = SkillState { current_step: Some("step1".into()), ..SkillState::new("comp-analysis-v2") };
        let pc = skill.on_step_enter(&state1).expect("step1 should have precompute");
        assert_eq!(pc.cache_key, "step1_precompute");
        assert!(pc.python_code.contains("_step1_clean"), "step1 precompute should call _step1_clean");

        let fb = skill.feedback_config(&state1).expect("step1 should have feedback config");
        assert!(fb.tools.contains(&"execute_python".to_string()));
        assert!(fb.tools.contains(&"export_data".to_string()));
        assert_eq!(fb.max_iterations, 3);

        // Step 5: has precompute + generate_report in allowed tools
        let state5 = SkillState { current_step: Some("step5".into()), ..SkillState::new("comp-analysis-v2") };
        let pc5 = skill.on_step_enter(&state5).expect("step5 should have precompute");
        assert!(pc5.python_code.contains("_step5_scenarios"), "step5 precompute should call _step5_scenarios");

        let allowed5 = skill.allowed_tool_names(&state5).expect("step5 should have tool whitelist");
        assert!(allowed5.contains(&"generate_report".to_string()));
        assert!(allowed5.contains(&"export_data".to_string()));

        // Keyword activation
        assert!(skill.should_activate("请进行薪酬分析v2", true, "daily-assistant"));
        assert!(!skill.should_activate("请进行薪酬分析", true, "daily-assistant"));
    }

    /// Verify max_iterations per step.
    #[test]
    fn test_precompute_step_iterations() {
        let plugin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("plugins/comp-analysis-v2");
        if !plugin_dir.exists() {
            return;
        }

        let plugin_toml = std::fs::read_to_string(plugin_dir.join("plugin.toml")).unwrap();
        let manifest = parse_plugin_manifest(&plugin_toml).unwrap();
        let skill = DeclarativeSkill::load(&manifest, &plugin_dir).unwrap();

        let state1 = SkillState { current_step: Some("step1".into()), ..SkillState::new("comp-analysis-v2") };
        assert_eq!(skill.max_iterations(&state1), 5);

        let state5 = SkillState { current_step: Some("step5".into()), ..SkillState::new("comp-analysis-v2") };
        assert_eq!(skill.max_iterations(&state5), 8);

        let state0 = SkillState { current_step: Some("step0".into()), ..SkillState::new("comp-analysis-v2") };
        assert_eq!(skill.max_iterations(&state0), 5);
    }
}