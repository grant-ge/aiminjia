use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default, deserialize_with = "crate::plugin::skill::frontmatter::deserialize_string_or_vec")]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    #[serde(default, deserialize_with = "crate::plugin::skill::frontmatter::deserialize_string_or_vec")]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    #[serde(default)]
    pub disable_model_invocation: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub hooks: serde_yaml::Value,
    #[serde(default)]
    pub shell: Option<String>,
    /// Category for skill marketplace browsing (e.g. "hr", "finance",
    /// "legal", "sales", "ops", "general"). Optional; clients fall back
    /// to "general" when missing.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub metadata: SkillMetadata,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkillMd {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
}

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DiskSkill {
    pub id: String,
    pub root: PathBuf,
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub source: SkillSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    User,
    Global,
}
