//! execute_python as RuntimeTool.
//!
//! `ExecutePythonRuntimeTool` keeps static permission checks local, while the
//! actual execution semantics are delegated to the shared
//! `llm::tool_executor::python::handle_execute_python_core` helper so the
//! runtime path and the legacy ToolPlugin path do not drift.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use crate::runtime::ids::RunId;
use crate::runtime::tools::builtin::python_execution::PythonExecution;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

const DANGEROUS_PATTERNS: &[&str] = &[
    "__import__('os').system",
    "__import__('subprocess')",
    "subprocess.call",
    "subprocess.Popen",
    "os.system(",
    "os.popen(",
    "exec(compile(",
    "eval(compile(",
];

pub struct ExecutePythonRuntimeTool {
    stub_mode: bool,
    python: Option<Arc<dyn PythonExecution>>,
    storage: Option<Arc<AppStorage>>,
    file_manager: Option<Arc<FileManager>>,
    requested_run_id: Option<RunId>,
    model: Option<String>,
    python_binary: Option<PathBuf>,
    python_home: Option<PathBuf>,
    error_message: Option<String>,
}

impl ExecutePythonRuntimeTool {
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            python: None,
            storage: None,
            file_manager: None,
            requested_run_id: None,
            model: None,
            python_binary: None,
            python_home: None,
            error_message: None,
        }
    }

    pub fn with_runtime_deps(
        python: Arc<dyn PythonExecution>,
        storage: Arc<AppStorage>,
        file_manager: Arc<FileManager>,
        requested_run_id: Option<RunId>,
        model: String,
        python_binary: PathBuf,
        python_home: Option<PathBuf>,
    ) -> Self {
        Self {
            stub_mode: false,
            python: Some(python),
            storage: Some(storage),
            file_manager: Some(file_manager),
            requested_run_id,
            model: Some(model),
            python_binary: Some(python_binary),
            python_home,
            error_message: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            stub_mode: false,
            python: None,
            storage: None,
            file_manager: None,
            requested_run_id: None,
            model: None,
            python_binary: None,
            python_home: None,
            error_message: Some(message),
        }
    }

    #[cfg(test)]
    pub fn python_binary_path(&self) -> Option<&PathBuf> {
        self.python_binary.as_ref()
    }
}

#[async_trait]
impl RuntimeTool for ExecutePythonRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("execute_python")
            .unwrap_or_else(|| ToolDefinition::new("execute_python", "Execute Python code"))
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        let code = input.get("code").and_then(Value::as_str).unwrap_or("");
        for pattern in DANGEROUS_PATTERNS {
            if code.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: format!("execute_python: dangerous pattern detected: '{}'", pattern),
                    reason: PermissionReason::Other("static_code_check".into()),
                });
            }
        }
        None
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(message) = &self.error_message {
            return Err(ToolError::ExecutionFailed(message.clone()));
        }

        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: stub mode, real execution not available".into(),
            ));
        }

        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: execution cancelled before start".into(),
            ));
        }

        let python = self.python.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing PythonExecution dependency".into(),
            )
        })?;
        let storage = self.storage.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing storage dependency".into(),
            )
        })?;
        let file_manager = self.file_manager.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing file_manager dependency".into(),
            )
        })?;
        let capability = ctx.capability.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing capability context".into(),
            )
        })?;
        let storage_cap = capability.storage.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing workspace capability".into(),
            )
        })?;

        let params = crate::llm::tool_executor::ExecutePythonCoreParams {
            storage,
            file_manager,
            workspace_path: &storage_cap.workspace_path,
            authorized_workspace: storage_cap.authorized_workspace.as_ref(),
            conversation_id: ctx.session_id.as_str(),
            requested_run_id: self.requested_run_id.as_ref(),
            model: self.model.as_deref().unwrap_or("unknown"),
            python_binary: self.python_binary.clone(),
            python_home: self.python_home.clone(),
            extra_read_paths: storage_cap
                .permission_ctx
                .additional_working_dirs
                .keys()
                .cloned()
                .collect(),
        };
        let content =
            crate::llm::tool_executor::handle_execute_python_core(&params, &input, python.as_ref())
                .await
                .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        Ok(ToolResult::new("execute_python", content, None))
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutePythonRuntimeTool;
    use crate::python::session::PythonSessionManager;
    use crate::runtime::tools::builtin::python_execution::DefaultPythonExecution;
    use crate::storage::file_manager::FileManager;
    use crate::storage::file_store::AppStorage;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn execute_python_runtime_tool_exposes_python_binary() {
        let workspace = tempfile::TempDir::new().expect("tempdir should exist");
        let session_manager = Arc::new(PythonSessionManager::new(
            workspace.path().to_path_buf(),
            None,
        ));
        let binary = PathBuf::from("/usr/bin/python3");
        let python = Arc::new(DefaultPythonExecution::new(
            session_manager,
            binary.clone(),
            None,
        ));
        let storage = Arc::new(AppStorage::new(workspace.path()).expect("storage should init"));
        let file_manager = Arc::new(FileManager::new(workspace.path()));

        let tool = ExecutePythonRuntimeTool::with_runtime_deps(
            python,
            storage,
            file_manager,
            None,
            "test-model".to_string(),
            binary.clone(),
            None,
        );

        assert_eq!(tool.python_binary_path(), Some(&binary));
    }
}
