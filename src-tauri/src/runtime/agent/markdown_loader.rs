//! Load `AgentDefinition` from a Markdown file with YAML frontmatter.
//!
//! Format:
//! ```markdown
//! ---
//! name: my_agent
//! description: ...
//! allowed_tools: [...]            # optional, [] = full subset (after disallowed)
//! disallowed_tools: [...]         # optional
//! max_iterations: 20              # optional, default 20
//! model: haiku                    # optional, omitted = inherit parent
//! permission_mode: bubble         # optional, default bubble (one of: bubble | auto_deny)
//! background_default: false       # optional, default false
//! ---
//!
//! Body becomes the system prompt (Inline).
//! ```

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};

#[derive(Deserialize)]
struct AgentFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    disallowed_tools: Vec<String>,
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_permission_mode")]
    permission_mode: String,
    #[serde(default)]
    background_default: bool,
}

fn default_max_iterations() -> usize {
    20
}
fn default_permission_mode() -> String {
    "bubble".into()
}

/// Parse a single agent .md file into an `AgentDefinition` with `source = User`.
pub fn load_agent_from_markdown(path: &Path) -> Result<AgentDefinition> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
    let parsed = matter.parse(&raw);
    let data = parsed
        .data
        .ok_or_else(|| anyhow::anyhow!("missing YAML frontmatter in {}", path.display()))?;
    let fm: AgentFrontmatter = data
        .deserialize()
        .with_context(|| format!("parse frontmatter {}", path.display()))?;

    // Reject blank/whitespace-only required string fields. serde only
    // enforces presence; empty agent names break registry lookups, and an
    // empty system prompt body produces an unusable agent.
    let name = fm.name.trim();
    if name.is_empty() {
        anyhow::bail!("empty `name` in {}", path.display());
    }
    let description = fm.description.trim();
    if description.is_empty() {
        anyhow::bail!("empty `description` in {}", path.display());
    }
    let body = parsed.content.trim();
    if body.is_empty() {
        anyhow::bail!(
            "empty system prompt body in {} (markdown body after frontmatter is required)",
            path.display()
        );
    }
    for tool in fm.allowed_tools.iter().chain(fm.disallowed_tools.iter()) {
        if tool.trim().is_empty() {
            anyhow::bail!(
                "blank tool name in allowed_tools/disallowed_tools of {}",
                path.display()
            );
        }
    }

    let permission_mode = match fm.permission_mode.as_str() {
        "auto_deny" => AgentPermissionMode::AutoDeny,
        "bubble" => AgentPermissionMode::Bubble,
        other => {
            anyhow::bail!(
                "unknown permission_mode '{other}' in {} (valid: bubble | auto_deny)",
                path.display()
            )
        }
    };

    let model = match fm.model {
        Some(m) if !m.trim().is_empty() => AgentModel::Fixed(m.trim().to_string()),
        _ => AgentModel::Inherit,
    };

    Ok(AgentDefinition {
        name: name.to_string(),
        description: description.to_string(),
        allowed_tools: fm
            .allowed_tools
            .into_iter()
            .map(|t| t.trim().to_string())
            .collect(),
        disallowed_tools: fm
            .disallowed_tools
            .into_iter()
            .map(|t| t.trim().to_string())
            .collect(),
        max_iterations: fm.max_iterations,
        model,
        system_prompt: AgentPrompt::Inline(body.to_string()),
        source: AgentSource::User,
        permission_mode,
        background_default: fm.background_default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_md(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn rejects_empty_name() {
        let f = write_md("---\nname: ''\ndescription: x\n---\nbody\n");
        let err = load_agent_from_markdown(f.path()).unwrap_err().to_string();
        assert!(err.contains("empty `name`"), "got: {err}");
    }

    #[test]
    fn rejects_whitespace_only_description() {
        let f = write_md("---\nname: foo\ndescription: '   '\n---\nbody\n");
        let err = load_agent_from_markdown(f.path()).unwrap_err().to_string();
        assert!(err.contains("empty `description`"), "got: {err}");
    }

    #[test]
    fn rejects_empty_system_prompt_body() {
        let f = write_md("---\nname: foo\ndescription: x\n---\n\n   \n");
        let err = load_agent_from_markdown(f.path()).unwrap_err().to_string();
        assert!(err.contains("empty system prompt body"), "got: {err}");
    }

    #[test]
    fn rejects_blank_tool_in_allowed_tools() {
        let f = write_md(
            "---\nname: foo\ndescription: x\nallowed_tools: ['read_workspace_file', '   ']\n---\nbody\n",
        );
        let err = load_agent_from_markdown(f.path()).unwrap_err().to_string();
        assert!(err.contains("blank tool name"), "got: {err}");
    }

    #[test]
    fn accepts_valid_definition() {
        let f = write_md(
            "---\nname: foo\ndescription: A test agent\nallowed_tools: ['Read']\n---\n\nYou are a test agent.\n",
        );
        let def = load_agent_from_markdown(f.path()).expect("valid def must parse");
        assert_eq!(def.name, "foo");
        assert_eq!(def.description, "A test agent");
        assert_eq!(def.allowed_tools, vec!["Read".to_string()]);
        assert!(
            matches!(def.system_prompt, AgentPrompt::Inline(ref s) if s == "You are a test agent.")
        );
    }
}
