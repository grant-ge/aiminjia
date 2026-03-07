//! Plugin manifest parsing (plugin.toml + workflow.toml).

use serde::Deserialize;

/// Top-level plugin.toml structure.
#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub trigger: Option<TriggerConfig>,
    pub model: Option<ModelConfig>,
    pub defaults: Option<DefaultsConfig>,
    pub capabilities: Option<CapabilitiesConfig>,
    pub prompts: Option<PromptsConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub plugin_type: String, // "tool" or "skill"
    pub description: Option<String>,
    pub priority: Option<u32>,
    pub runtime: Option<String>, // "python" for script-based tools
    pub handler: Option<String>, // e.g., "handler.py"
}

#[derive(Debug, Deserialize)]
pub struct TriggerConfig {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub requires_files: bool,
    /// Secondary keywords: activate when has_files=true AND message matches these.
    #[serde(default)]
    pub file_keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModelConfig {
    pub preference: Option<String>, // "deep_reasoning", "cost_efficient", etc.
}

#[derive(Debug, Deserialize)]
pub struct DefaultsConfig {
    pub max_iterations: Option<usize>,
    pub token_budget: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CapabilitiesConfig {
    pub file_system: Option<String>, // "workspace", "readonly"
}

/// Prompt composition config.
#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    /// Whether to prepend the app's base.md prompt (default true).
    #[serde(default = "default_true")]
    pub include_app_base: bool,
}

/// Workflow definition from workflow.toml.
#[derive(Debug, Deserialize)]
pub struct WorkflowManifest {
    #[serde(rename = "steps")]
    pub steps: Vec<WorkflowStepManifest>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowStepManifest {
    pub id: String,
    pub name: String,
    pub prompt: Option<String>, // path to prompt .md file
    pub tools_only: Option<Vec<String>>,
    pub tools_exclude: Option<Vec<String>>,
    pub max_iterations: Option<usize>,
    pub token_budget: Option<u32>,
    /// "any" or "confirm" (default "confirm").
    #[serde(default = "default_confirm")]
    pub advance_on: String,
    #[serde(default = "default_true")]
    pub requires_confirmation: bool,
    /// Path to a Python script for deterministic pre-computation.
    /// Executed by Rust before the LLM agent loop starts.
    pub precompute: Option<String>,
    /// Tools available when user provides feedback (non-confirmation).
    /// Switches from display mode to modify mode.
    pub tools_on_feedback: Option<Vec<String>>,
    /// Maximum iterations in feedback/modify mode (default 3).
    pub max_iterations_feedback: Option<usize>,
}

fn default_confirm() -> String {
    "confirm".to_string()
}

fn default_true() -> bool {
    true
}

/// Parse a plugin.toml file.
pub fn parse_plugin_manifest(content: &str) -> Result<PluginManifest, toml::de::Error> {
    toml::from_str(content)
}

/// Parse a workflow.toml file.
pub fn parse_workflow_manifest(content: &str) -> Result<WorkflowManifest, toml::de::Error> {
    toml::from_str(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_manifest() {
        let toml = r#"
[plugin]
id = "resume-parser"
name = "Resume Parser"
type = "tool"
runtime = "python"
handler = "handler.py"

[capabilities]
file_system = "workspace"
"#;
        let manifest = parse_plugin_manifest(toml).unwrap();
        assert_eq!(manifest.plugin.id, "resume-parser");
        assert_eq!(manifest.plugin.plugin_type, "tool");
        assert_eq!(manifest.plugin.runtime.as_deref(), Some("python"));
    }

    #[test]
    fn test_parse_skill_manifest() {
        let toml = r#"
[plugin]
id = "recruit-analysis"
name = "招聘分析"
type = "skill"

[trigger]
keywords = ["招聘分析", "简历筛选"]
requires_files = true

[model]
preference = "deep_reasoning"

[defaults]
max_iterations = 15
token_budget = 8192
"#;
        let manifest = parse_plugin_manifest(toml).unwrap();
        assert_eq!(manifest.plugin.id, "recruit-analysis");
        assert_eq!(manifest.plugin.plugin_type, "skill");
        assert!(manifest.trigger.as_ref().unwrap().requires_files);
        assert_eq!(
            manifest.trigger.as_ref().unwrap().keywords,
            vec!["招聘分析", "简历筛选"]
        );
    }

    #[test]
    fn test_parse_workflow_manifest() {
        let toml = r#"
[[steps]]
id = "step1"
name = "数据分析"
prompt = "prompts/step1.md"
tools_only = ["load_file", "execute_python"]
max_iterations = 10
requires_confirmation = true

[[steps]]
id = "step2"
name = "报告生成"
prompt = "prompts/step2.md"
tools_only = ["generate_report", "generate_chart"]
max_iterations = 15
"#;
        let manifest = parse_workflow_manifest(toml).unwrap();
        assert_eq!(manifest.steps.len(), 2);
        assert_eq!(manifest.steps[0].id, "step1");
        assert_eq!(manifest.steps[0].tools_only.as_ref().unwrap().len(), 2);
        assert!(manifest.steps[1].requires_confirmation); // default true
    }

    #[test]
    fn test_parse_workflow_manifest_with_precompute() {
        let toml = r#"
[[steps]]
id = "step1"
name = "数据清洗"
prompt = "prompts/step1.md"
precompute = "scripts/step1.py"
tools_only = ["export_data"]
tools_on_feedback = ["execute_python", "export_data"]
max_iterations = 5
max_iterations_feedback = 3
advance_on = "confirm"
"#;
        let manifest = parse_workflow_manifest(toml).unwrap();
        assert_eq!(manifest.steps.len(), 1);
        let step = &manifest.steps[0];
        assert_eq!(step.precompute.as_deref(), Some("scripts/step1.py"));
        assert_eq!(step.tools_on_feedback.as_ref().unwrap(), &["execute_python", "export_data"]);
        assert_eq!(step.max_iterations_feedback, Some(3));
        assert_eq!(step.tools_only.as_ref().unwrap(), &["export_data"]);
        assert_eq!(step.max_iterations, Some(5));
    }

    #[test]
    fn test_parse_workflow_manifest_precompute_optional() {
        // Existing TOML without precompute fields should parse fine
        let toml = r#"
[[steps]]
id = "step0"
name = "确认方向"
tools_only = ["load_file"]
max_iterations = 5
"#;
        let manifest = parse_workflow_manifest(toml).unwrap();
        let step = &manifest.steps[0];
        assert!(step.precompute.is_none());
        assert!(step.tools_on_feedback.is_none());
        assert!(step.max_iterations_feedback.is_none());
    }
}
