//! Built-in tool plugins — thin wrappers delegating to llm/tool_executor/.
//!
//! # DEPRECATED: ToolPlugin 路径
//!
//! 本模块中所有实现 `ToolPlugin` trait 的工具均为**过期工具**，不与 claude-code-best
//! 架构对齐（走 `registry.execute() → PluginContext` 路径，无 RuntimeTool dispatcher）。
//!
//! **待删除工具列表（下方注释掉的 register 调用）：**
//! - `web_search` — 已有 `WebSearchRuntimeTool`（builtin/network.rs）替代
//! - `python_exec` — 已有 `ExecutePythonRuntimeTool`（builtin/python.rs）替代
//! - `file_load` — 已有 `LoadFileRuntimeTool`（builtin/file.rs）替代
//! - `report_gen` — 已有 `GenerateReportRuntimeTool`（builtin/report.rs）替代
//! - `chart_gen` — 已有 `GenerateChartRuntimeTool`（builtin/chart.rs）替代
//! - `browse_navigate` — 已有 `BrowseNavigateRuntimeTool`（builtin/browser.rs）替代
//! - `read_page_content` — 已有 `ReadPageContentRuntimeTool`（builtin/browser.rs）替代
//! - `page_execute_js` — 已有 `PageExecuteJsRuntimeTool`（builtin/browser.rs）替代
//! - `extract_table_data` — 已有 `ExtractTableDataRuntimeTool`（builtin/browser.rs）替代
//! - `extract_with_pagination` — 已有 `ExtractWithPaginationRuntimeTool`（builtin/browser.rs）替代
//! - `hypothesis_test` — 无 RuntimeTool 替代，待删除
//! - `anomaly_detect` — 无 RuntimeTool 替代，待删除
//! - `analysis_note` — 无 RuntimeTool 替代，待删除
//! - `data_export` — 无 RuntimeTool 替代，待删除
//! - `progress_update` — 无 RuntimeTool 替代，待删除
//! - `plan_update` — 无 RuntimeTool 替代，待删除
//! - `slides_gen` — 无 RuntimeTool 替代，待删除
//! - `browse_and_extract` — 复合工具，无 RuntimeTool 替代，待删除
//!
//! 见 `plugin/registry.rs::execute()` 的 `#[deprecated]` 注解。
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
pub mod page_execute_js;
pub mod plan_update;
pub mod progress_update;
pub mod python_exec;
pub mod read_page_content;
pub mod report_gen;
pub mod slides_gen;
pub mod web_search;
// DingTalk AI Table integration (Phase 13)
pub mod dingtalk;

use crate::plugin::ToolRegistry;
use std::sync::Arc;

/// Register all built-in tools.
///
/// DEPRECATED ToolPlugin 注册已全部关闭——这些工具走 `registry.execute() → PluginContext`
/// 路径，不与 claude-code-best 架构对齐，等待删除。
///
/// 仅保留 RuntimeTool 注册（走 `ToolDispatcher` 正确路径）。
pub async fn register_builtin_tools(registry: &ToolRegistry) {
    // ── DEPRECATED: ToolPlugin 工具注册已关闭 ──────────────────────────────
    // 以下工具均为过期工具，不再注入 agent。如需临时恢复，取消注释并说明原因。
    //
    // let _deprecated_tools: Vec<Arc<dyn crate::plugin::ToolPlugin>> = vec![
    //     Arc::new(web_search::WebSearchTool),          // 替代: WebSearchRuntimeTool
    //     Arc::new(python_exec::PythonExecTool),        // 替代: ExecutePythonRuntimeTool
    //     Arc::new(file_load::FileLoadTool),            // 替代: LoadFileRuntimeTool
    //     Arc::new(report_gen::ReportGenTool),          // 替代: GenerateReportRuntimeTool
    //     Arc::new(chart_gen::ChartGenTool),            // 替代: GenerateChartRuntimeTool
    //     Arc::new(hypothesis_test::HypothesisTestTool),   // 待删除
    //     Arc::new(anomaly_detect::AnomalyDetectTool),     // 待删除
    //     Arc::new(analysis_note::AnalysisNoteTool),       // 待删除
    //     Arc::new(data_export::DataExportTool),           // 待删除
    //     Arc::new(progress_update::ProgressUpdateTool),   // 待删除
    //     Arc::new(plan_update::PlanUpdateTool),           // 待删除
    //     Arc::new(slides_gen::SlidesGenTool),             // 待删除
    //     Arc::new(browse_navigate::BrowseNavigateTool),   // 替代: BrowseNavigateRuntimeTool
    //     Arc::new(read_page_content::ReadPageContentTool),// 替代: ReadPageContentRuntimeTool
    //     Arc::new(page_execute_js::PageExecuteJsTool),    // 替代: PageExecuteJsRuntimeTool
    //     Arc::new(browse_and_extract::BrowseAndExtractTool), // 待删除
    //     Arc::new(extract_table_data::ExtractTableDataTool),     // 替代: ExtractTableDataRuntimeTool
    //     Arc::new(extract_with_pagination::ExtractWithPaginationTool), // 替代: ExtractWithPaginationRuntimeTool
    // ];

    // ── RuntimeTool 注册（正确路径，走 ToolDispatcher）─────────────────────
    use crate::runtime::tools::builtin::ask_user_question::AskUserQuestionRuntimeTool;
    #[cfg(not(windows))]
    use crate::runtime::tools::builtin::bash::BashTool;
    use crate::runtime::tools::builtin::grep::GrepContentTool;
    #[cfg(windows)]
    use crate::runtime::tools::builtin::powershell::PowerShellTool;
    use crate::runtime::tools::builtin::task_tools::{
        TaskCreateRuntimeTool, TaskListRuntimeTool, TaskUpdateRuntimeTool,
    };
    use crate::runtime::tools::builtin::workspace::{
        EditFileRuntimeTool, GetFileInfoRuntimeTool, ListDirectoryRuntimeTool,
        ReadWorkspaceFileRuntimeTool, SearchFilesRuntimeTool, WriteFileRuntimeTool,
    };
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
    #[cfg(not(windows))]
    registry.register_runtime(Arc::new(BashTool)).await;
    #[cfg(windows)]
    registry.register_runtime(Arc::new(PowerShellTool)).await;
    registry.register_runtime(Arc::new(GrepContentTool)).await;
    registry
        .register_runtime(Arc::new(AskUserQuestionRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(TaskCreateRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(TaskUpdateRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(TaskListRuntimeTool))
        .await;
    registry.validate_catalog_consistency().await;
}
