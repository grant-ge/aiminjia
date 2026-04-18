use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use app_lib::python::runner::ExecutionResult;
use app_lib::python::sandbox::SandboxConfig;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{AgentId, RunId, SessionId};
use app_lib::runtime::tools::builtin::browse_data::{
    BrowseDataLaunchContext, BrowseDataLaunchRequest, BrowseDataLaunchResult, BrowseDataLauncher,
    BrowseDataRuntimeTool,
};
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
use app_lib::runtime::tools::builtin::python_execution::{
    DefaultPythonExecution, PythonExecution,
};
use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
use app_lib::runtime::tools::builtin::chart_capability::{
    ChartCapability, ChartRunOutput, PersistedChartInfo,
};
use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
use app_lib::runtime::tools::builtin::report_capability::{
    PersistedFileInfo, ReportCapability, ReportGenOutput,
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
        std::path::PathBuf::from("python3"),
        None,
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
        std::path::PathBuf::from("python3"),
        None,
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

#[derive(Debug, Default)]
struct MockReportCapability {
    persisted_bytes: Mutex<Vec<u8>>,
}

#[async_trait]
impl ReportCapability for MockReportCapability {
    async fn generate_report_bytes(
        &self,
        _workspace_path: &Path,
        _title: &str,
        _sections: &[serde_json::Value],
        _format: &str,
        unmask_map: &HashMap<String, String>,
        product_name: Option<&str>,
    ) -> Result<ReportGenOutput> {
        let mut html = "<html>AI小家 {{NAME}}</html>".to_string();
        if let Some(name) = product_name {
            html = html.replace("AI小家", name);
        }
        for (masked, original) in unmask_map {
            html = html.replace(masked, original);
        }
        Ok(ReportGenOutput {
            bytes: html.into_bytes(),
            extension: "html".to_string(),
            actual_format: "html".to_string(),
            is_degraded: false,
            degradation_notice: None,
        })
    }

    fn get_pii_unmask_map(&self, _conversation_id: &str) -> HashMap<String, String> {
        HashMap::from([(String::from("{{NAME}}"), String::from("张三"))])
    }

    async fn get_product_name(&self) -> Option<String> {
        Some("Lotus".to_string())
    }

    async fn persist_file(
        &self,
        _conversation_id: &str,
        bytes: &[u8],
        _extension: &str,
        _title: &str,
        _actual_format: &str,
    ) -> Result<PersistedFileInfo> {
        *self.persisted_bytes.lock().expect("mutex poisoned") = bytes.to_vec();
        Ok(PersistedFileInfo {
            file_id: "report-file-1".to_string(),
            file_name: "mock-report.html".to_string(),
            stored_path: "reports/mock-report.html".to_string(),
            file_size: bytes.len() as u64,
        })
    }
}

#[test]
fn report_capability_trait_is_accessible() {
    let _: Option<Box<dyn ReportCapability>> = None;
}

#[tokio::test]
async fn generate_report_stub_returns_execution_failed() {
    let tool = GenerateReportRuntimeTool::stub();
    let result = tool
        .execute(
            json!({
                "title": "stub",
                "sections": [{"heading": "summary"}],
            }),
            ToolExecutionContext::for_test("plan-e-conv", "run-1", "tc-report-stub"),
        )
        .await;

    assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
}

#[tokio::test]
async fn generate_report_runtime_tool_with_mock_capability_persists_transformed_html() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let cap = Arc::new(MockReportCapability::default());
    let tool = GenerateReportRuntimeTool::with_capability(cap.clone() as Arc<dyn ReportCapability>);

    let result = tool
        .execute(
            json!({
                "title": "Plan E Report",
                "sections": [{"heading": "summary", "content": "body"}],
            }),
            build_exec_ctx(tmp.path()),
        )
        .await
        .expect("runtime report tool should execute without PluginContext");

    let persisted = String::from_utf8(
        cap.persisted_bytes
            .lock()
            .expect("mutex poisoned")
            .clone(),
    )
    .expect("persisted html should be utf-8");

    assert!(
        persisted.contains("Lotus"),
        "product name should be substituted before persist: {persisted}"
    );
    assert!(
        persisted.contains("张三"),
        "PII placeholders should be unmasked before persist: {persisted}"
    );
    assert!(
        result.content.contains("report-file-1"),
        "result should contain persisted file id: {}",
        result.content
    );
    assert_eq!(
        result
            .file_meta
            .as_ref()
            .expect("file meta should be preserved")
            .stored_path,
        "reports/mock-report.html"
    );
}

#[tokio::test]
async fn generate_report_runtime_tool_rejects_source_outside_workspace() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let outside = TempDir::new().expect("TempDir::new should succeed");
    let source_path = outside.path().join("sections.json");
    std::fs::write(
        &source_path,
        r#"[{"heading":"Outside","content":"should fail"}]"#,
    )
    .expect("outside source file should be written");

    let tool =
        GenerateReportRuntimeTool::with_capability(Arc::new(MockReportCapability::default()));

    let err = tool
        .execute(
            json!({
                "title": "Outside",
                "source": source_path.to_string_lossy(),
            }),
            build_exec_ctx(tmp.path()),
        )
        .await
        .expect_err("source outside workspace must be rejected");

    assert!(
        matches!(err, ToolError::ExecutionFailed(_)),
        "source boundary should surface as execution failure: {err:?}"
    );
    assert!(
        err.to_string().contains("outside"),
        "error should mention outside workspace boundary, got: {err}"
    );
}

#[derive(Debug, Default)]
struct MockChartCapability;

#[async_trait]
impl ChartCapability for MockChartCapability {
    async fn run_chart_python(
        &self,
        _workspace_path: &Path,
        _chart_type: &str,
        _title: &str,
        _data: &serde_json::Value,
        _options: &serde_json::Value,
    ) -> Result<ChartRunOutput> {
        Ok(ChartRunOutput {
            html_bytes: b"<html>mock chart</html>".to_vec(),
            chart_filename: "mock-chart.html".to_string(),
        })
    }

    async fn persist_chart(
        &self,
        _conversation_id: &str,
        bytes: &[u8],
        filename: &str,
        _chart_type: &str,
        _title: &str,
    ) -> Result<PersistedChartInfo> {
        Ok(PersistedChartInfo {
            file_id: "chart-file-1".to_string(),
            file_name: filename.to_string(),
            stored_path: format!("charts/{filename}"),
            file_size: bytes.len() as u64,
        })
    }
}

#[test]
fn chart_capability_trait_is_accessible() {
    let _: Option<Box<dyn ChartCapability>> = None;
}

#[tokio::test]
async fn generate_chart_stub_returns_execution_failed() {
    let tool = GenerateChartRuntimeTool::stub();
    let result = tool
        .execute(
            json!({
                "chart_type": "bar",
                "title": "stub chart",
                "data": {"labels": ["Q1"], "values": [1]},
            }),
            ToolExecutionContext::for_test("plan-e-conv", "run-1", "tc-chart-stub"),
        )
        .await;

    assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
}

#[tokio::test]
async fn generate_chart_runtime_tool_with_mock_capability_returns_file_meta() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let tool = GenerateChartRuntimeTool::with_capability(Arc::new(MockChartCapability));

    let result = tool
        .execute(
            json!({
                "chart_type": "bar",
                "title": "Plan E Chart",
                "data": {"labels": ["Q1", "Q2"], "values": [10, 12]},
            }),
            build_exec_ctx(tmp.path()),
        )
        .await
        .expect("runtime chart tool should execute without PluginContext");

    assert!(
        result.content.contains("chart-file-1"),
        "result should contain persisted chart file id: {}",
        result.content
    );
    assert_eq!(
        result
            .file_meta
            .as_ref()
            .expect("chart file meta should be preserved")
            .stored_path,
        "charts/mock-chart.html"
    );
}

#[tokio::test]
async fn generate_chart_runtime_tool_rejects_data_file_outside_workspace() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let outside = TempDir::new().expect("TempDir::new should succeed");
    let data_file = outside.path().join("chart.json");
    std::fs::write(
        &data_file,
        r#"{"labels":["Q1"],"values":[42]}"#,
    )
    .expect("outside chart file should be written");

    let tool = GenerateChartRuntimeTool::with_capability(Arc::new(MockChartCapability));
    let err = tool
        .execute(
            json!({
                "chart_type": "bar",
                "title": "Outside chart",
                "data_file": data_file.to_string_lossy(),
            }),
            build_exec_ctx(tmp.path()),
        )
        .await
        .expect_err("data_file outside workspace must be rejected");

    assert!(
        matches!(err, ToolError::ExecutionFailed(_)),
        "outside chart file should surface as execution failure: {err:?}"
    );
    assert!(
        err.to_string().contains("outside"),
        "error should mention workspace boundary, got: {err}"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BrowseDataLaunchSnapshot {
    task: String,
    url: Option<String>,
    session_id: String,
    parent_run_id: Option<String>,
    parent_agent_id: Option<String>,
    cancelled: bool,
}

#[derive(Debug)]
struct MockBrowseDataLauncher {
    response: String,
    snapshots: Arc<Mutex<Vec<BrowseDataLaunchSnapshot>>>,
}

#[async_trait]
impl BrowseDataLauncher for MockBrowseDataLauncher {
    async fn launch(
        &self,
        request: BrowseDataLaunchRequest,
        context: BrowseDataLaunchContext,
    ) -> Result<BrowseDataLaunchResult> {
        self.snapshots
            .lock()
            .expect("mutex poisoned")
            .push(BrowseDataLaunchSnapshot {
                task: request.task,
                url: request.url,
                session_id: context.session_id.as_str().to_string(),
                parent_run_id: context
                    .parent_run_id
                    .as_ref()
                    .map(|run_id| run_id.as_str().to_string()),
                parent_agent_id: context
                    .parent_agent_id
                    .as_ref()
                    .map(|agent_id| agent_id.as_str().to_string()),
                cancelled: context.cancellation.is_cancelled(),
            });
        Ok(BrowseDataLaunchResult::completed(self.response.clone()))
    }
}

#[test]
fn browse_data_runtime_tool_is_constructible_without_plugin_context() {
    let tool = BrowseDataRuntimeTool::with_launcher(Arc::new(MockBrowseDataLauncher {
        response: "ok".to_string(),
        snapshots: Arc::new(Mutex::new(Vec::new())),
    }));

    assert_eq!(tool.definition().id, "browse_data");
}

#[derive(Debug)]
struct AskBrowseDataLauncher {
    message: String,
}

#[async_trait]
impl BrowseDataLauncher for AskBrowseDataLauncher {
    async fn launch(
        &self,
        _request: BrowseDataLaunchRequest,
        _context: BrowseDataLaunchContext,
    ) -> Result<BrowseDataLaunchResult> {
        Ok(BrowseDataLaunchResult::ask(
            app_lib::runtime::tools::permission::PermissionDecision::Ask {
                message: self.message.clone(),
                suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
                reason: app_lib::runtime::tools::permission::PermissionReason::UnknownScope,
            },
        ))
    }
}

#[tokio::test]
async fn browse_data_runtime_tool_passes_parent_identity_to_launcher() {
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let tool = BrowseDataRuntimeTool::with_launcher(Arc::new(MockBrowseDataLauncher {
        response: "browser agent ok".to_string(),
        snapshots: snapshots.clone(),
    }));
    let parent_cancel = CancellationToken::new();
    let ctx = ToolExecutionContext::new(
        SessionId::new("plan-e-conv"),
        RunId::new("run-e6"),
        Some(AgentId::new("agent-e6")),
        "tc-browse-data",
        parent_cancel.child_token(),
    );

    let result = tool
        .execute(
            json!({
                "task": "抓取订单列表",
                "url": "https://example.com/orders",
            }),
            ctx,
        )
        .await
        .expect("browse_data runtime tool should call launcher");

    assert_eq!(result.content, "browser agent ok");

    let captured = snapshots
        .lock()
        .expect("mutex poisoned")
        .clone()
        .pop()
        .expect("launcher should record one request");
    assert_eq!(captured.task, "抓取订单列表");
    assert_eq!(
        captured.url.as_deref(),
        Some("https://example.com/orders")
    );
    assert_eq!(captured.session_id, "plan-e-conv");
    assert_eq!(captured.parent_run_id.as_deref(), Some("run-e6"));
    assert_eq!(captured.parent_agent_id.as_deref(), Some("agent-e6"));
    assert!(!captured.cancelled, "fresh child token should not start cancelled");
}

#[tokio::test]
async fn browse_data_runtime_tool_propagates_parent_cancellation_to_launcher() {
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let tool = BrowseDataRuntimeTool::with_launcher(Arc::new(MockBrowseDataLauncher {
        response: "cancelled".to_string(),
        snapshots: snapshots.clone(),
    }));
    let parent_cancel = CancellationToken::new();
    let exec_cancel = parent_cancel.child_token();
    parent_cancel.cancel();
    let ctx = ToolExecutionContext::new(
        SessionId::new("plan-e-conv"),
        RunId::new("run-e6"),
        None,
        "tc-browse-data-cancelled",
        exec_cancel,
    );

    let _ = tool
        .execute(json!({"task": "抓取取消态任务"}), ctx)
        .await
        .expect("launcher should still be called for cancellation inspection");

    let captured = snapshots
        .lock()
        .expect("mutex poisoned")
        .clone()
        .pop()
        .expect("launcher should record one request");
    assert!(
        captured.cancelled,
        "browse_data launcher must receive the parent-linked cancellation token"
    );
    assert_eq!(captured.parent_run_id.as_deref(), Some("run-e6"));
}

#[tokio::test]
async fn browse_data_runtime_tool_preserves_structured_ask_required() {
    let tool = BrowseDataRuntimeTool::with_launcher(Arc::new(AskBrowseDataLauncher {
        message: "browse_data needs confirmation".to_string(),
    }));
    let ctx = ToolExecutionContext::new(
        SessionId::new("plan-e-conv"),
        RunId::new("run-e7"),
        Some(AgentId::new("agent-e7")),
        "tc-browse-data-ask",
        CancellationToken::new(),
    );

    let err = tool
        .execute(json!({ "task": "抓取需要授权的数据" }), ctx)
        .await
        .expect_err("browse_data runtime tool should surface structured ask");

    match err {
        ToolError::AskRequired(app_lib::runtime::tools::permission::PermissionDecision::Ask {
            message,
            suggestions,
            ..
        }) => {
            assert_eq!(message, "browse_data needs confirmation");
            assert_eq!(suggestions, vec!["Allow once".to_string(), "Deny".to_string()]);
        }
        other => panic!("expected ask-required error, got: {other:?}"),
    }
}
