use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use app_lib::python::runner::ExecutionResult;
use app_lib::python::sandbox::SandboxConfig;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
use app_lib::runtime::tools::builtin::python_execution::{
    DefaultPythonExecution, PythonExecution,
};
use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext};
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use async_trait::async_trait;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn python_execution_trait_is_accessible() {
    let _: Option<Box<dyn PythonExecution>> = None;
}

#[test]
fn default_python_execution_implements_trait() {
    fn assert_impl<T: PythonExecution>() {}
    assert_impl::<DefaultPythonExecution>();
}

#[derive(Clone, Debug)]
struct MockPythonExecution {
    result: ExecutionResult,
}

#[async_trait]
impl PythonExecution for MockPythonExecution {
    async fn execute_oneshot(
        &self,
        _workspace_path: &Path,
        _code: &str,
        _sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        Ok(self.result.clone())
    }

    async fn execute_for_run(
        &self,
        _run_id: &RunId,
        _code: &str,
        _timeout: Duration,
        _sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        Ok(self.result.clone())
    }

    async fn interrupt_run(&self, _run_id: &RunId) -> Result<()> {
        Ok(())
    }
}

fn build_runtime_tool(
    workspace: &std::path::Path,
    requested_run_id: Option<RunId>,
) -> ExecutePythonRuntimeTool {
    let storage = Arc::new(AppStorage::new(workspace).expect("AppStorage::new should succeed"));
    storage
        .create_conversation("plan-e-conv", "Plan E")
        .expect("conversation should be created");
    let file_manager = Arc::new(FileManager::new(workspace));
    let mock = Arc::new(MockPythonExecution {
        result: ExecutionResult {
            stdout: "hello from mock\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
            execution_time_ms: 10,
            timed_out: false,
        },
    });

    ExecutePythonRuntimeTool::with_runtime_deps(
        mock as Arc<dyn PythonExecution>,
        storage,
        file_manager,
        requested_run_id,
        "test-model".to_string(),
    )
}

fn build_exec_ctx(workspace: &std::path::Path) -> ToolExecutionContext {
    let cap = Arc::new(CapabilityContext::with_workspace(
        workspace.to_path_buf(),
        "plan-e-conv",
    ));
    ToolExecutionContext::for_test("plan-e-conv", "run-1", "tc-1").with_capability(cap)
}

#[tokio::test]
async fn execute_python_runtime_tool_with_mock_returns_stdout_without_plugin_context() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let tool = build_runtime_tool(tmp.path(), Some(RunId::new("run-1")));

    let result = tool
        .execute(
            json!({
                "code": "print('hello from mock')",
                "purpose": "plan-e smoke",
            }),
            build_exec_ctx(tmp.path()),
        )
        .await
        .expect("runtime tool should execute without PluginContext");

    assert!(
        result.content.contains("hello from mock"),
        "stdout should flow through runtime tool: {}",
        result.content
    );
}

#[tokio::test]
async fn execute_python_runtime_tool_preserves_missing_run_id_analysis_error() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let storage = Arc::new(AppStorage::new(tmp.path()).expect("AppStorage::new should succeed"));
    storage
        .create_conversation("plan-e-conv", "Plan E")
        .expect("conversation should be created");
    storage
        .upsert_analysis_state("plan-e-conv", 1, r#"{"status":"running"}"#, "{}")
        .expect("analysis state should be created");

    let file_manager = Arc::new(FileManager::new(tmp.path()));
    let mock = Arc::new(MockPythonExecution {
        result: ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            execution_time_ms: 10,
            timed_out: false,
        },
    });
    let tool = ExecutePythonRuntimeTool::with_runtime_deps(
        mock as Arc<dyn PythonExecution>,
        storage,
        file_manager,
        None,
        "test-model".to_string(),
    );

    let err = tool
        .execute(json!({"code": "print('analysis')"}), build_exec_ctx(tmp.path()))
        .await
        .expect_err("analysis runtime path without requested run_id must fail");

    assert!(
        matches!(err, ToolError::ExecutionFailed(_)),
        "analysis failure should surface as execution failure: {err:?}"
    );
    assert!(
        err.to_string().contains("run_id"),
        "analysis error should mention missing run_id, got: {}",
        err
    );
}
