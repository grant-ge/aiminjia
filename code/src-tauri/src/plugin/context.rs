//! Plugin context — shared services available to all plugins.

use std::path::PathBuf;
use std::sync::Arc;

use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;
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
}
