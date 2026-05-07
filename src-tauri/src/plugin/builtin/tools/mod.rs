//! Built-in RuntimeTool registration.
//!
//! All `ToolPlugin`-based builtin tools have been removed (see git history
//! before commit chore/cleanup-deprecated). The current implementation
//! registers RuntimeTools directly via `register_runtime()` against the
//! ToolDispatcher path. The legacy registry.execute() / PluginContext path
//! remains for MCP / python_bridge but is no longer exercised by builtins.

#![allow(deprecated)]

pub mod echo_runtime;

use crate::plugin::ToolRegistry;
use std::sync::Arc;

/// Register all built-in RuntimeTools onto the dispatcher.
pub async fn register_builtin_tools(registry: &ToolRegistry) {
    use crate::runtime::tools::builtin::ask_user_question::AskUserQuestionRuntimeTool;
    #[cfg(not(windows))]
    use crate::runtime::tools::builtin::bash::BashTool;
    use crate::runtime::tools::builtin::grep::GrepContentTool;
    #[cfg(windows)]
    use crate::runtime::tools::builtin::powershell::PowerShellTool;
    use crate::runtime::tools::builtin::task_tools::{
        TaskCreateRuntimeTool, TaskListRuntimeTool, TaskUpdateRuntimeTool,
    };
    use crate::runtime::tools::builtin::workspace::{
        EditFileRuntimeTool, GetFileInfoRuntimeTool,
        ReadWorkspaceFileRuntimeTool, SearchFilesRuntimeTool, WriteFileRuntimeTool,
    };
    registry
        .register_runtime(Arc::new(ReadWorkspaceFileRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(SearchFilesRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(GetFileInfoRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(WriteFileRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(EditFileRuntimeTool))
        .await;
    #[cfg(not(windows))]
    registry.register_runtime(Arc::new(BashTool)).await;
    #[cfg(windows)]
    registry.register_runtime(Arc::new(PowerShellTool)).await;
    registry.register_runtime(Arc::new(GrepContentTool)).await;
    registry
        .register_runtime(Arc::new(AskUserQuestionRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(TaskCreateRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(TaskUpdateRuntimeTool))
        .await;
    registry
        .register_runtime(Arc::new(TaskListRuntimeTool))
        .await;
    registry.validate_catalog_consistency().await;
}
