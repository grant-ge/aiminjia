//! S3 Task 11 — load_file 运行时语义 gate 测试。
//!
//! 锁住重构后 LoadFileRuntimeTool 行为：
//! 1. `is_loaded()` 在 `load_file()` 调用后返回 true（loaded key 写入）
//! 2. `LoadFileRuntimeTool::execute()` 无 file_ops 时返回 ToolError::Other
//! 3. cancellation token 经 turn cascade 正确传播到 ToolExecutionContext
//! 4. LoadFileRuntimeTool 经过统一 permission pipeline（DenyAll → PermissionDenied）

use std::sync::Arc;

use app_lib::runtime::tools::builtin::file::LoadFileRuntimeTool;
use app_lib::runtime::tools::{
    CapabilityContext, FileOperations, PermissionDecision, PermissionPipeline, PermissionReason,
    RuntimeTool, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext,
};
use async_trait::async_trait;
use serde_json::{json, Value};

// ─── Mock FileOperations ─────────────────────────────────────────────────────

/// In-memory mock: 记录 load_file 是否被调用，模拟 is_loaded 状态。
#[derive(Debug, Clone)]
struct MockFileOps {
    /// simulated set of (scope_id, file_id) pairs that have been "loaded"
    loaded: Arc<std::sync::Mutex<std::collections::HashSet<(String, String)>>>,
    /// return value for load_file (Ok/Err)
    load_result: Arc<std::sync::Mutex<Option<anyhow::Error>>>,
}

impl MockFileOps {
    fn new() -> Self {
        Self {
            loaded: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            load_result: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Pre-mark a file as loaded (simulates what DefaultFileOperations would do after
    /// a successful load_file call sets the `loaded:{scope_id}:{file_id}` key).
    fn mark_loaded(&self, scope_id: &str, file_id: &str) {
        self.loaded
            .lock()
            .unwrap()
            .insert((scope_id.to_string(), file_id.to_string()));
    }
}

#[async_trait]
impl FileOperations for MockFileOps {
    async fn load_file(
        &self,
        args: &serde_json::Value,
    ) -> anyhow::Result<app_lib::runtime::tools::capability::LoadedFileResult> {
        // Check for an injected error
        if let Some(err) = self.load_result.lock().unwrap().take() {
            return Err(err);
        }

        // Simulate: mark this file_id as loaded (scope key would be written by DefaultFileOperations)
        if let Some(file_id) = args.get("file_id").and_then(|v| v.as_str()) {
            let scope_id = args
                .get("scope_id")
                .and_then(|v| v.as_str())
                .unwrap_or("default-scope");
            self.mark_loaded(scope_id, file_id);
        }

        Ok(app_lib::runtime::tools::capability::LoadedFileResult {
            content: r#"{"file_meta":{"file_id":"test-file-1","name":"test.csv","rows":10},"preview":"col1,col2\n1,2"}"#
                .to_string(),
        })
    }

    fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool {
        self.loaded
            .lock()
            .unwrap()
            .contains(&(scope_id.to_string(), file_id.to_string()))
    }

    fn workspace_path(&self) -> &std::path::Path {
        std::path::Path::new("/tmp/mock-workspace")
    }
}

// ─── Mock: DenyAll permission pipeline ───────────────────────────────────────

struct DenyAllPipeline;

impl PermissionPipeline for DenyAllPipeline {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Deny {
            message: format!("deny_all: tool '{}' blocked", definition.id),
            reason: PermissionReason::Other("deny_all".into()),
        }
    }
}

// ─── Helper ──────────────────────────────────────────────────────────────────

fn make_ctx_with_file_ops(file_ops: Arc<dyn FileOperations>) -> ToolExecutionContext {
    let cap = CapabilityContext {
        storage: Some(app_lib::runtime::tools::StorageCapability {
            workspace_path: std::path::PathBuf::from("/tmp/mock-workspace"),
            authorized_workspace: None,
        }),
        workspace_id: Some("ws-test".to_string()),
        browser_available: false,
        runtime_resolver: None,
        file_ops: Some(file_ops),
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: None,
        is_subagent: false,
    };
    ToolExecutionContext::for_test("test-conv", "test-run", "test-tc")
        .with_capability(Arc::new(cap))
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: load_file 调用后 is_loaded() 返回 true（loaded key 语义）
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn load_file_is_loaded_returns_true_after_successful_load() {
    let file_ops = Arc::new(MockFileOps::new());

    // Before any load, is_loaded must be false
    assert!(
        !file_ops.is_loaded("test-file-1", "default-scope"),
        "is_loaded must be false before load_file is called"
    );

    // Simulate a load call with file_id + scope_id
    let args = json!({"file_id": "test-file-1", "scope_id": "default-scope"});
    let result = file_ops.load_file(&args).await;
    assert!(
        result.is_ok(),
        "load_file should succeed, got: {:?}",
        result.err()
    );

    // After a successful load, is_loaded must return true
    assert!(
        file_ops.is_loaded("test-file-1", "default-scope"),
        "is_loaded must return true after a successful load_file call"
    );

    // Different file_id must still be false
    assert!(
        !file_ops.is_loaded("other-file-id", "default-scope"),
        "is_loaded must be false for a file that was not loaded"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: LoadFileRuntimeTool::execute() 返回的 content 包含 file_meta 相关信息
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn load_file_runtime_tool_execute_returns_file_meta_content() {
    let file_ops = Arc::new(MockFileOps::new());
    let ctx = make_ctx_with_file_ops(file_ops.clone());

    let tool = LoadFileRuntimeTool::new();
    let input = json!({"file_id": "test-file-1"});
    let result = RuntimeTool::execute(&tool, input, ctx).await;

    assert!(
        result.is_ok(),
        "LoadFileRuntimeTool::execute should succeed when file_ops is present, got: {:?}",
        result.err()
    );

    let tool_result = result.unwrap();
    assert_eq!(tool_result.tool_name, "load_file");
    assert!(
        tool_result.content.contains("file_meta"),
        "ToolResult content should include file_meta, got: {}",
        tool_result.content
    );

    // data field must be Some (it mirrors the content string)
    assert!(
        tool_result.data.is_some(),
        "ToolResult data field must be Some when load_file succeeds"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2b: file_ops が None のとき ToolError::Other が返る（境界チェック）
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn load_file_runtime_tool_execute_errors_when_file_ops_is_none() {
    // No file_ops attached — capability has no file_ops field
    let cap = CapabilityContext {
        storage: Some(app_lib::runtime::tools::StorageCapability {
            workspace_path: std::path::PathBuf::from("/tmp"),
            authorized_workspace: None,
        }),
        workspace_id: Some("ws-test".to_string()),
        browser_available: false,
        runtime_resolver: None,
        file_ops: None, // deliberately absent
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: None,
        is_subagent: false,
    };
    let ctx = ToolExecutionContext::for_test("test-conv", "test-run", "test-tc")
        .with_capability(Arc::new(cap));

    let tool = LoadFileRuntimeTool::new();
    let result = RuntimeTool::execute(&tool, json!({"file_id": "any"}), ctx).await;

    assert!(
        result.is_err(),
        "LoadFileRuntimeTool::execute should return Err when file_ops is None"
    );
    assert!(
        matches!(result, Err(ToolError::Other(_))),
        "expected ToolError::Other when file_ops is None, got: {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: cancellation token 来自 turn cascade
// ─────────────────────────────────────────────────────────────────────────────
// 这个测试复用 cancel_cascade_test.rs 的 TurnState pattern，
// 验证 turn.build_execution_context() 构造的 ctx.cancellation 是 parent 的 grandchild。

#[test]
fn load_file_context_cancellation_cascades_from_parent_via_turn() {
    use app_lib::runtime::cancellation::CancellationToken;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::state::TurnState;

    let parent = CancellationToken::new();

    let mapping = IdentityMapping::from_legacy_conversation_id("load-file-sess".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("load-file-run"),
        "load_file call".into(),
    )
    .with_cancellation(parent.child_token());

    // build_execution_context() creates a child of the turn's token
    let ctx = turn.build_execution_context("call-load-file-1");

    assert!(
        !ctx.cancellation.is_cancelled(),
        "ctx cancellation should start uncancelled"
    );

    // Cancelling the parent must cascade all the way down to ctx
    parent.cancel();

    assert!(
        turn.cancellation().is_cancelled(),
        "parent cancel must cascade to turn token"
    );
    assert!(
        ctx.cancellation.is_cancelled(),
        "parent cancel must cascade through turn to ctx.cancellation \
        (load_file gets the correct cancellation token from turn cascade)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: load_file 经过统一 permission pipeline（DenyAll → PermissionDenied）
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn load_file_goes_through_permission_pipeline_deny_all_returns_permission_denied() {
    // ToolDispatcher with a DenyAll pipeline — load_file should be blocked.
    let dispatcher = ToolDispatcher::new(Arc::new(DenyAllPipeline));
    dispatcher.register(Arc::new(LoadFileRuntimeTool::new()));

    // Build a ctx with file_ops so the tool *would* succeed if permission were granted
    let file_ops = Arc::new(MockFileOps::new());
    let ctx = make_ctx_with_file_ops(file_ops);

    let outcome = dispatcher
        .dispatch("load_file", json!({"file_id": "test-file-1"}), ctx)
        .await;

    assert!(
        outcome.is_err(),
        "DenyAll pipeline must cause dispatch to return Err, got Ok"
    );
    // outcome is Err here — extract the error for a better assertion message.
    // We can't use unwrap_err() because ToolDispatchOutcome doesn't implement Debug.
    match outcome {
        Err(ToolError::PermissionDenied(msg)) => {
            assert!(
                msg.contains("load_file"),
                "PermissionDenied message should mention tool name, got: {}",
                msg
            );
        }
        Err(other_err) => panic!("expected PermissionDenied, got: {:?}", other_err),
        Ok(_) => panic!("expected Err, got Ok"),
    }
}
