//! Built-in tool plugins — thin wrappers delegating to llm/tool_executor/.

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
pub mod plan_update;
pub mod slides_gen;
pub mod memory_save;
pub mod memory_search;
pub mod memory_core;
pub mod memory_distill;
pub mod browse_navigate;
pub mod read_page_content;
pub mod page_execute_js;
pub mod browse_and_extract;
pub mod browse_data;
pub mod extract_table_data;

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
        Arc::new(plan_update::PlanUpdateTool),
        Arc::new(slides_gen::SlidesGenTool),
        Arc::new(memory_save::MemorySaveTool),
        Arc::new(memory_search::MemorySearchTool),
        Arc::new(memory_core::CoreMemoryTool),
        Arc::new(memory_distill::MemoryDistillTool),
        Arc::new(browse_navigate::BrowseNavigateTool),
        Arc::new(read_page_content::ReadPageContentTool),
        Arc::new(page_execute_js::PageExecuteJsTool),
        Arc::new(browse_and_extract::BrowseAndExtractTool),
        Arc::new(browse_data::BrowseDataTool),
        Arc::new(extract_table_data::ExtractTableDataTool),
    ];

    for tool in tools {
        registry.register(tool, "builtin").await;
    }
}
