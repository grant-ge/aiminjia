use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDisplayI18nText {
    #[serde(default, alias = "Name")]
    pub name: Option<String>,
    #[serde(default, alias = "Description")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, rename = "displayI18n", alias = "display_i18n")]
    pub display_i18n: HashMap<String, SkillDisplayI18nText>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::plugin::skill::frontmatter::deserialize_string_or_vec"
    )]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::plugin::skill::frontmatter::deserialize_string_or_vec"
    )]
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
    /// 本地用户上传 / 派活产物。路径 `~/.renlijia/users/{scope}/skills/<id>`。
    /// 重名时优先级最高(覆盖 Tenant + Global)。
    User,
    /// 租户管理员通过 lotus tenant-portal 上传到自己租户的 scope=tenant 技能。
    /// 桌面端登录后从 gateway `/v1/skill-packages` 同步,装到 `~/.renlijia/skills/<id>`
    /// 与 Global 同目录(handler 已在一次请求里返回 tenant+public 两类)。
    /// 重名优先级:User > Tenant > Global。注:Tenant 与 Global 当前装到同一目录,
    /// 区分仅由 `SkillPackageItem.scope` 字段决定加载时打的 source 标签。
    Tenant,
    /// 平台/OPS 上传的 scope=public 技能。
    Global,
}
