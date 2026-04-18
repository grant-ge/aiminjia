use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use crate::python::runner::{ExecutionResult, PythonRunner};
use crate::python::sandbox::SandboxConfig;
use crate::python::session::PythonSessionManager;
use crate::runtime::ids::RunId;

#[async_trait]
pub trait PythonExecution: Send + Sync + std::fmt::Debug {
    async fn execute_oneshot(
        &self,
        workspace_path: &Path,
        code: &str,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult>;

    async fn execute_for_run(
        &self,
        run_id: &RunId,
        code: &str,
        timeout: Duration,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult>;

    async fn interrupt_run(&self, run_id: &RunId) -> Result<()>;
}

pub struct DefaultPythonExecution {
    session_manager: Arc<PythonSessionManager>,
    python_binary: PathBuf,
    python_home: Option<PathBuf>,
}

impl DefaultPythonExecution {
    pub fn new(
        session_manager: Arc<PythonSessionManager>,
        python_binary: PathBuf,
        python_home: Option<PathBuf>,
    ) -> Self {
        Self {
            session_manager,
            python_binary,
            python_home,
        }
    }
}

impl std::fmt::Debug for DefaultPythonExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultPythonExecution")
            .field("python_binary", &self.python_binary)
            .field("python_home", &self.python_home)
            .finish()
    }
}

#[async_trait]
impl PythonExecution for DefaultPythonExecution {
    async fn execute_oneshot(
        &self,
        workspace_path: &Path,
        code: &str,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        let runner = PythonRunner::with_runtime(
            workspace_path.to_path_buf(),
            sandbox.clone(),
            self.python_binary.clone(),
            self.python_home.clone(),
        );
        runner.execute_raw(code).await
    }

    async fn execute_for_run(
        &self,
        run_id: &RunId,
        code: &str,
        timeout: Duration,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        let result = self
            .session_manager
            .execute_for_run(run_id, code, timeout, sandbox)
            .await?;
        Ok(result.result)
    }

    async fn interrupt_run(&self, run_id: &RunId) -> Result<()> {
        self.session_manager.interrupt_run(run_id).await
    }
}
