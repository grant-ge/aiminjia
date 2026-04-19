use std::sync::{Arc, Mutex};

use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::capability::FileStateCache;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::Value;
use tempfile::TempDir;

struct ReadFileStateCaptureTool {
    captured_caches: Arc<Mutex<Vec<Option<Arc<FileStateCache>>>>>,
}

#[async_trait]
impl RuntimeTool for ReadFileStateCaptureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "capture_read_file_state",
            "capture read_file_state capability",
        )
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let read_file_state = ctx
            .capability
            .as_ref()
            .and_then(|capability| capability.read_file_state.clone());
        self.captured_caches.lock().unwrap().push(read_file_state);
        Ok(ToolResult::new("capture_read_file_state", "ok", None))
    }
}

#[tokio::test]
async fn review_session_state_b1_query_engine_reuses_file_state_cache_across_turns() {
    let captured_caches: Arc<Mutex<Vec<Option<Arc<FileStateCache>>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ReadFileStateCaptureTool {
        captured_caches: captured_caches.clone(),
    }));

    let workspace = TempDir::new().unwrap();
    let query_engine = QueryEngine::with_dispatcher(dispatcher)
        .with_workspace_path(workspace.path().to_path_buf());

    let cache_a = query_engine.read_file_state();
    let cache_b = query_engine.read_file_state();
    assert!(
        Arc::ptr_eq(&cache_a, &cache_b),
        "QueryEngine::read_file_state() must return the same Arc<FileStateCache> instance"
    );

    let mapping_1 = IdentityMapping::from_legacy_conversation_id("conv-b1".to_string());
    let turn_1 = TurnState::new(mapping_1, RunId::new("run-b1-1"), "capture".to_string());
    let mapping_2 = IdentityMapping::from_legacy_conversation_id("conv-b1".to_string());
    let turn_2 = TurnState::new(mapping_2, RunId::new("run-b1-2"), "capture".to_string());
    let bus = RuntimeEventBus::new();

    query_engine
        .run_tool_call_with_bus(
            &turn_1,
            &bus,
            RuntimeToolCallRequest {
                tool_call_id: "tc-b1-001".to_string(),
                tool_name: "capture_read_file_state".to_string(),
                args: serde_json::json!({}),
                purpose: None,
            },
        )
        .await
        .expect("tool call should succeed");

    query_engine
        .run_tool_call_with_bus(
            &turn_2,
            &bus,
            RuntimeToolCallRequest {
                tool_call_id: "tc-b1-002".to_string(),
                tool_name: "capture_read_file_state".to_string(),
                args: serde_json::json!({}),
                purpose: None,
            },
        )
        .await
        .expect("second tool call should succeed");

    query_engine
        .run_tool_with_bus(&turn_2, &bus, "capture_read_file_state")
        .await
        .expect("legacy run_tool_with_bus path should also inject file state cache");

    let captured = captured_caches.lock().unwrap().clone();
    assert!(
        captured.len() >= 3,
        "expected three tool executions to capture read_file_state, got {}",
        captured.len()
    );
    let first = captured[0]
        .as_ref()
        .expect("first tool call should receive read_file_state cache");
    let second = captured[1]
        .as_ref()
        .expect("second tool call should receive read_file_state cache");
    let third = captured[2]
        .as_ref()
        .expect("run_tool_with_bus should also receive read_file_state cache");

    assert!(
        Arc::ptr_eq(&cache_a, first),
        "captured cache from first run_tool_call_with_bus must be the QueryEngine session cache"
    );
    assert!(
        Arc::ptr_eq(first, second),
        "two run_tool_call_with_bus executions must reuse the same session cache Arc"
    );
    assert!(
        Arc::ptr_eq(second, third),
        "run_tool_with_bus must reuse the same session cache Arc as run_tool_call_with_bus"
    );
}
