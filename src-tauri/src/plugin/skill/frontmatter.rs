use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer};

use super::types::{ParsedSkillMd, SkillFrontmatter};

pub fn parse_skill_md(input: &str) -> Result<ParsedSkillMd> {
    // Tolerate UTF-8 BOM (Windows Notepad default) + CRLF line endings (Windows
    // editors / git autocrlf=true). Without this, SKILL.md saved on Windows
    // would fail with "missing YAML frontmatter" — same family as decision 41
    // text_io::read_to_string_strip_bom.
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let rest = input
        .strip_prefix("---\n")
        .or_else(|| input.strip_prefix("---\r\n"))
        .ok_or_else(|| anyhow::anyhow!("SKILL.md missing YAML frontmatter"))?;
    let end = rest
        .find("\n---")
        .ok_or_else(|| anyhow::anyhow!("SKILL.md frontmatter is not closed with ---"))?;
    let yaml = &rest[..end];
    let body_start = end + "\n---".len();
    let body_raw = &rest[body_start..];
    let body = body_raw
        .strip_prefix("\r\n")
        .or_else(|| body_raw.strip_prefix('\n'))
        .unwrap_or(body_raw)
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
        Some(StringOrVec::String(s)) => shell_words::split(&s).map_err(serde::de::Error::custom)?,
        Some(StringOrVec::Vec(v)) => v,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lf_skill_md() {
        let input = "---\nname: foo\ndescription: bar\n---\nbody";
        let parsed = parse_skill_md(input).unwrap();
        assert_eq!(parsed.frontmatter.name, "foo");
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn parses_crlf_skill_md() {
        let input = "---\r\nname: foo\r\ndescription: bar\r\n---\r\nbody";
        let parsed = parse_skill_md(input).unwrap();
        assert_eq!(parsed.frontmatter.name, "foo");
        assert!(parsed.body.starts_with("body"));
    }

    #[test]
    fn parses_bom_plus_crlf_skill_md() {
        let input = "\u{feff}---\r\nname: foo\r\ndescription: bar\r\n---\r\nbody\r\nline2";
        let parsed = parse_skill_md(input).unwrap();
        assert_eq!(parsed.frontmatter.name, "foo");
        assert!(parsed.body.contains("line2"));
    }

    #[test]
    fn rejects_no_frontmatter() {
        assert!(parse_skill_md("plain markdown\nno fm").is_err());
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        assert!(parse_skill_md("---\nname: foo\ndescription: bar\nbody").is_err());
    }
}
