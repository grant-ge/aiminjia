//! Plugin manifest parsing (plugin.toml + workflow.toml).

use serde::Deserialize;
use std::path::Path;

/// Display metadata for UI skill cards.
#[derive(Debug, Deserialize)]
pub struct DisplayConfig {
    pub icon: Option<String>,
    pub short_description: Option<String>,
    pub trigger_text: Option<String>,
    pub category: Option<String>,
    pub name_en: Option<String>,
    pub short_description_en: Option<String>,
}

/// Top-level plugin.toml structure.
#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub trigger: Option<TriggerConfig>,
    pub model: Option<ModelConfig>,
    pub defaults: Option<DefaultsConfig>,
    pub capabilities: Option<CapabilitiesConfig>,
    pub prompts: Option<PromptsConfig>,
    pub display: Option<DisplayConfig>,
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
    pub prompt: Option<String>, // path to prompt .md file (static mode)
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
    // ─── Dynamic prompt routing (multi-file-handler, Phase 12) ─────────────
    /// Map of branch_key → prompt file path. When present, resolves the
    /// prompt at runtime based on `prompt_router` instead of the static
    /// `prompt` field. Example:
    ///
    /// ```toml
    /// [steps.prompts]
    /// compare = "prompts/step2-compare.md"
    /// merge   = "prompts/step2-merge.md"
    /// ```
    pub prompts: Option<std::collections::HashMap<String, String>>,
    /// Dotted path locating the branch_key inside a saved note.
    /// Format: `note:{note_suffix}.{json_field}` (conversation_id is
    /// injected automatically).
    ///
    /// Example: `"note:step0_intent.mode"` resolves to memory key
    /// `note:{conv_id}:step0_intent`, parsed as JSON, read `.mode` field.
    pub prompt_router: Option<String>,
    /// Branch_key to use when the router source is missing or its value
    /// isn't in the `prompts` map. Must be a key in `prompts`.
    pub default_branch: Option<String>,
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

/// Read a skill/plugin manifest from a skill directory.
///
/// Migration window behavior:
/// 1. Prefer `plugin.toml` when present.
/// 2. Fallback to `SKILL.md` frontmatter for skill-only manifests.
pub fn read_manifest_from_skill_dir(skill_dir: &Path) -> Result<PluginManifest, String> {
    let plugin_toml_path = skill_dir.join("plugin.toml");
    if plugin_toml_path.exists() {
        let content = std::fs::read_to_string(&plugin_toml_path).map_err(|e| e.to_string())?;
        return parse_plugin_manifest(&content).map_err(|e| e.to_string());
    }

    let skill_md_path = skill_dir.join("SKILL.md");
    if skill_md_path.exists() {
        let content = std::fs::read_to_string(&skill_md_path).map_err(|e| e.to_string())?;
        return parse_skill_md_manifest(&content);
    }

    Err("No plugin.toml or SKILL.md found".to_string())
}

fn parse_skill_md_manifest(content: &str) -> Result<PluginManifest, String> {
    let frontmatter = extract_frontmatter(content)?;
    let map = parse_frontmatter_map(&frontmatter);

    let name = map
        .get("name")
        .cloned()
        .filter(|v| !v.is_empty())
        .ok_or("SKILL.md frontmatter missing required field 'name'")?;
    let id = map
        .get("id")
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| slugify_name(&name));

    let keywords = parse_list(map.get("keywords"));
    let file_keywords = parse_list(map.get("file_keywords"));
    let requires_files = parse_bool(map.get("requires_files")).unwrap_or(false);
    let preference = map
        .get("model_preference")
        .cloned()
        .or_else(|| map.get("preference").cloned());
    let max_iterations = parse_usize(map.get("max_iterations"));
    let token_budget = parse_u32(map.get("token_budget"));
    let include_app_base = parse_bool(map.get("include_app_base"));

    Ok(PluginManifest {
        plugin: PluginMeta {
            id,
            name,
            plugin_type: "skill".to_string(),
            description: map.get("description").cloned(),
            priority: parse_u32(map.get("priority")),
            runtime: None,
            handler: None,
        },
        trigger: if keywords.is_empty() && file_keywords.is_empty() && !requires_files {
            None
        } else {
            Some(TriggerConfig {
                keywords,
                requires_files,
                file_keywords,
            })
        },
        model: preference.map(|p| ModelConfig {
            preference: Some(p),
        }),
        defaults: if max_iterations.is_none() && token_budget.is_none() {
            None
        } else {
            Some(DefaultsConfig {
                max_iterations,
                token_budget,
            })
        },
        capabilities: None,
        prompts: Some(PromptsConfig {
            include_app_base: include_app_base.unwrap_or(true),
        }),
        display: None,
    })
}

fn extract_frontmatter(content: &str) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return Err("SKILL.md must start with YAML frontmatter delimited by ---".to_string());
    }

    for idx in 1..lines.len() {
        if lines[idx].trim() == "---" {
            return Ok(lines[1..idx].join("\n"));
        }
    }

    Err("SKILL.md frontmatter closing delimiter '---' not found".to_string())
}

fn parse_frontmatter_map(frontmatter: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let lines: Vec<&str> = frontmatter.lines().collect();
    let mut i = 0usize;

    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim();
        i += 1;

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim().to_string();
        let value = v.trim();

        if value.is_empty() {
            let mut items = Vec::new();
            while i < lines.len() {
                let child = lines[i].trim();
                if let Some(item) = child.strip_prefix("- ") {
                    items.push(strip_quotes(item.trim()).to_string());
                    i += 1;
                    continue;
                }
                break;
            }
            out.insert(key, format!("[{}]", items.join(",")));
            continue;
        }

        out.insert(key, strip_quotes(value).to_string());
    }

    out
}

fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn parse_list(v: Option<&String>) -> Vec<String> {
    let Some(raw) = v else {
        return Vec::new();
    };
    let trimmed = raw.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return Vec::new();
    }
    trimmed[1..trimmed.len() - 1]
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| strip_quotes(s).to_string())
        .collect()
}

fn parse_bool(v: Option<&String>) -> Option<bool> {
    v.and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn parse_usize(v: Option<&String>) -> Option<usize> {
    v.and_then(|raw| raw.trim().parse::<usize>().ok())
}

fn parse_u32(v: Option<&String>) -> Option<u32> {
    v.and_then(|raw| raw.trim().parse::<u32>().ok())
}

fn slugify_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
        assert_eq!(
            step.tools_on_feedback.as_ref().unwrap(),
            &["execute_python", "export_data"]
        );
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

    #[test]
    fn test_read_manifest_from_skill_dir_with_plugin_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plugin-toml-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            r#"[plugin]
id = "skill-from-plugin"
name = "Skill From Plugin"
type = "skill"
"#,
        )
        .unwrap();

        let manifest = read_manifest_from_skill_dir(Path::new(&dir)).unwrap();
        assert_eq!(manifest.plugin.id, "skill-from-plugin");
        assert_eq!(manifest.plugin.plugin_type, "skill");
    }

    #[test]
    fn test_read_manifest_prefers_plugin_toml_when_both_manifest_files_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skill-both");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            r#"[plugin]
id = "skill-from-plugin"
name = "Skill From Plugin"
type = "skill"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            r#"---
id: "skill-from-md"
name: "Skill From Md"
---
# Body
"#,
        )
        .unwrap();

        let manifest = read_manifest_from_skill_dir(Path::new(&dir)).unwrap();
        assert_eq!(manifest.plugin.id, "skill-from-plugin");
        assert_eq!(manifest.plugin.name, "Skill From Plugin");
    }

    #[test]
    fn test_read_manifest_from_skill_dir_with_skill_md_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skill-md-only");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            r#"---
id: "skill-from-md"
name: "Skill From Md"
description: "desc"
keywords: ["k1", "k2"]
requires_files: true
max_iterations: 7
token_budget: 5000
---
# Body
"#,
        )
        .unwrap();

        let manifest = read_manifest_from_skill_dir(Path::new(&dir)).unwrap();
        assert_eq!(manifest.plugin.id, "skill-from-md");
        assert_eq!(manifest.plugin.name, "Skill From Md");
        assert_eq!(manifest.plugin.plugin_type, "skill");
        assert_eq!(
            manifest
                .trigger
                .as_ref()
                .map(|t| t.keywords.clone())
                .unwrap_or_default(),
            vec!["k1".to_string(), "k2".to_string()]
        );
        assert!(manifest
            .trigger
            .as_ref()
            .map(|t| t.requires_files)
            .unwrap_or(false));
        assert_eq!(
            manifest.defaults.as_ref().and_then(|d| d.max_iterations),
            Some(7)
        );
        assert_eq!(manifest.defaults.as_ref().and_then(|d| d.token_budget), Some(5000));
    }
}
