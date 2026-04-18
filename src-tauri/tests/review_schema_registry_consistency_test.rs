use std::sync::Arc;

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::tools::catalog::TOOL_CATALOG;
use app_lib::runtime::tools::{RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

struct MissingCatalogRuntimeTool;

#[async_trait]
impl RuntimeTool for MissingCatalogRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "missing_catalog_runtime_tool",
            "Runtime tool intentionally missing from TOOL_CATALOG",
        )
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(
            "missing_catalog_runtime_tool",
            "ok",
            None,
        ))
    }
}

#[tokio::test]
async fn review_validate_catalog_consistency_panics_for_runtime_tool_without_catalog_entry() {
    let registry = ToolRegistry::new();
    registry
        .register_runtime(Arc::new(MissingCatalogRuntimeTool))
        .await;

    let handle = tokio::spawn(async move {
        registry.validate_catalog_consistency().await;
    });
    let join_err = handle
        .await
        .expect_err("missing catalog entry should trigger panic");
    assert!(join_err.is_panic(), "expected panic, got: {join_err}");
}

#[tokio::test]
async fn review_register_builtin_tools_preserves_catalog_consistency() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    registry.validate_catalog_consistency().await;
}

#[test]
fn review_registered_workspace_and_request_scoped_tools_all_in_catalog() {
    for id in &[
        "list_directory",
        "read_workspace_file",
        "search_files",
        "get_file_info",
        "write_file",
        "edit_file",
        "bash",
        "grep_content",
        "web_search",
        "load_file",
        "browse_data",
        "execute_python",
        "generate_report",
        "generate_chart",
    ] {
        assert!(
            TOOL_CATALOG.get_entry(id).is_some(),
            "TOOL_CATALOG missing entry for '{id}'",
        );
    }
}
