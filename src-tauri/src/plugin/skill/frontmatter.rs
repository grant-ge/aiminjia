use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer};

use super::types::{ParsedSkillMd, SkillFrontmatter};

pub fn parse_skill_md(input: &str) -> Result<ParsedSkillMd> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let Some(rest) = input.strip_prefix("---\n") else {
        bail!("SKILL.md missing YAML frontmatter");
    };
    let Some(end) = rest.find("\n---") else {
        bail!("SKILL.md frontmatter is not closed with ---");
    };
    let yaml = &rest[..end];
    let body_start = end + "\n---".len();
    let body = rest[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&rest[body_start..])
        .to_string();

    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml)
        .map_err(|e| anyhow::anyhow!("Failed to parse SKILL.md YAML frontmatter: {e}"))?;

    if frontmatter.name.trim().is_empty() {
        bail!("SKILL.md frontmatter field 'name' is required");
    }
    if frontmatter.description.trim().is_empty() {
        bail!("SKILL.md frontmatter field 'description' is required");
    }

    Ok(ParsedSkillMd { frontmatter, body })
}

pub fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    let value = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(match value {
        None => Vec::new(),
        Some(StringOrVec::String(s)) => shell_words::split(&s)
            .map_err(serde::de::Error::custom)?,
        Some(StringOrVec::Vec(v)) => v,
    })
}
