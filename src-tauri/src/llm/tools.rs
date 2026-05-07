// ⚠️  COMPATIBILITY LAYER — not the source of truth for tool schemas.
// Use `runtime::tools::catalog::TOOL_CATALOG` for authoritative tool definitions.
// This module only exposes a legacy helper for callers that still expect
// `llm::tools::get_tool_definitions()`. New code should read the catalog
// directly instead of extending this wrapper.

//! Tool registry — compatibility layer delegating schema to ToolCatalog.
//!
//! Tool definitions are now sourced from `runtime::tools::catalog::TOOL_CATALOG`.
//! This module is a shim for legacy callers that still want a `Vec<ToolDefinition>`.
//!
//! Tool definitions are cached at first access via `LazyLock` to avoid
//! rebuilding the `Vec<ToolDefinition>` on every LLM request.

use crate::llm::streaming::ToolDefinition;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use std::sync::LazyLock;

/// Cached tool definitions — built once, reused on every call.
static ALL_TOOLS: LazyLock<Vec<ToolDefinition>> = LazyLock::new(build_tool_definitions);

/// Get all registered tool definitions for LLM context.
///
/// Returns a clone of the cached `Vec<ToolDefinition>`. This is cheap
/// because the definitions are only built once.
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    ALL_TOOLS.clone()
}

/// Build the full tool definitions (called once by LazyLock).
///
/// Delegates to `TOOL_CATALOG`.
fn build_tool_definitions() -> Vec<ToolDefinition> {
    TOOL_CATALOG
        .all_ids()
        .into_iter()
        .filter_map(|name| {
            TOOL_CATALOG.get_entry(&name).map(|entry| ToolDefinition {
                name: entry.definition.id.clone(),
                description: entry.definition.description.clone(),
                parameters: entry.json_schema.clone(),
            })
        })
        .collect()
}

/// Look up a tool definition by name.
///
/// Searches the cached tool definitions. Returns `None` if no tool with
/// the given name is registered.
pub fn get_tool_by_name(name: &str) -> Option<ToolDefinition> {
    ALL_TOOLS.iter().find(|t| t.name == name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_count() {
        let tools = get_tool_definitions();
        assert!(!tools.is_empty());
    }

    #[test]
    fn test_all_tools_have_names() {
        let tools = get_tool_definitions();
        for tool in &tools {
            assert!(!tool.name.is_empty(), "Tool name must not be empty");
            assert!(
                !tool.description.is_empty(),
                "Tool '{}' must have a description",
                tool.name
            );
        }
    }

    #[test]
    fn test_all_tools_have_valid_parameters() {
        let tools = get_tool_definitions();
        for tool in &tools {
            assert_eq!(
                tool.parameters["type"], "object",
                "Tool '{}' parameters must be an object schema",
                tool.name
            );
            assert!(
                tool.parameters.get("required").is_some(),
                "Tool '{}' must declare required fields",
                tool.name
            );
        }
    }

    #[test]
    fn test_unique_tool_names() {
        let tools = get_tool_definitions();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), tools.len(), "Tool names must be unique");
    }

    #[test]
    fn test_get_tool_by_name_found() {
        let tool = get_tool_by_name("read_workspace_file");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "read_workspace_file");
    }

    #[test]
    fn test_get_tool_by_name_not_found() {
        let tool = get_tool_by_name("nonexistent_tool");
        assert!(tool.is_none());
    }
}
