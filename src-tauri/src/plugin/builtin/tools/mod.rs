//! Built-in tool plugins — thin wrappers delegating to llm/tool_executor/.
//!
//! All tools in this module implement the deprecated `ToolPlugin` trait and
//! are bridged into the runtime via `LegacyToolAdapter`.  They should be
//! migrated to `RuntimeTool` incrementally — see `echo_runtime.rs` for the
//! reference implementation of the new pattern.
#![allow(deprecated)]

pub mod analysis_note;
pub mod anomaly_detect;
pub mod browse_and_extract;
pub mod browse_data;
pub mod browse_navigate;
pub mod chart_gen;
pub mod data_export;
pub mod echo_runtime;
pub mod extract_table_data;
pub mod extract_with_pagination;
pub mod file_load;
pub mod hypothesis_test;
pub mod memory_core;
pub mod memory_distill;
pub mod memory_save;
pub mod memory_search;
pub mod page_execute_js;
pub mod plan_update;
pub mod progress_update;
pub mod python_exec;
pub mod read_page_content;
pub mod report_gen;
pub mod slides_gen;
pub mod web_search;

use crate::plugin::ToolRegistry;
use std::sync::Arc;

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
        Arc::new(extract_with_pagination::ExtractWithPaginationTool),
    ];

    for tool in tools {
        registry.register(tool, "builtin").await;
    }

    // Register runtime-native workspace tools (take precedence over legacy ToolPlugin).
    // These four tools have no constructor deps and can be registered unconditionally.
    use crate::runtime::tools::builtin::workspace::{
        EditFileRuntimeTool, GetFileInfoRuntimeTool, ListDirectoryRuntimeTool,
        ReadWorkspaceFileRuntimeTool, SearchFilesRuntimeTool, WriteFileRuntimeTool,
    };
    use crate::runtime::tools::builtin::bash::BashTool;
    use crate::runtime::tools::builtin::grep::GrepContentTool;
    registry
        .register_runtime(Arc::new(ListDirectoryRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(ReadWorkspaceFileRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(SearchFilesRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(GetFileInfoRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(WriteFileRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(EditFileRuntimeTool))
        .await;
    registry.register_runtime(Arc::new(BashTool)).await;
    registry.register_runtime(Arc::new(GrepContentTool)).await;
}
