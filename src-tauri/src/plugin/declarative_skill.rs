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
    /// Dynamic prompt routing (Phase 12 multi-file-handler).
    /// Steps with a `prompt_router` in TOML get a branch config; at
    /// runtime `resolve_dynamic_prompt` reads the referenced note from
    /// storage, extracts the routing field, and returns the matching
    /// branch prompt (falling back to `default_branch` on any miss).
    step_prompt_branches: HashMap<String, PromptBranchConfig>,
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
    category: String,
    name_en: String,
    short_desc_en: String,
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

/// Runtime branch selector — pre-loaded prompt texts keyed by branch,
/// plus the source path (inside a saved note) that determines which
/// branch to pick.
#[derive(Debug, Clone)]
pub(crate) struct PromptBranchConfig {
    /// note_suffix (without the `note:{conv_id}:` prefix) that stores
    /// the routing JSON. e.g. "step0_intent".
    pub(crate) note_suffix: String,
    /// JSON field whose value is used as the branch key. e.g. "mode".
    pub(crate) field: String,
    /// Pre-loaded prompt text per branch_key.
    pub(crate) branches: HashMap<String, String>,
    /// Branch_key to use when lookup fails (note missing, JSON parse
    /// error, field absent, or value not in `branches`). Guaranteed to
    /// exist in `branches` at load time (else skill load fails).
    pub(crate) default_key: String,
}

/// Parse a `prompt_router` TOML string of form
/// `"note:{suffix}.{field}"` into `(suffix, field)`.
/// Returns `None` if the format doesn't match. Validation is loose
/// on purpose — unknown prefixes fall through to static-prompt mode.
fn parse_prompt_router(raw: &str) -> Option<(String, String)> {
    let rest = raw.strip_prefix("note:")?;
    // Expect exactly one '.' separating suffix and field (note keys
    // themselves never contain dots in this codebase).
    let idx = rest.find('.')?;
    let suffix = rest[..idx].to_string();
    let field = rest[idx + 1..].to_string();
    if suffix.is_empty() || field.is_empty() {
        return None;
    }
    Some((suffix, field))
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
        let category = display.and_then(|d| d.category.clone()).unwrap_or_else(|| "general".to_string());
        let name_en = display.and_then(|d| d.name_en.clone()).unwrap_or_default();
        let short_desc_en = display.and_then(|d| d.short_description_en.clone()).unwrap_or_default();

        // Load workflow and step prompts
        let workflow_path = plugin_dir.join("workflow.toml");
        let (workflow, step_prompts, step_configs, step_prompt_branches) = if workflow_path.exists() {
            let workflow_content = std::fs::read_to_string(&workflow_path)
                .map_err(|e| format!("Failed to read workflow.toml: {}", e))?;
            let wf_manifest: WorkflowManifest = toml::from_str(&workflow_content)
                .map_err(|e| format!("Invalid workflow.toml: {}", e))?;

            let mut prompts = HashMap::new();
            let mut configs = HashMap::new();
            let mut branches: HashMap<String, PromptBranchConfig> = HashMap::new();
            let mut steps = Vec::new();

            for step in &wf_manifest.steps {
                // Static prompt path (existing behavior)
                if let Some(prompt_path) = &step.prompt {
                    let prompt = Self::load_prompt(plugin_dir, prompt_path);
                    if !prompt.is_empty() {
                        prompts.insert(step.id.clone(), prompt);
                    }
                }

                // Dynamic prompt routing (new): if `prompt_router` +
                // `prompts` map + `default_branch` all present and valid,
                // pre-load all branch prompts into step_prompt_branches.
                // Validation is strict — any malformed config fails skill
                // load with a clear error, so mistakes surface early.
                if let Some(router_raw) = &step.prompt_router {
                    let (note_suffix, field) = parse_prompt_router(router_raw)
                        .ok_or_else(|| format!(
                            "step '{}': prompt_router '{}' must be of form 'note:{{suffix}}.{{field}}'",
                            step.id, router_raw
                        ))?;

                    let prompts_map = step.prompts.as_ref().ok_or_else(|| format!(
                        "step '{}': prompt_router set but [steps.prompts] map missing",
                        step.id
                    ))?;
                    if prompts_map.is_empty() {
                        return Err(format!(
                            "step '{}': [steps.prompts] map is empty — at least one branch required",
                            step.id
                        ));
                    }

                    let default_key = step.default_branch.clone().ok_or_else(|| format!(
                        "step '{}': prompt_router set but default_branch missing",
                        step.id
                    ))?;
                    if !prompts_map.contains_key(&default_key) {
                        return Err(format!(
                            "step '{}': default_branch '{}' not found in [steps.prompts] (have: {:?})",
                            step.id, default_key, prompts_map.keys().collect::<Vec<_>>()
                        ));
                    }

                    // Pre-load all branch prompt texts
                    let mut loaded_branches = HashMap::new();
                    for (branch_key, rel_path) in prompts_map {
                        let content = Self::load_prompt(plugin_dir, rel_path);
                        if content.is_empty() {
                            return Err(format!(
                                "step '{}': branch '{}' prompt file '{}' is missing or empty",
                                step.id, branch_key, rel_path
                            ));
                        }
                        loaded_branches.insert(branch_key.clone(), content);
                    }

                    branches.insert(step.id.clone(), PromptBranchConfig {
                        note_suffix,
                        field,
                        branches: loaded_branches,
                        default_key,
                    });
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
            (Some(wf), prompts, configs, branches)
        } else {
            (None, HashMap::new(), HashMap::new(), HashMap::new())
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
            step_prompt_branches,
            workflow,
            step_configs,
            extract_base,
            extract_steps,
            plugin_dir: plugin_dir.to_path_buf(),
            icon,
            short_desc,
            trigger,
            category,
            name_en,
            short_desc_en,
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

    // ─── Dynamic prompt routing (Phase 12) ───────────────────────────────────
    //
    // Steps with `prompt_router` in workflow.toml resolve their prompt at
    // runtime. The agent loop calls `resolve_dynamic_prompt` before each
    // step's streaming and writes the result to `state.resolved_step_prompt`.
    //
    // Resolution:
    //   1. Look up memory key `note:{conv_id}:{note_suffix}`
    //   2. Parse value as JSON, extract `.{field}` as string
    //   3. Return `branches[key]` if key is a known branch, else `branches[default_key]`
    //
    // Any failure (no branches configured, note missing, JSON parse error,
    // field absent) falls back to the default_key silently. This is
    // intentional: static-prompt skills never hit this code path and
    // dynamic-prompt skills always have a working default. Callers can
    // check return `None` to know the step has no dynamic config.

    /// For unit testing without a real AppStorage — takes a pre-resolved
    /// note value (or None to simulate missing note).
    #[cfg(test)]
    pub(crate) fn resolve_dynamic_prompt_from_note_value(
        &self,
        step_id: &str,
        note_json_value: Option<&str>,
    ) -> Option<String> {
        let branch_cfg = self.step_prompt_branches.get(step_id)?;
        let branch_key = note_json_value
            .and_then(|v| parse_branch_from_json(v, &branch_cfg.field))
            .filter(|k| branch_cfg.branches.contains_key(k))
            .unwrap_or_else(|| branch_cfg.default_key.clone());
        branch_cfg.branches.get(&branch_key).cloned()
    }
}

fn resolve_branch_key(
    storage: &crate::storage::file_store::AppStorage,
    note_key: &str,
    field: &str,
) -> Option<String> {
    let raw = storage.get_memory(note_key).ok().flatten()?;
    parse_branch_from_json(&raw, field)
}

fn parse_branch_from_json(raw: &str, field: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    v.get(field).and_then(|f| f.as_str()).map(|s| s.to_string())
}

#[async_trait]
impl Skill for DeclarativeSkill {
    // ─── Dynamic prompt routing (Phase 12) ───────────────────────────────────
    //
    // Steps with `prompt_router` in workflow.toml resolve their prompt at
    // runtime. The agent loop calls this before each step's streaming and
    // writes the result to `state.resolved_step_prompt`.
    //
    // Resolution steps:
    //   1. Look up memory key `note:{conv_id}:{note_suffix}`
    //   2. Parse value as JSON, extract `.{field}` as string
    //   3. Return `branches[key]` if key is a known branch, else `branches[default_key]`
    //
    // Any failure (no branches configured, note missing, JSON parse error,
    // field absent) falls back to the default_key silently. This is
    // intentional: static-prompt skills return `None` (trait default);
    // dynamic-prompt skills always have a working default.
    fn resolve_dynamic_prompt(
        &self,
        state: &SkillState,
        storage: &crate::storage::file_store::AppStorage,
        conversation_id: &str,
    ) -> Option<String> {
        let step_id = state.current_step.as_deref()?;
        let branch_cfg = self.step_prompt_branches.get(step_id)?;

        let note_key = format!("note:{}:{}", conversation_id, branch_cfg.note_suffix);
        let branch_key = resolve_branch_key(storage, &note_key, &branch_cfg.field)
            .filter(|k| branch_cfg.branches.contains_key(k))
            .unwrap_or_else(|| branch_cfg.default_key.clone());

        // `default_key` was validated to exist at load time, so this clone
        // always succeeds for static-default-only paths too.
        branch_cfg.branches.get(&branch_key).cloned()
    }

    fn id(&self) -> &str { &self.id }
    fn display_name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn icon(&self) -> &str { &self.icon }
    fn short_description(&self) -> &str { &self.short_desc }
    fn trigger_text(&self) -> &str { &self.trigger }
    fn category(&self) -> &str { &self.category }
    fn display_name_en(&self) -> &str { &self.name_en }
    fn short_description_en(&self) -> &str { &self.short_desc_en }

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
            // Prefer the runtime-resolved dynamic prompt when the agent
            // loop populated it (Phase 12 multi-file-handler). Falls back
            // to the static per-step prompt for all other skills.
            let step_prompt = state.resolved_step_prompt.as_ref()
                .filter(|p| !p.is_empty())
                .or_else(|| self.step_prompts.get(step).filter(|p| !p.is_empty()));
            if let Some(sp) = step_prompt {
                parts.push(sp.clone());
            }

            // Guide user to upload files when requires_files skill enters step0 without files
            if self.requires_files && step == "step0" && !state.has_files {
                parts.push(
                    "⚠️ 用户尚未上传文件。请先友好地引导用户上传相关数据文件，说明需要什么类型的文件（如 Excel/CSV 工资表、预算表等）。不要尝试调用 load_file，等用户上传文件后再进行数据加载。".to_string()
                );
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
                // Check if a knowledge/ directory exists under scripts/
                let knowledge_path = self.plugin_dir.join("scripts").join("knowledge");
                let knowledge_dir = if knowledge_path.is_dir() {
                    log::info!(
                        "[Skill:{}] on_step_enter: found knowledge dir at '{}'",
                        self.id, knowledge_path.display()
                    );
                    Some(knowledge_path)
                } else {
                    None
                };

                Some(StepPrecompute {
                    python_code: code,
                    cache_key,
                    knowledge_dir,
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

    // ─── Phase 12 dynamic prompt routing tests ───────────────────────────────
    //
    // Build a minimal skill in a tempdir with a `prompt_router` step, then
    // exercise all 4 resolution paths: happy-path (valid mode), missing
    // note (falls back to default), unknown mode (falls back to default),
    // missing field (falls back to default).

    fn build_dynamic_routing_skill() -> (tempfile::TempDir, DeclarativeSkill) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(
            dir.join("plugin.toml"),
            r#"[plugin]
id = "routing-test-skill"
name = "Routing Test"
type = "skill"

[trigger]
keywords = ["routing-test", "rt1", "rt2"]

[display]
category = "general"
icon = "🔀"
"#,
        )
        .unwrap();

        std::fs::create_dir_all(dir.join("prompts")).unwrap();
        std::fs::write(dir.join("prompts/step0.md"), "Step 0 static prompt text for testing routing fallback behavior.").unwrap();
        std::fs::write(dir.join("prompts/step2-compare.md"), "COMPARE branch selected.").unwrap();
        std::fs::write(dir.join("prompts/step2-merge.md"), "MERGE branch selected.").unwrap();
        std::fs::write(dir.join("prompts/step2-summarize.md"), "SUMMARIZE branch selected.").unwrap();

        std::fs::write(
            dir.join("workflow.toml"),
            r#"
[[steps]]
id = "step0"
name = "intent"
prompt = "prompts/step0.md"
advance_on = "any"

[[steps]]
id = "step2"
name = "execute"
prompt_router = "note:step0_intent.mode"
default_branch = "compare"
advance_on = "confirm"

[steps.prompts]
compare = "prompts/step2-compare.md"
merge = "prompts/step2-merge.md"
summarize = "prompts/step2-summarize.md"
"#,
        )
        .unwrap();

        let manifest = parse_plugin_manifest(&std::fs::read_to_string(dir.join("plugin.toml")).unwrap()).unwrap();
        let skill = DeclarativeSkill::load(&manifest, dir).unwrap();
        (tmp, skill)
    }

    #[test]
    fn dynamic_prompt_routes_to_matching_branch() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        // mode = "merge" → should pick step2-merge.md
        let p = skill
            .resolve_dynamic_prompt_from_note_value("step2", Some(r#"{"mode":"merge"}"#))
            .expect("branch should resolve");
        assert!(p.contains("MERGE branch"), "got: {}", p);
    }

    #[test]
    fn dynamic_prompt_falls_back_to_default_when_note_missing() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        // None → default_branch "compare"
        let p = skill
            .resolve_dynamic_prompt_from_note_value("step2", None)
            .expect("fallback should resolve");
        assert!(p.contains("COMPARE branch"), "got: {}", p);
    }

    #[test]
    fn dynamic_prompt_falls_back_to_default_when_mode_unknown() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        // mode = "frobnicate" (not in branches) → default_branch "compare"
        let p = skill
            .resolve_dynamic_prompt_from_note_value("step2", Some(r#"{"mode":"frobnicate"}"#))
            .expect("fallback should resolve");
        assert!(p.contains("COMPARE branch"), "got: {}", p);
    }

    #[test]
    fn dynamic_prompt_falls_back_to_default_when_field_missing() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        // JSON valid but no `mode` field → default_branch "compare"
        let p = skill
            .resolve_dynamic_prompt_from_note_value("step2", Some(r#"{"other_field":"x"}"#))
            .expect("fallback should resolve");
        assert!(p.contains("COMPARE branch"), "got: {}", p);
    }

    #[test]
    fn dynamic_prompt_returns_none_for_static_prompt_step() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        // step0 uses static prompt = no branches configured
        assert!(skill
            .resolve_dynamic_prompt_from_note_value("step0", Some(r#"{"mode":"merge"}"#))
            .is_none());
    }

    #[test]
    fn dynamic_prompt_returns_none_for_unknown_step() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        assert!(skill
            .resolve_dynamic_prompt_from_note_value("step-nonexistent", None)
            .is_none());
    }

    // ─── Validation at load time ─────────────────────────────────────────────

    #[test]
    fn load_fails_when_default_branch_not_in_prompts_map() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("plugin.toml"),
            r#"[plugin]
id = "bad-routing"
name = "Bad"
type = "skill"
[trigger]
keywords = ["a", "b", "c"]
[display]
category = "general"
icon = "X"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("prompts")).unwrap();
        std::fs::write(dir.join("prompts/step2-compare.md"), "x").unwrap();
        std::fs::write(
            dir.join("workflow.toml"),
            r#"
[[steps]]
id = "step2"
name = "exec"
prompt_router = "note:s0.mode"
default_branch = "missing_branch"

[steps.prompts]
compare = "prompts/step2-compare.md"
"#,
        )
        .unwrap();
        let manifest = parse_plugin_manifest(&std::fs::read_to_string(dir.join("plugin.toml")).unwrap()).unwrap();
        let err = match DeclarativeSkill::load(&manifest, dir) {
            Ok(_) => panic!("expected load to fail"),
            Err(e) => e,
        };
        assert!(err.contains("default_branch 'missing_branch' not found"), "got: {}", err);
    }

    #[test]
    fn load_fails_when_prompt_router_malformed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("plugin.toml"), r#"[plugin]
id = "bad-routing2"
name = "Bad2"
type = "skill"
[trigger]
keywords = ["a", "b", "c"]
[display]
category = "general"
icon = "X"
"#).unwrap();
        std::fs::create_dir_all(dir.join("prompts")).unwrap();
        std::fs::write(dir.join("prompts/step2-compare.md"), "x").unwrap();
        std::fs::write(
            dir.join("workflow.toml"),
            r#"
[[steps]]
id = "step2"
name = "exec"
prompt_router = "bad-format-no-note-prefix"
default_branch = "compare"

[steps.prompts]
compare = "prompts/step2-compare.md"
"#,
        )
        .unwrap();
        let manifest = parse_plugin_manifest(&std::fs::read_to_string(dir.join("plugin.toml")).unwrap()).unwrap();
        let err = match DeclarativeSkill::load(&manifest, dir) {
            Ok(_) => panic!("expected load to fail"),
            Err(e) => e,
        };
        assert!(err.contains("must be of form"), "got: {}", err);
    }

    #[test]
    fn system_prompt_uses_resolved_over_static() {
        let (_tmp, skill) = build_dynamic_routing_skill();
        // step2 has prompt_router — normally static prompt is empty. Set
        // resolved_step_prompt manually and confirm system_prompt includes it.
        let mut state = SkillState::new("routing-test-skill");
        state.current_step = Some("step2".into());
        state.resolved_step_prompt = Some("RESOLVED DYNAMIC CONTENT".into());

        let sp = skill.system_prompt(&state);
        assert!(sp.contains("RESOLVED DYNAMIC CONTENT"), "got: {}", sp);
    }
}