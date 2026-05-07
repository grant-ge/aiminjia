use std::sync::Arc;

use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::tools::catalog::TOOL_CATALOG;
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

struct MockRuntimeTool {
    def: ToolDefinition,
}

#[async_trait]
impl RuntimeTool for MockRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        self.def.clone()
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(self.def.id.clone(), "ok", None))
    }
}

#[tokio::test]
async fn register_runtime_syncs_to_catalog() {
    let registry = ToolRegistry::new();
    let tool_id = format!("dynamic_registry_sync_{}", uuid::Uuid::new_v4());

    registry
        .register_runtime(Arc::new(MockRuntimeTool {
            def: ToolDefinition::new(&tool_id, "Test tool sync"),
        }))
        .await;

    let entry = TOOL_CATALOG
        .get_entry(&tool_id)
        .expect("register_runtime should sync runtime tool into TOOL_CATALOG");
    assert_eq!(entry.definition.id, tool_id);
    assert_eq!(
        entry.json_schema,
        json!({"type": "object", "properties": {}})
    );
}

#[tokio::test]
async fn register_runtime_does_not_overwrite_builtin_schema() {
    let registry = ToolRegistry::new();
    let original_schema = TOOL_CATALOG
        .get_entry("bash")
        .expect("execute_python must exist in TOOL_CATALOG")
        .json_schema;

    registry
        .register_runtime(Arc::new(MockRuntimeTool {
            def: ToolDefinition::new("bash", "Override attempt"),
        }))
        .await;

    let new_schema = TOOL_CATALOG
        .get_entry("bash")
        .expect("execute_python should remain in TOOL_CATALOG")
        .json_schema;
    assert_eq!(
        original_schema, new_schema,
        "builtin schema should not be overwritten"
    );
}
