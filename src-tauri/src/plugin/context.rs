//! Plugin context — shared services available to all plugins.

use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::AuthManager;
use crate::connector::ConnectorEngine;
use crate::llm::gateway::LlmGateway;
use crate::models::settings::AppSettings;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;
use crate::plugin::registry::ToolRegistry;
use crate::python::session::PythonSessionManager;

/// Shared service context passed to every plugin execution.
///
/// All fields are `Arc`-wrapped or cheaply clonable, making it safe
/// to share across concurrent tool executions via `Clone`.
#[derive(Clone)]
pub struct PluginContext {
    pub storage: Arc<AppStorage>,
    pub file_manager: Arc<FileManager>,
    pub workspace_path: PathBuf,
    pub conversation_id: String,
    pub tavily_api_key: Option<String>,
    pub bocha_api_key: Option<String>,
    pub app_handle: Option<tauri::AppHandle>,
    pub session_manager: Arc<PythonSessionManager>,
    pub auth_manager: Option<Arc<AuthManager>>,
    pub connector_engine: Option<Arc<ConnectorEngine>>,
    pub use_cloud: bool,
    pub model: String,
    /// LLM gateway for sub-agent execution (delegation tools).
    pub gateway: Option<Arc<LlmGateway>>,
    /// Tool registry for sub-agent tool execution.
    pub tool_registry: Option<Arc<ToolRegistry>>,
    /// App settings for sub-agent LLM calls.
    pub app_settings: Option<Arc<AppSettings>>,
}
