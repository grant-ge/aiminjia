//! Tests for ToolSchemaFilter enum and build_visible_tool_defs filtering logic.

use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use app_lib::transport::tauri_commands::chat::chat_runtime_impl::{
    build_visible_tool_defs, ToolSchemaFilter,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

// ─── Minimal FakeRuntimeTool for test fixtures ───────────────────────────────

struct FakeRuntimeTool {
    id: &'static str,
}

#[async_trait]
impl RuntimeTool for FakeRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.id, "fake tool for schema filter tests")
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(self.id, format!("fake:{}", self.id), None))
    }
}

/// Build a ToolRegistry with one FakeRuntimeTool per name.
/// Async because `register_runtime` is async.
async fn make_test_registry_with_tools(names: &[&'static str]) -> ToolRegistry {
    let registry = ToolRegistry::new();
    for &id in names {
        registry
            .register_runtime(Arc::new(FakeRuntimeTool { id }))
            .await;
    }
    registry
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn daily_filter_excludes_tools_not_in_whitelist() {
    // "search_memory" is in DAILY_ALLOWED_TOOLS; "obscure_tool_not_in_daily" is not.
    let registry = make_test_registry_with_tools(&[
        "search_memory",
        "read_workspace_file",
        "obscure_tool_not_in_daily",
    ])
    .await;
    let defs = build_visible_tool_defs(&registry, true, ToolSchemaFilter::DailyWhitelist).await;
    let names: HashSet<_> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(
        names.contains("search_memory"),
        "search_memory is in DAILY_ALLOWED_TOOLS and should be included"
    );
    assert!(
        !names.contains("obscure_tool_not_in_daily"),
        "obscure_tool_not_in_daily is not in DAILY_ALLOWED_TOOLS and must be excluded"
    );
}

#[tokio::test]
async fn employee_filter_uses_employee_whitelist_only() {
    let registry = make_test_registry_with_tools(&[
        "search_memory",
        "browse_navigate",
        "extract_table_data",
    ])
    .await;
    let mut employee_set = HashSet::new();
    employee_set.insert("browse_navigate".to_string());
    employee_set.insert("extract_table_data".to_string());
    let defs = build_visible_tool_defs(
        &registry,
        true,
        ToolSchemaFilter::EmployeeWhitelist(employee_set),
    )
    .await;
    let names: HashSet<_> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(
        !names.contains("search_memory"),
        "employee path must NOT leak daily-only tools"
    );
    assert!(
        names.contains("browse_navigate"),
        "browse_navigate was in employee whitelist and should be included"
    );
    assert!(
        names.contains("extract_table_data"),
        "extract_table_data was in employee whitelist and should be included"
    );
}

#[tokio::test]
async fn no_filter_returns_full_set() {
    // ToolSchemaFilter::None means "no whitelist filter"; the result should
    // include the registered fake tools. (TOOL_CATALOG also exposes other
    // request-scoped tools globally, so we only assert presence of our names,
    // not exact length.)
    let registry = make_test_registry_with_tools(&["fake_no_filter_a", "fake_no_filter_b", "fake_no_filter_c"]).await;
    let defs = build_visible_tool_defs(&registry, true, ToolSchemaFilter::None).await;
    let names: HashSet<_> = defs.iter().map(|d| d.name.as_str()).collect();
    for expected in ["fake_no_filter_a", "fake_no_filter_b", "fake_no_filter_c"] {
        assert!(
            names.contains(expected),
            "ToolSchemaFilter::None should expose registered tool '{}', got {:?}",
            expected,
            names
        );
    }
}
