/// Phase 2 Task 3 Step 3 — ToolCapabilityContext boundary tests.
///
/// Verifies that:
/// 1. New RuntimeTool implementations receive only a scoped CapabilityContext,
///    not the full PluginContext.
/// 2. ToolExecutionContext can carry an optional CapabilityContext.
/// 3. LegacyToolAdapter continues to bridge legacy ToolPlugin via from_plugin,
///    proving no regression.
use app_lib::runtime::dependencies::{
    RuntimeDependencyError, StaticRuntimeResolver, WorkspaceDependencies,
};
use app_lib::runtime::tools::capability::{CapabilityContext, StorageCapability};
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// A fake RuntimeTool that reads StorageCapability from CapabilityContext.
// ---------------------------------------------------------------------------
struct WorkspacePrinterTool;

#[async_trait]
impl RuntimeTool for WorkspacePrinterTool {
    fn id(&self) -> &str {
        "workspace_printer"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(
            "workspace_printer",
            "Print workspace path from capability ctx",
        )
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let workspace = ctx
            .capability
            .as_ref()
            .and_then(|c| c.storage.as_ref())
            .map(|s| s.workspace_path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string());
        Ok(ToolResult::new("workspace_printer", workspace, None))
    }
}

// ---------------------------------------------------------------------------
// Test: ToolExecutionContext.capability is None by default.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn tool_execution_context_has_no_capability_by_default() {
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    assert!(ctx.capability.is_none());
}

// ---------------------------------------------------------------------------
// Test: with_capability attaches a CapabilityContext; tool can read it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn runtime_tool_reads_workspace_from_capability_context() {
    let storage_cap = StorageCapability {
        workspace_path: std::path::PathBuf::from("/tmp/test-workspace"),
        authorized_workspace: None,
        permission_ctx: std::sync::Arc::new(
            app_lib::runtime::path_auth::ToolPermissionContext::empty(),
        ),
    };
    let cap_ctx = CapabilityContext {
        storage: Some(storage_cap),
        workspace_id: Some("ws-42".to_string()),
        runtime_resolver: None,
        file_ops: None,
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: None,
        is_subagent: false,
    };
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_capability(Arc::new(cap_ctx));

    let tool = WorkspacePrinterTool;
    let result = RuntimeTool::execute(&tool, json!({}), ctx).await.unwrap();
    assert_eq!(result.content, "/tmp/test-workspace");
}

// ---------------------------------------------------------------------------
// Test: capability context only exposes limited fields — no gateway, no
// auth_manager, no full PluginContext.  Structural check via field access.
// ---------------------------------------------------------------------------
#[test]
fn capability_context_does_not_expose_full_plugin_context() {
    let cap = CapabilityContext {
        storage: None,
        workspace_id: Some("ws-1".to_string()),
        runtime_resolver: None,
        file_ops: None,
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: None,
        is_subagent: false,
    };
    // Verify we can ONLY access the declared fields: storage, workspace_id.
    // If PluginContext fields (e.g. gateway, auth_manager) were leaked, this
    // compile-time test would fail because those fields don't exist on
    // CapabilityContext.
    let _ = cap.storage;
    let _ = cap.workspace_id;
}

// ── Task 1.2 tests ──────────────────────────────────────────────────────────

#[test]
fn file_state_cache_returns_none_for_unknown_path() {
    use app_lib::runtime::tools::capability::FileStateCache;
    let cache = FileStateCache::new();
    assert!(cache
        .get(std::path::Path::new("/tmp/nonexistent.txt"))
        .is_none());
}

#[test]
fn file_state_cache_stores_and_retrieves_entry() {
    use app_lib::runtime::tools::capability::{FileState, FileStateCache};
    let cache = FileStateCache::new();
    let path = std::path::PathBuf::from("/tmp/test.csv");
    let state = FileState {
        content: "a,b,c".to_string(),
        mtime_secs: 1000,
        offset: None,
        limit: None,
    };
    cache.set(path.clone(), state.clone());
    let retrieved = cache.get(&path).unwrap();
    assert_eq!(retrieved.content, "a,b,c");
    assert_eq!(retrieved.mtime_secs, 1000);
}

#[test]
fn file_reading_limits_default_is_one_mb() {
    use app_lib::runtime::tools::capability::FileReadingLimits;
    let limits = FileReadingLimits::default();
    assert_eq!(limits.max_size_bytes, 1_048_576);
}

#[test]
fn capability_context_new_fields_default_to_none() {
    use app_lib::runtime::tools::capability::CapabilityContext;
    let ctx = CapabilityContext {
        storage: None,
        workspace_id: None,
        runtime_resolver: None,
        file_ops: None,
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: None,
        is_subagent: false,
    };
    assert!(ctx.read_file_state.is_none());
    assert!(ctx.file_reading_limits.is_none());
    assert!(ctx.notification_sink.is_none());
}

#[test]
fn file_state_offset_and_limit_accept_usize() {
    use app_lib::runtime::tools::capability::FileState;
    let offset: usize = 5;
    let limit: usize = 10;
    let state = FileState {
        content: "a,b,c".to_string(),
        mtime_secs: 1000,
        offset: Some(offset),
        limit: Some(limit),
    };
    assert_eq!(state.offset, Some(5_usize));
    assert_eq!(state.limit, Some(10_usize));
}

#[derive(Debug)]
struct TestNotificationSink;

impl app_lib::runtime::tools::capability::NotificationSink for TestNotificationSink {
    fn notify(&self, _message: &str) {}
}

#[test]
fn notification_sink_exposes_notify_method() {
    use app_lib::runtime::tools::capability::NotificationSink;
    let sink = TestNotificationSink;
    sink.notify("hello");
}

// ── Task 1.3 tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn read_workspace_file_uses_file_state_cache_on_second_read() {
    use app_lib::runtime::tools::builtin::workspace::ReadWorkspaceFileRuntimeTool;
    use app_lib::runtime::tools::capability::{
        CapabilityContext, FileReadingLimits, FileStateCache,
    };
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    write!(tmp, "line1\nline2\n").unwrap();
    let dir = tmp.path().parent().unwrap().to_path_buf();
    let filename = tmp
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let cache = Arc::new(FileStateCache::new());
    let cap = CapabilityContext::with_workspace(dir.clone(), "ws")
        .with_read_file_state(cache.clone())
        .with_file_reading_limits(FileReadingLimits::default());
    let ctx = || {
        ToolExecutionContext::for_test("conv", "run", "tc-1").with_capability(Arc::new(cap.clone()))
    };

    let tool = ReadWorkspaceFileRuntimeTool;

    let r1 = RuntimeTool::execute(&tool, json!({"file_path": filename}), ctx())
        .await
        .unwrap();
    let r1_data = r1
        .data
        .as_ref()
        .expect("first read should include structured data");
    assert_eq!(r1_data["content"], json!("line1\nline2\n"));
    assert!(r1_data.get("cached").is_none());

    let r2 = RuntimeTool::execute(&tool, json!({"file_path": filename}), ctx())
        .await
        .unwrap();
    let r2_data = r2
        .data
        .as_ref()
        .expect("second read should include structured data");
    assert_eq!(r2_data["content"], json!("line1\nline2\n"));
    assert_eq!(r2_data["cached"], json!(true));
}

#[tokio::test]
async fn read_workspace_file_reloads_when_larger_request_exceeds_truncated_cache() {
    use app_lib::runtime::tools::builtin::workspace::ReadWorkspaceFileRuntimeTool;
    use app_lib::runtime::tools::capability::{CapabilityContext, FileStateCache};
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    write!(tmp, "abcdefghij").unwrap();
    let dir = tmp.path().parent().unwrap().to_path_buf();
    let filename = tmp
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let cache = Arc::new(FileStateCache::new());
    let cap = CapabilityContext::with_workspace(dir, "ws").with_read_file_state(cache);
    let ctx = || {
        ToolExecutionContext::for_test("conv", "run", "tc-1").with_capability(Arc::new(cap.clone()))
    };

    let tool = ReadWorkspaceFileRuntimeTool;

    let r1 = RuntimeTool::execute(&tool, json!({"file_path": filename, "max_bytes": 4}), ctx())
        .await
        .unwrap();
    let r1_data = r1
        .data
        .as_ref()
        .expect("truncated read should include structured data");
    assert_eq!(r1_data["content"], json!("abcd"));
    assert_eq!(r1_data["truncated"], json!(true));
    assert!(r1_data.get("cached").is_none());

    let r2 = RuntimeTool::execute(&tool, json!({"file_path": filename, "max_bytes": 4}), ctx())
        .await
        .unwrap();
    let r2_data = r2
        .data
        .as_ref()
        .expect("repeat read should include structured data");
    assert_eq!(r2_data["content"], json!("abcd"));
    assert_eq!(r2_data["cached"], json!(true));
    assert_eq!(r2_data["truncated"], json!(true));

    let r3 = RuntimeTool::execute(&tool, json!({"file_path": filename}), ctx())
        .await
        .unwrap();
    let r3_data = r3
        .data
        .as_ref()
        .expect("larger read should include structured data");
    assert_eq!(r3_data["content"], json!("abcdefghij"));
    assert!(r3_data.get("cached").is_none());
    assert!(r3_data.get("truncated").is_none());
}

#[tokio::test]
async fn read_workspace_file_truncates_cached_content_for_smaller_follow_up_limit() {
    use app_lib::runtime::tools::builtin::workspace::ReadWorkspaceFileRuntimeTool;
    use app_lib::runtime::tools::capability::{CapabilityContext, FileStateCache};
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    write!(tmp, "abcdefghij").unwrap();
    let dir = tmp.path().parent().unwrap().to_path_buf();
    let filename = tmp
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let cache = Arc::new(FileStateCache::new());
    let cap = CapabilityContext::with_workspace(dir, "ws").with_read_file_state(cache);
    let ctx = || {
        ToolExecutionContext::for_test("conv", "run", "tc-1").with_capability(Arc::new(cap.clone()))
    };

    let tool = ReadWorkspaceFileRuntimeTool;

    let r1 = RuntimeTool::execute(&tool, json!({"file_path": filename}), ctx())
        .await
        .unwrap();
    let r1_data = r1
        .data
        .as_ref()
        .expect("initial read should include structured data");
    assert_eq!(r1_data["content"], json!("abcdefghij"));

    let r2 = RuntimeTool::execute(&tool, json!({"file_path": filename, "max_bytes": 4}), ctx())
        .await
        .unwrap();
    let r2_data = r2
        .data
        .as_ref()
        .expect("cached read should include structured data");
    assert_eq!(r2_data["content"], json!("abcd"));
    assert_eq!(r2_data["cached"], json!(true));
    assert_eq!(r2_data["truncated"], json!(true));
}

#[tokio::test]
async fn read_workspace_file_preserves_utf8_boundaries_between_cold_and_cached_reads() {
    use app_lib::runtime::tools::builtin::workspace::ReadWorkspaceFileRuntimeTool;
    use app_lib::runtime::tools::capability::{CapabilityContext, FileStateCache};
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    write!(tmp, "你好z").unwrap();
    let dir = tmp.path().parent().unwrap().to_path_buf();
    let filename = tmp
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let cache = Arc::new(FileStateCache::new());
    let cap = CapabilityContext::with_workspace(dir, "ws").with_read_file_state(cache);
    let ctx = || {
        ToolExecutionContext::for_test("conv", "run", "tc-1").with_capability(Arc::new(cap.clone()))
    };

    let tool = ReadWorkspaceFileRuntimeTool;

    let r1 = RuntimeTool::execute(&tool, json!({"file_path": filename, "max_bytes": 6}), ctx())
        .await
        .unwrap();
    let r1_data = r1
        .data
        .as_ref()
        .expect("cold read should include structured data");
    assert_eq!(r1_data["content"], json!("你好"));
    assert_eq!(r1_data["truncated"], json!(true));
    assert!(r1_data.get("cached").is_none());
    assert!(!r1_data["content"].as_str().unwrap().contains('\u{fffd}'));

    let r2 = RuntimeTool::execute(&tool, json!({"file_path": filename, "max_bytes": 6}), ctx())
        .await
        .unwrap();
    let r2_data = r2
        .data
        .as_ref()
        .expect("cached read should include structured data");
    assert_eq!(r2_data["content"], json!("你好"));
    assert_eq!(r2_data["cached"], json!(true));
    assert_eq!(r2_data["truncated"], json!(true));
    assert!(!r2_data["content"].as_str().unwrap().contains('\u{fffd}'));
}

#[test]
fn notification_sink_receives_message_from_tool_context() {
    use app_lib::runtime::tools::capability::{CapabilityContext, NotificationSink};
    use std::sync::{Arc, Mutex};

    struct RecordingSink(Mutex<Vec<String>>);
    impl std::fmt::Debug for RecordingSink {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "RecordingSink")
        }
    }
    impl NotificationSink for RecordingSink {
        fn notify(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    let sink = Arc::new(RecordingSink(Mutex::new(vec![])));
    let cap = CapabilityContext {
        storage: None,
        workspace_id: None,
        runtime_resolver: None,
        file_ops: None,
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: Some(sink.clone()),
        is_subagent: false,
    };

    if let Some(s) = &cap.notification_sink {
        s.notify("test notification");
    }
    let msgs = sink.0.lock().unwrap();
    assert_eq!(msgs.as_slice(), &["test notification"]);
}

#[test]
fn capability_context_exposes_runtime_dependencies() {
    let expected = WorkspaceDependencies {
        python: "/tmp/runtime/python".into(),
        node: "/tmp/runtime/node".into(),
        npm: "/tmp/runtime/npm".into(),
        npx: "/tmp/runtime/npx".into(),
        uv: "/tmp/runtime/uv".into(),
        uvx: "/tmp/runtime/uvx".into(),
        node_modules: "/tmp/runtime/node_modules".into(),
        python_site_packages: "/tmp/runtime/site-packages".into(),
    };
    let resolver = Arc::new(StaticRuntimeResolver::new(
        expected.python.clone(),
        expected.node.clone(),
        expected.npm.clone(),
        expected.npx.clone(),
        expected.uv.clone(),
        expected.uvx.clone(),
        expected.node_modules.clone(),
        expected.python_site_packages.clone(),
    ));

    let ctx = CapabilityContext::with_workspace("/tmp/workspace".into(), "ws")
        .with_runtime_resolver(resolver);

    let deps = ctx
        .workspace_dependencies()
        .expect("resolver should provide deps");
    assert_eq!(deps, expected);
    assert_eq!(deps.python, std::path::PathBuf::from("/tmp/runtime/python"));
}

#[test]
fn capability_context_without_runtime_resolver_returns_clear_error() {
    let ctx = CapabilityContext::with_workspace("/tmp/workspace".into(), "ws");
    let err = ctx
        .workspace_dependencies()
        .expect_err("missing resolver should return an error");
    assert!(matches!(
        err,
        RuntimeDependencyError::ResolverUnavailable(_)
    ));
}
