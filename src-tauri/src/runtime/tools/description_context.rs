//! [`ToolDescriptionContext`] — session-scoped context handed to
//! `RuntimeTool::definition()` so a tool can render a description that
//! depends on session state (which agents are registered, which MCP
//! servers are connected).
//!
//! Aligns with claude-code-best `tool.prompt({ tools, agents, ... })` —
//! the canonical approach: tool description is a function of session
//! context, not a compile-time constant.

use crate::runtime::agent::definition::AgentSource;

/// Compact summary of a registered/dispatchable agent — covers builtin
/// agents, user markdown agents, AND employees projected via
/// `EmployeeStore → AgentRegistry`. The `source` field tells callers
/// which variant this is.
#[derive(Clone, Debug)]
pub struct AgentDefSummary {
    pub name: String,
    /// First sentence of the agent's description.
    pub description: String,
    pub source: AgentSource,
}

/// Session-scoped context for rendering tool descriptions.
///
/// All fields are immutable snapshots — assemble once per turn at the
/// `build_visible_tool_defs` layer and pass to every tool's
/// `definition()` call.
#[derive(Clone, Debug, Default)]
pub struct ToolDescriptionContext {
    pub agents: Vec<AgentDefSummary>,
    /// Connected MCP server names (e.g. `["dingtalk", "sales"]`).  Tools
    /// can reference these to advertise capability availability.
    pub mcp_servers: Vec<String>,
}

impl ToolDescriptionContext {
    /// Empty context — used when no session info is available
    /// (registration phase, unit tests for tools that don't depend on
    /// session state).
    pub fn empty() -> Self {
        Self::default()
    }
}
