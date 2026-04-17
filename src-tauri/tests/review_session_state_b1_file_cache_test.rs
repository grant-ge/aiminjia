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
    captured_cache: Arc<Mutex<Option<Arc<FileStateCache>>>>,
}

#[async_trait]
impl RuntimeTool for ReadFileStateCaptureTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("capture_read_file_state", "capture read_file_state capability")
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
        *self.captured_cache.lock().unwrap() = read_file_state;
        Ok(ToolResult::new("capture_read_file_state", "ok", None))
    }
}

#[tokio::test]
async fn review_session_state_b1_query_engine_reuses_file_state_cache_across_turns() {
    let captured_cache: Arc<Mutex<Option<Arc<FileStateCache>>>> = Arc::new(Mutex::new(None));
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ReadFileStateCaptureTool {
        captured_cache: captured_cache.clone(),
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

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-b1".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-b1"), "capture".to_string());
    let bus = RuntimeEventBus::new();

    query_engine
        .run_tool_call_with_bus(
            &turn,
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

    let captured = captured_cache.lock().unwrap().clone();
    assert!(
        captured.is_some(),
        "CapabilityContext.read_file_state should be injected for tool execution"
    );
}
