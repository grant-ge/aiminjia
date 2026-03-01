//! Built-in tool plugins — migrated from llm/tool_executor.rs.

pub mod web_search;
pub mod python_exec;
pub mod file_load;
pub mod report_gen;
pub mod chart_gen;
pub mod hypothesis_test;
pub mod anomaly_detect;
pub mod analysis_note;
pub mod data_export;
pub mod progress_update;

use std::sync::Arc;
use crate::plugin::ToolRegistry;

/// Register all built-in tools.
pub async fn register_builtin_tools(registry: &ToolRegistry) {
    let tools: Vec<Arc<dyn crate::plugin::ToolPlugin>> = vec![
        Arc::new(web_search::WebSearchTool),
        Arc::new(python_exec::PythonExecTool),
        Arc::new(file_load::FileLoadTool),
        Arc::new(report_gen::ReportGenTool),
        Arc::new(chart_gen::ChartGenTool),
        Arc::new(hypothesis_test::HypothesisTestTool),
        Arc::new(anomaly_detect::AnomalyDetectTool),
        Arc::new(analysis_note::AnalysisNoteTool),
        Arc::new(data_export::DataExportTool),
        Arc::new(progress_update::ProgressUpdateTool),
    ];

    for tool in tools {
        registry.register(tool, "builtin").await;
    }
}
