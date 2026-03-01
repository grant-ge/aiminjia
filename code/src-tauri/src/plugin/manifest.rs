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
}

#[derive(Debug, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub plugin_type: String, // "tool" or "skill"
    pub runtime: Option<String>, // "python" for script-based tools
    pub handler: Option<String>, // e.g., "handler.py"
}

#[derive(Debug, Deserialize)]
pub struct TriggerConfig {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub requires_files: bool,
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
    #[serde(default = "default_true")]
    pub requires_confirmation: bool,
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
tools_only = ["analyze_file", "execute_python"]
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
}
