//! Python session manager stub — no-op implementation.
//!
//! The Python execution tools have been deleted. This stub provides a
//! compile-compatible `PythonSessionManager` with no-op methods so that
//! the call sites in chat.rs, worker_runtime.rs, conversation_service.rs,
//! etc. continue to compile without modification in this phase.
//!
//! A future cleanup pass should remove the remaining `session_manager` field
//! references from all non-Python code paths.

use std::path::PathBuf;
use crate::runtime::dependencies::ManagedRuntimeResolver;
use crate::runtime::ids::RunId;

/// No-op Python session manager stub.
///
/// All methods are async no-ops. The struct is Send + Sync so it can be
/// wrapped in `Arc<PythonSessionManager>` and managed as Tauri state.
pub struct PythonSessionManager;

impl PythonSessionManager {
    /// Create a new no-op session manager.
    pub fn new(_workspace_path: PathBuf, _resolver: Option<ManagedRuntimeResolver>) -> Self {
        Self
    }

    /// Create a session manager that lazily resolves the Python runtime.
    pub fn with_lazy_runtime_resolver(
        _workspace_path: PathBuf,
        _resolver: ManagedRuntimeResolver,
    ) -> Self {
        Self
    }

    /// Destroy sessions associated with a conversation (no-op).
    pub async fn destroy(&self, _conversation_id: &str) {}

    /// Destroy sessions associated with a run (no-op).
    pub async fn destroy_run(&self, _run_id: &RunId) {}

    /// Interrupt the active Python execution for a conversation (no-op).
    pub async fn interrupt(&self, _conversation_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Interrupt the active Python execution for a run (no-op).
    pub async fn interrupt_run(&self, _run_id: &RunId) -> anyhow::Result<()> {
        Ok(())
    }

    /// Reap idle sessions (no-op).
    pub async fn reap_idle(&self) {}

    /// Shut down all sessions (no-op).
    pub async fn shutdown_all(&self) {}
}
