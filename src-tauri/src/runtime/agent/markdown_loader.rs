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
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
    let parsed = matter.parse(&raw);
    let data = parsed
        .data
        .ok_or_else(|| anyhow::anyhow!("missing YAML frontmatter in {}", path.display()))?;
    let fm: AgentFrontmatter = data
        .deserialize()
        .with_context(|| format!("parse frontmatter {}", path.display()))?;

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
        Some(m) if !m.is_empty() => AgentModel::Fixed(m),
        _ => AgentModel::Inherit,
    };

    Ok(AgentDefinition {
        name: fm.name,
        description: fm.description,
        allowed_tools: fm.allowed_tools,
        disallowed_tools: fm.disallowed_tools,
        max_iterations: fm.max_iterations,
        model,
        system_prompt: AgentPrompt::Inline(parsed.content.trim().to_string()),
        source: AgentSource::User,
        permission_mode,
        background_default: fm.background_default,
    })
}
