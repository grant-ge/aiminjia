// ⚠️  COMPATIBILITY LAYER — not the source of truth for tool schemas.
// Use `runtime::tools::catalog::TOOL_CATALOG` for authoritative tool definitions.
// This file only provides `get_tool_definitions_for_step()` step-filter shims
// used by legacy analysis orchestration. New code must NOT add tools here.

//! Tool registry — compatibility layer delegating schema to ToolCatalog.
//!
//! Tool definitions are now sourced from `runtime::tools::catalog::TOOL_CATALOG`.
//! This module is a shim for legacy analysis orchestration that needs step-filtered
//! tool definitions. Do NOT add new tool schemas here.
//!
//! Tool definitions are cached at first access via `LazyLock` to avoid
//! rebuilding the `Vec<ToolDefinition>` on every LLM request.
#![allow(dead_code)]

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
/// Delegates to `TOOL_CATALOG` — no longer hardcodes schemas here.
/// The 10 step-analysis tools are sourced from the catalog by name.
fn build_tool_definitions() -> Vec<ToolDefinition> {
    // 当前分析步骤使用的 10 个工具名（保持与原 llm/tools.rs 一致）
    // 注意：第 10 个工具 catalog 中名为 "progress_update"
    let step_tool_names = [
        "web_search",
        "execute_python",
        "load_file",
        "generate_report",
        "generate_chart",
        "hypothesis_test",
        "detect_anomalies",
        "save_analysis_note",
        "export_data",
        "progress_update",
    ];
    step_tool_names
        .iter()
        .filter_map(|name| {
            TOOL_CATALOG.get_entry(name).map(|entry| ToolDefinition {
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

/// Get tool names that are relevant for a specific analysis step.
///
/// This helps the agent focus on the right tools during each phase of the
/// 5-step compensation analysis workflow.
pub fn get_tools_for_step(step: u32) -> Vec<String> {
    match step {
        // Step 0: Analysis direction confirmation
        0 => vec!["load_file".to_string(), "save_analysis_note".to_string()],
        // Step 1: Data cleaning and understanding
        1 => vec![
            "load_file".to_string(),
            "execute_python".to_string(),
            "save_analysis_note".to_string(),
            "progress_update".to_string(),
        ],
        // Step 2: Job normalization and job family construction
        2 => vec![
            "execute_python".to_string(),
            "web_search".to_string(),
            "save_analysis_note".to_string(),
            "progress_update".to_string(),
        ],
        // Step 3: Level framework and inference
        3 => vec![
            "execute_python".to_string(),
            "web_search".to_string(),
            "save_analysis_note".to_string(),
            "progress_update".to_string(),
        ],
        // Step 4: Fairness diagnosis (most tools needed)
        4 => vec![
            "execute_python".to_string(),
            "hypothesis_test".to_string(),
            "detect_anomalies".to_string(),
            "generate_chart".to_string(),
            "save_analysis_note".to_string(),
            "progress_update".to_string(),
        ],
        // Step 5: Action plan, reports, and export
        5 => vec![
            "execute_python".to_string(),
            "generate_report".to_string(),
            "generate_chart".to_string(),
            "export_data".to_string(),
            "progress_update".to_string(),
        ],
        // Unknown step: return all tool names
        _ => get_tool_definitions()
            .iter()
            .map(|t| t.name.clone())
            .collect(),
    }
}

/// Get full tool definitions relevant for a specific analysis step.
///
/// Unlike [`get_tools_for_step`] which returns just names, this returns
/// the complete [`ToolDefinition`] objects ready to pass to the gateway.
/// Uses the cached definitions to avoid rebuilding.
pub fn get_tool_definitions_for_step(step: u32) -> Vec<ToolDefinition> {
    let names = get_tools_for_step(step);
    ALL_TOOLS
        .iter()
        .filter(|t| names.contains(&t.name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_count() {
        let tools = get_tool_definitions();
        assert_eq!(tools.len(), 10);
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
        let tool = get_tool_by_name("web_search");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "web_search");
    }

    #[test]
    fn test_get_tool_by_name_not_found() {
        let tool = get_tool_by_name("nonexistent_tool");
        assert!(tool.is_none());
    }

    #[test]
    fn test_get_tools_for_step_1() {
        let tools = get_tools_for_step(1);
        assert!(tools.contains(&"load_file".to_string()));
        assert!(tools.contains(&"execute_python".to_string()));
        assert!(tools.contains(&"progress_update".to_string()));
        assert!(!tools.contains(&"web_search".to_string()));
    }

    #[test]
    fn test_get_tools_for_step_3() {
        let tools = get_tools_for_step(3);
        assert!(tools.contains(&"web_search".to_string()));
        assert!(tools.contains(&"execute_python".to_string()));
        assert!(!tools.contains(&"hypothesis_test".to_string()));
    }

    #[test]
    fn test_get_tools_for_step_4() {
        let tools = get_tools_for_step(4);
        assert!(tools.contains(&"execute_python".to_string()));
        assert!(tools.contains(&"hypothesis_test".to_string()));
        assert!(tools.contains(&"detect_anomalies".to_string()));
        assert!(tools.contains(&"generate_chart".to_string()));
    }

    #[test]
    fn test_get_tools_for_unknown_step() {
        let tools = get_tools_for_step(99);
        assert_eq!(tools.len(), 10, "Unknown step should return all tools");
    }

    #[test]
    fn test_get_tool_definitions_for_step() {
        let defs = get_tool_definitions_for_step(1);
        assert!(!defs.is_empty());
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"load_file"));
        assert!(names.contains(&"execute_python"));
        assert!(!names.contains(&"web_search"));
    }

    #[test]
    fn test_get_tool_definitions_for_step_all_valid() {
        for step in 1..=5 {
            let defs = get_tool_definitions_for_step(step);
            assert!(
                !defs.is_empty(),
                "Step {} should have tool definitions",
                step
            );
            for def in &defs {
                assert!(!def.name.is_empty());
                assert!(!def.description.is_empty());
            }
        }
    }
}
