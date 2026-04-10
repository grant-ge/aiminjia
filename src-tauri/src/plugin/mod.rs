//! Plugin system — extensible Tools and Skills for the AI小家 agent.
//!
//! - **Tools**: MCP-style plugins (JSON Schema → execute → structured output)
//! - **Skills**: Vertical scenario packages (prompt + tool filter + workflow)
//!
//! Plugins can be:
//! - **Built-in** (Rust): compiled into the binary
//! - **Python scripts**: loaded from `{resource_dir}/plugins/`
//! - **Declarative** (TOML + Markdown): Skill definitions without code
//!
//! # Deprecation notice
//!
//! `ToolPlugin` and the associated `PluginContext` full-context pattern are
//! deprecated.  New tools should implement `crate::runtime::tools::RuntimeTool`.
//! The `#[allow(deprecated)]` below is intentional — this module is the
//! canonical public re-export point for the legacy surface.
#![allow(deprecated)]

pub mod builtin;
pub mod context;
pub mod declarative_skill;
pub mod manifest;
pub mod python_bridge;
pub mod registry;
pub mod skill_trait;
pub mod tool_trait;

pub use context::PluginContext;
pub use registry::{SkillInfo, SkillRegistry, ToolInfo, ToolRegistry};
pub use skill_trait::Skill;
pub use tool_trait::ToolPlugin;
