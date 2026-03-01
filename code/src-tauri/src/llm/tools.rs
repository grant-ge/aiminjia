//! Tool registry — definitions and dispatch for agent tools.
//!
//! Each tool is defined with a JSON Schema for parameters. The LLM
//! receives these schemas and can call any tool during a conversation.
//! Tools cover the full HR compensation analysis workflow: data upload,
//! statistical testing, anomaly detection, chart generation, and report
//! export.
//!
//! Tool definitions are cached at first access via `LazyLock` to avoid
//! rebuilding the `Vec<ToolDefinition>` on every LLM request.
#![allow(dead_code)]

use std::sync::LazyLock;
use serde_json::json;
use crate::llm::streaming::ToolDefinition;

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
fn build_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        // ─── 1. Web Search ───────────────────────────────────
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web for information. Use for market salary data, \
                industry benchmarks, regulations, and company information."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum results to return (default 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        },
        // ─── 2. Python Code Execution ────────────────────────
        ToolDefinition {
            name: "execute_python".to_string(),
            description: "Execute Python code for data analysis, statistical computation, \
                and file processing. Has access to pandas, numpy, scipy, openpyxl. \
                Working directory contains uploaded files."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Python code to execute"
                    },
                    "purpose": {
                        "type": "string",
                        "description": "Brief description of what this code does"
                    }
                },
                "required": ["code"]
            }),
        },
        // ─── 3. File Analysis ────────────────────────────────
        ToolDefinition {
            name: "analyze_file".to_string(),
            description: "Parse and analyze an uploaded file. Returns column names, \
                data types, row count, and sample data."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_id": {
                        "type": "string",
                        "description": "ID of the uploaded file"
                    }
                },
                "required": ["file_id"]
            }),
        },
        // ─── 4. Report Generation ────────────────────────────
        ToolDefinition {
            name: "generate_report".to_string(),
            description: "Generate a professional analysis report as a downloadable HTML file. \
                Supports rich content: text with markdown, structured tables, metric cards, \
                bullet lists, and highlighted callouts. Use this for the final comprehensive \
                report at the end of analysis."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Report title (e.g. '薪酬公平性分析报告')"
                    },
                    "sections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "heading": {
                                    "type": "string",
                                    "description": "Section heading"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "Text content (supports markdown: **bold**, `code`, tables, lists, headers)"
                                },
                                "metrics": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": { "type": "string" },
                                            "value": { "type": "string" },
                                            "subtitle": { "type": "string" },
                                            "state": { "type": "string", "enum": ["good", "warn", "bad", "neutral"] }
                                        },
                                        "required": ["label", "value"]
                                    },
                                    "description": "Metric cards displayed as a grid"
                                },
                                "table": {
                                    "type": "object",
                                    "properties": {
                                        "title": { "type": "string" },
                                        "columns": {
                                            "type": "array",
                                            "items": { "type": "string" }
                                        },
                                        "rows": {
                                            "type": "array",
                                            "items": { "type": "array", "items": { "type": "string" } }
                                        }
                                    },
                                    "description": "Structured data table"
                                },
                                "items": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Bullet list items"
                                },
                                "highlight": {
                                    "type": "string",
                                    "description": "Highlighted callout text for key findings"
                                }
                            },
                            "required": ["heading"]
                        },
                        "description": "Report sections. Each section must have a heading and can include any combination of content, metrics, table, items, highlight."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["html", "markdown"],
                        "default": "html"
                    }
                },
                "required": ["title", "sections"]
            }),
        },
        // ─── 5. Chart Generation ─────────────────────────────
        ToolDefinition {
            name: "generate_chart".to_string(),
            description: "Generate a data visualization chart. Supports bar, line, scatter, \
                box, and heatmap types."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chart_type": {
                        "type": "string",
                        "enum": ["bar", "line", "scatter", "box", "heatmap"]
                    },
                    "title": { "type": "string" },
                    "data": {
                        "type": "object",
                        "description": "Chart data with labels and values"
                    },
                    "options": {
                        "type": "object",
                        "description": "Additional chart configuration"
                    }
                },
                "required": ["chart_type", "title", "data"]
            }),
        },
        // ─── 6. Statistical Hypothesis Testing ───────────────
        ToolDefinition {
            name: "hypothesis_test".to_string(),
            description: "Run a statistical hypothesis test on compensation data. Supports \
                t-test, ANOVA, chi-square, Mann-Whitney, and regression analysis."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "test_type": {
                        "type": "string",
                        "enum": ["t_test", "anova", "chi_square", "regression", "mann_whitney"]
                    },
                    "groups": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Column names to compare"
                    },
                    "data_source": {
                        "type": "string",
                        "description": "File ID or inline data reference"
                    },
                    "significance_level": {
                        "type": "number",
                        "default": 0.05
                    }
                },
                "required": ["test_type", "groups"]
            }),
        },
        // ─── 7. Anomaly Detection ────────────────────────────
        ToolDefinition {
            name: "detect_anomalies".to_string(),
            description: "Detect outliers and anomalies in compensation data using statistical \
                methods (Z-score, IQR, or Grubbs test)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "column": {
                        "type": "string",
                        "description": "Column name to analyze"
                    },
                    "method": {
                        "type": "string",
                        "enum": ["zscore", "iqr", "grubbs"],
                        "default": "zscore"
                    },
                    "threshold": {
                        "type": "number",
                        "description": "Detection threshold (default from settings)"
                    },
                    "group_by": {
                        "type": "string",
                        "description": "Optional grouping column"
                    }
                },
                "required": ["column"]
            }),
        },
        // ─── 8. Save Analysis Note ───────────────────────────
        ToolDefinition {
            name: "save_analysis_note".to_string(),
            description: "Save an intermediate analysis finding or decision to the conversation \
                context. This helps maintain continuity across the 5-step analysis."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Note identifier (e.g. 'job_normalization_confirmed')"
                    },
                    "content": {
                        "type": "string",
                        "description": "The analysis note content"
                    },
                    "step": {
                        "type": "integer",
                        "description": "Which analysis step (1-5) this belongs to"
                    }
                },
                "required": ["key", "content"]
            }),
        },
        // ─── 9. Export Data ──────────────────────────────────
        ToolDefinition {
            name: "export_data".to_string(),
            description: "Export processed data to a file format (CSV, Excel, JSON).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "data": {
                        "type": "object",
                        "description": "Data to export"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["csv", "excel", "json"]
                    },
                    "filename": {
                        "type": "string",
                        "description": "Output filename"
                    }
                },
                "required": ["data", "format", "filename"]
            }),
        },
        // ─── 10. Update Analysis Progress ────────────────────
        ToolDefinition {
            name: "update_progress".to_string(),
            description: "Update the analysis progress indicator. Call this when transitioning \
                between analysis steps."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "current_step": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 5
                    },
                    "step_status": {
                        "type": "string",
                        "enum": ["active", "completed", "error"]
                    },
                    "summary": {
                        "type": "string",
                        "description": "Brief summary of step progress"
                    }
                },
                "required": ["current_step", "step_status"]
            }),
        },
    ]
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
        0 => vec![
            "analyze_file".to_string(),
            "save_analysis_note".to_string(),
        ],
        // Step 1: Data cleaning and understanding
        1 => vec![
            "analyze_file".to_string(),
            "execute_python".to_string(),
            "save_analysis_note".to_string(),
            "update_progress".to_string(),
        ],
        // Step 2: Job normalization and job family construction
        2 => vec![
            "execute_python".to_string(),
            "web_search".to_string(),
            "save_analysis_note".to_string(),
            "update_progress".to_string(),
        ],
        // Step 3: Level framework and inference
        3 => vec![
            "execute_python".to_string(),
            "web_search".to_string(),
            "save_analysis_note".to_string(),
            "update_progress".to_string(),
        ],
        // Step 4: Fairness diagnosis (most tools needed)
        4 => vec![
            "execute_python".to_string(),
            "hypothesis_test".to_string(),
            "detect_anomalies".to_string(),
            "generate_chart".to_string(),
            "save_analysis_note".to_string(),
            "update_progress".to_string(),
        ],
        // Step 5: Action plan, reports, and export
        5 => vec![
            "execute_python".to_string(),
            "generate_report".to_string(),
            "generate_chart".to_string(),
            "export_data".to_string(),
            "update_progress".to_string(),
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
        assert!(tools.contains(&"analyze_file".to_string()));
        assert!(tools.contains(&"execute_python".to_string()));
        assert!(tools.contains(&"update_progress".to_string()));
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
        assert!(names.contains(&"analyze_file"));
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
