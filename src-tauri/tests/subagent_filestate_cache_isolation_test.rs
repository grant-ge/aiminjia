#![allow(deprecated)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use app_lib::plugin::context::PluginContext;
use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::tools::capability::{FileState, FileStateCache};
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::TempDir;

struct CaptureFileStateTool;

#[async_trait]
impl RuntimeTool for CaptureFileStateTool {
    fn id(&self) -> &str {
        "capture_file_state"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("capture_file_state", "capture subagent file state cache")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let target = PathBuf::from(
            input
                .get("path")
                .and_then(Value::as_str)
                .expect("path is required"),
        );
        let content = input
            .get("content")
            .and_then(Value::as_str)
            .expect("content is required");

        let cache = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.read_file_state.as_ref())
            .expect("subagent runtime tool should receive read_file_state");

        cache.set(
            target.clone(),
            FileState {
                content: content.to_string(),
                mtime_secs: 2_000,
                offset: None,
                limit: None,
            },
        );

        Ok(ToolResult::new(
            "capture_file_state",
            format!("updated {}", target.display()),
            None,
        ))
    }
}

#[allow(deprecated)]
fn make_plugin_ctx(workspace: &Path, cache: Option<Arc<FileStateCache>>) -> PluginContext {
    let storage = Arc::new(AppStorage::new(workspace).expect("AppStorage::new failed"));
    let file_manager = Arc::new(FileManager::new(workspace));

    PluginContext {
        storage,
        file_manager,
        workspace_path: workspace.to_path_buf(),
        conversation_id: "subagent-cache-conv".to_string(),
        session_id: app_lib::runtime::ids::SessionId::new("subagent-cache-conv"),
        run_id: Some(app_lib::runtime::ids::RunId::new("subagent-cache-run")),
        agent_id: Some(app_lib::runtime::ids::AgentId::new("child-agent")),
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        auth_manager: None,
        dingtalk_bridge: None,
        use_cloud: false,
        model: String::new(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        skill_registry: None,
        authorized_workspace: None,
        read_file_state: cache,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
            runtime_resolver: None,
        permission_ctx: None,
        current_persona_id: None,
    }
}

#[test]
fn test_h1_1_clone_for_child_reads_parent_snapshot() {
    let parent_cache = Arc::new(FileStateCache::new());
    let target = PathBuf::from("/tmp/subagent-cache.txt");
    parent_cache.set(
        target.clone(),
        FileState {
            content: "parent-content".to_string(),
            mtime_secs: 1_000,
            offset: None,
            limit: None,
        },
    );

    let child_cache = parent_cache.clone_for_child();

    let child_state = child_cache
        .get(&target)
        .expect("child should inherit parent snapshot");
    assert_eq!(child_state.content, "parent-content");

    child_cache.set(
        target.clone(),
        FileState {
            content: "child-content".to_string(),
            mtime_secs: 2_000,
            offset: None,
            limit: None,
        },
    );

    let parent_state = parent_cache
        .get(&target)
        .expect("parent state should still exist");
    assert_eq!(parent_state.content, "parent-content");
    assert_eq!(parent_state.mtime_secs, 1_000);
}

#[test]
fn test_h1_2_child_snapshot_isolated_from_later_parent_writes() {
    let parent_cache = Arc::new(FileStateCache::new());
    let target = PathBuf::from("/tmp/subagent-cache-later.txt");
    parent_cache.set(
        target.clone(),
        FileState {
            content: "initial".to_string(),
            mtime_secs: 1_000,
            offset: None,
            limit: None,
        },
    );

    let child_cache = parent_cache.clone_for_child();

    parent_cache.set(
        target.clone(),
        FileState {
            content: "parent-updated".to_string(),
            mtime_secs: 3_000,
            offset: None,
            limit: None,
        },
    );

    let child_state = child_cache
        .get(&target)
        .expect("child snapshot should still exist");
    assert_eq!(child_state.content, "initial");
    assert_eq!(child_state.mtime_secs, 1_000);
}

#[tokio::test]
async fn test_h1_3_subagent_runtime_tool_writes_only_child_cache() {
    let tmp = TempDir::new().expect("TempDir::new failed");
    let registry = ToolRegistry::new();
    registry
        .register_runtime(Arc::new(CaptureFileStateTool))
        .await;

    let target = PathBuf::from("/tmp/subagent-write.txt");
    let parent_cache = Arc::new(FileStateCache::new());
    parent_cache.set(
        target.clone(),
        FileState {
            content: "parent-before".to_string(),
            mtime_secs: 1_000,
            offset: None,
            limit: None,
        },
    );

    let child_cache = parent_cache.clone_for_child();
    let plugin_ctx = make_plugin_ctx(tmp.path(), Some(child_cache.clone()));

    registry
        .execute(
            "capture_file_state",
            &RequestScopedRuntimeDeps::from_plugin_context(&plugin_ctx),
            json!({
                "path": target.to_string_lossy(),
                "content": "child-after"
            }),
            CancellationToken::new(),
        )
        .await
        .expect("runtime tool should update child cache only");

    let parent_state = parent_cache
        .get(&target)
        .expect("parent cache should retain original");
    assert_eq!(parent_state.content, "parent-before");

    let child_state = child_cache
        .get(&target)
        .expect("child cache should be updated");
    assert_eq!(child_state.content, "child-after");
    assert_eq!(child_state.mtime_secs, 2_000);
}
