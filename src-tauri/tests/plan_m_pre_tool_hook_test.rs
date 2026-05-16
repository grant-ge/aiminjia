use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use app_lib::runtime::tools::dispatcher::RuntimeTool;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext,
    ToolResult,
};

struct RecordingTool {
    name: String,
    received_inputs: Arc<Mutex<Vec<Value>>>,
}

#[async_trait]
impl RuntimeTool for RecordingTool {
    fn id(&self) -> &str {

        &self.name

    }


    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(&self.name, "recording")
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.received_inputs.lock().unwrap().push(input);
        Ok(ToolResult::new(&self.name, "ok", None))
    }
}

#[tokio::test]
async fn pre_tool_hook_deny_prevents_execution() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\",\"message\":\"blocked by hook\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher
        .dispatch("bash_tool", json!({"command": "rm -rf /"}), ctx)
        .await;

    assert!(result.is_err());
    let err_str = match result {
        Err(err) => err.to_string(),
        Ok(_) => panic!("expected hook to deny"),
    };
    assert!(err_str.contains("blocked by hook"));
    assert_eq!(received.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn pre_tool_hook_allow_permits_execution() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher
        .dispatch("bash_tool", json!({"command": "ls"}), ctx)
        .await;
    assert!(result.is_ok());
    assert_eq!(received.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn pre_tool_hook_updated_input_modifies_args() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command:
            r#"printf '{\"behavior\":\"allow\",\"updatedInput\":{\"command\":\"echo safe\"}}'"#
                .to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    dispatcher
        .dispatch("bash_tool", json!({"command": "dangerous"}), ctx)
        .await
        .unwrap();

    let inputs = received.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].get("command").and_then(serde_json::Value::as_str),
        Some("echo safe")
    );
}

#[tokio::test]
async fn no_hook_registry_executes_normally() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");

    let result = dispatcher
        .dispatch("bash_tool", json!({"command": "ls"}), ctx)
        .await;
    assert!(result.is_ok());
    assert_eq!(received.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn pre_tool_hook_tool_filter_only_affects_target() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "Write".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\"}'".to_string(),
        tool_filter: Some("bash_tool".to_string()),
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher
        .dispatch("Write", json!({"file_path": "/tmp/x"}), ctx)
        .await;
    assert!(result.is_ok());
    assert_eq!(received.lock().unwrap().len(), 1);
}
