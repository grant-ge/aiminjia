use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_lib::runtime::hooks::{HookConfig, HookEvent, HookRegistry, HookRunner};
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::TempDir;

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
        ToolDefinition::new(&self.name, "recording tool")
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

fn make_pre_hook(command: &str) -> HookConfig {
    HookConfig {
        event: HookEvent::PreToolUse,
        command: command.to_string(),
        tool_filter: None,
        timeout_secs: Some(1),
    }
}

#[tokio::test]
async fn review_hook_timeout_stays_non_blocking() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "sleep 5".to_string(),
        tool_filter: None,
        timeout_secs: Some(1),
    };

    let result = runner
        .run_hook(&config, "bash_tool", &json!({}))
        .await
        .unwrap();

    assert!(matches!(
        result.decision,
        app_lib::runtime::hooks::HookDecision::Allow
    ));
}

#[tokio::test]
async fn review_hook_exec_error_stays_non_blocking() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "__lotus_missing_hook_command__".to_string(),
        tool_filter: None,
        timeout_secs: Some(1),
    };

    let result = runner
        .run_hook(&config, "bash_tool", &json!({}))
        .await
        .unwrap();

    assert!(matches!(
        result.decision,
        app_lib::runtime::hooks::HookDecision::Allow
    ));
}

#[tokio::test]
async fn review_hook_updated_input_rejects_unknown_fields() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(make_pre_hook(
        r#"printf '{"behavior":"allow","updatedInput":{"command":"echo safe","unknown_field":123}}'"#,
    ));

    let ctx =
        ToolExecutionContext::for_test("conv-hook-unknown", "run-hook-unknown", "tc-hook-unknown")
            .with_capability(Arc::new(CapabilityContext::with_workspace(
                std::env::temp_dir(),
                "hook-review",
            )))
            .with_hook_registry(Arc::new(registry));

    dispatcher
        .dispatch("bash_tool", json!({"command": "dangerous"}), ctx)
        .await
        .unwrap();

    let inputs = received.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0], json!({"command": "dangerous"}));
}

#[tokio::test]
async fn review_hook_updated_input_accepts_known_fields() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(make_pre_hook(
        r#"printf '{"behavior":"allow","updatedInput":{"command":"echo safe"}}'"#,
    ));

    let ctx = ToolExecutionContext::for_test("conv-hook-known", "run-hook-known", "tc-hook-known")
        .with_capability(Arc::new(CapabilityContext::with_workspace(
            std::env::temp_dir(),
            "hook-review",
        )))
        .with_hook_registry(Arc::new(registry));

    dispatcher
        .dispatch("bash_tool", json!({"command": "dangerous"}), ctx)
        .await
        .unwrap();

    let inputs = received.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0], json!({"command": "echo safe"}));
}

#[tokio::test]
async fn review_hook_uses_workspace_root_as_cwd_when_provided() {
    let runner = HookRunner::new();
    let workspace = TempDir::new().unwrap();
    let workspace_root = workspace.path().to_path_buf();
    let config = make_pre_hook(
        r#"cwd=$(pwd); printf '{"behavior":"allow","updatedInput":{"cwd":"%s"}}' "$cwd""#,
    );

    let outcome = runner
        .run_hook_in_workspace(
            &config,
            "bash_tool",
            &json!({"cwd": ""}),
            Some(workspace_root.as_path()),
        )
        .await
        .unwrap();

    let updated_cwd = outcome
        .updated_input
        .as_ref()
        .and_then(|value| value.get("cwd"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    let expected_cwd = workspace_root.canonicalize().unwrap();

    assert_eq!(updated_cwd.as_deref(), Some(expected_cwd.as_path()));
}
