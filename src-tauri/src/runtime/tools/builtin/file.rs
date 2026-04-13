//! load_file as RuntimeTool.
//!
//! `LoadFileRuntimeTool` does NOT accept a `PluginContext` at the call site.
//! All dependencies are injected at construction time via `LoadFileDeps`.
//!
//! # TRANSITIONAL bridge
//!
//! `handle_load_file` in `llm/tool_executor/file_load.rs` still accepts
//! `&PluginContext`.  During this transition window we reconstruct a minimal
//! `PluginContext` from `LoadFileDeps` fields inside `execute()`.
//!
//! When `handle_load_file` is refactored to accept `LoadFileDeps` (or its
//! constituent parameters) directly, delete the `build_plugin_ctx` helper
//! and call the new function instead.  The `LoadFileDeps` struct itself
//! will survive that refactor — its fields map 1-to-1 to what the handler needs.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::python::session::PythonSessionManager;
use crate::runtime::ids::RunId;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

// ── LoadFileDeps ─────────────────────────────────────────────────────────────

/// Narrow load_file dependencies — injected at construction.
///
/// TRANSITIONAL: When `handle_load_file` is refactored to not need
/// `PluginContext`, this struct's fields will map 1:1 to that function's
/// parameters and the bridge below can be removed.
pub struct LoadFileDeps {
    pub storage: Arc<AppStorage>,
    pub file_manager: Arc<FileManager>,
    pub workspace_path: PathBuf,
    pub session_manager: Arc<PythonSessionManager>,
    pub conversation_id: String,
    pub run_id: Option<RunId>,
}

// ── LoadFileRuntimeTool ───────────────────────────────────────────────────────

pub struct LoadFileRuntimeTool {
    deps: Arc<LoadFileDeps>,
}

impl LoadFileRuntimeTool {
    pub fn new(deps: LoadFileDeps) -> Self {
        Self { deps: Arc::new(deps) }
    }
}

#[async_trait]
impl RuntimeTool for LoadFileRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("load_file")
            .cloned()
            .unwrap_or_else(|| {
                ToolDefinition::new(
                    "load_file",
                    "加载已上传文件，使数据可在 execute_python 中以 _df/_text 变量使用",
                )
            })
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        // TRANSITIONAL: construct minimal PluginContext from LoadFileDeps for legacy handler.
        // Remove this bridge when handle_load_file accepts LoadFileDeps directly.
        #[allow(deprecated)]
        let plugin_ctx = build_plugin_ctx(&self.deps, &ctx);

        let content = crate::llm::tool_executor::handle_load_file(&plugin_ctx, &input)
            .await
            .map_err(ToolError::Other)?;

        Ok(ToolResult {
            tool_name: "load_file".to_string(),
            content: content.clone(),
            data: Some(Value::String(content)),
        })
    }
}

// ── TRANSITIONAL bridge helper ────────────────────────────────────────────────

/// Build a minimal `PluginContext` from `LoadFileDeps` for the legacy handler.
///
/// Only the fields consumed by `handle_load_file` are populated:
///   `storage`, `file_manager`, `workspace_path`, `conversation_id`,
///   `session_id`, `run_id`.
///
/// All other fields are set to harmless defaults / None.
/// `app_handle` is always None — runtime/ must not import tauri::.
///
/// TRANSITIONAL: Delete this function when `handle_load_file` is refactored
/// to accept `LoadFileDeps` (or equivalent parameters) directly.
#[allow(deprecated)]
fn build_plugin_ctx(
    deps: &LoadFileDeps,
    ctx: &ToolExecutionContext,
) -> crate::plugin::context::PluginContext {
    use crate::plugin::context::PluginContext;

    PluginContext {
        storage: deps.storage.clone(),
        file_manager: deps.file_manager.clone(),
        workspace_path: deps.workspace_path.clone(),
        conversation_id: deps.conversation_id.clone(),
        session_id: ctx.session_id.clone(),
        run_id: deps.run_id.clone(),
        agent_id: ctx.agent_id.clone(),
        tavily_api_key: None,
        bocha_api_key: None,
        // TRANSITIONAL: app_handle is None here because runtime/ must not import tauri::.
        // PythonRunner::with_config(path, config, None) falls back to default signal handling.
        // When handle_load_file is refactored to accept LoadFileDeps directly, this restriction
        // can be addressed via RuntimeHost trait injection instead.
        app_handle: None,
        // session_manager is used indirectly through PythonRunner in handle_load_file
        session_manager: deps.session_manager.clone(),
        auth_manager: None,
        connector_engine: None,
        use_cloud: false,
        model: String::new(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        authorized_workspace: None,
    }
}
