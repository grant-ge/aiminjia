//! Plugin system — extensible Tools and Skills for the AI小家 agent.
//!
//! - **Tools**: MCP-style plugins (JSON Schema → execute → structured output)
//! - **Skills**: Vertical scenario packages (prompt + tool filter + workflow)
//!
//! Plugins can be:
//! - **Built-in** (Rust): compiled into the binary
//! - **Python scripts**: loaded from `{resource_dir}/plugins/`
//! - **Declarative** (TOML + Markdown): Skill definitions without code

pub mod tool_trait;
pub mod skill_trait;
pub mod registry;
pub mod context;
pub mod manifest;
pub mod python_bridge;
pub mod declarative_skill;
pub mod builtin;

pub use tool_trait::ToolPlugin;
pub use skill_trait::Skill;
pub use registry::{ToolRegistry, SkillRegistry, ToolInfo, SkillInfo};
pub use context::PluginContext;
