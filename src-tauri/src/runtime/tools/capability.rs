//! Capability-scoped context for runtime tools.
//!
//! `CapabilityContext` is the *narrow* view of system services that a new
//! [`crate::runtime::tools::RuntimeTool`] implementation may request.  It
//! deliberately exposes far less than the legacy [`crate::plugin::context::PluginContext`]:
//!
//! - No `LlmGateway`, no `AgentRuntime`, no `AuthManager` — those are
//!   orchestration concerns that belong above the tool layer.
//! - No raw `AppStorage` arc — instead, only a scoped `StorageCapability`
//!   that surfaces the fields a tool actually needs.
//!
//! This enforces the architectural boundary introduced in Phase 2 Task 3.
//! Legacy tools continue to receive the full `PluginContext` through the
//! [`crate::runtime::tools::LegacyToolAdapter`] bridge.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use std::num::NonZeroUsize;
use std::sync::Mutex;

/// Storage-related capability subset exposed to runtime tools.
///
/// Contains only the fields a tool legitimately needs for file I/O and
/// workspace resolution — not the full [`crate::storage::file_store::AppStorage`].
#[derive(Clone, Debug)]
pub struct StorageCapability {
    /// Absolute path to the active workspace directory.
    pub workspace_path: PathBuf,
    /// Authorized local directory for this session (if any).
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
}

#[derive(Clone, Debug)]
pub struct FileState {
    pub content: String,
    pub mtime_secs: u64,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug)]
pub struct FileStateCache {
    cache: Mutex<lru::LruCache<PathBuf, FileState>>,
}

impl FileStateCache {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(lru::LruCache::new(
                NonZeroUsize::new(100).expect("FileStateCache capacity must be non-zero"),
            )),
        }
    }

    pub fn from_other(other: &FileStateCache) -> Self {
        let mut cloned = lru::LruCache::new(
            NonZeroUsize::new(100).expect("FileStateCache capacity must be non-zero"),
        );
        let guard = other
            .cache
            .lock()
            .expect("FileStateCache mutex poisoned");
        for (path, state) in guard.iter() {
            cloned.put(path.clone(), state.clone());
        }
        Self {
            cache: Mutex::new(cloned),
        }
    }

    pub fn clone_for_child(&self) -> Arc<FileStateCache> {
        Arc::new(Self::from_other(self))
    }

    pub fn get(&self, path: &Path) -> Option<FileState> {
        self.cache
            .lock()
            .expect("FileStateCache mutex poisoned")
            .get(path)
            .cloned()
    }

    pub fn set(&self, path: PathBuf, state: FileState) {
        self.cache
            .lock()
            .expect("FileStateCache mutex poisoned")
            .put(path, state);
    }
}

impl Default for FileStateCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct FileReadingLimits {
    pub max_size_bytes: usize,
}

impl Default for FileReadingLimits {
    fn default() -> Self {
        Self {
            max_size_bytes: 1_048_576,
        }
    }
}

pub trait NotificationSink: Send + Sync + std::fmt::Debug {
    fn notify(&self, message: &str);
}

// ── FileOperations trait ─────────────────────────────────────────────────────

/// Result returned by [`FileOperations::load_file`].
#[derive(Clone, Debug)]
pub struct LoadedFileResult {
    /// User-visible JSON payload returned to the LLM/runtime.
    pub content: String,
}

/// Narrow file-loading capability exposed to runtime tools.
///
/// This trait decouples `LoadFileRuntimeTool` from `PluginContext` and
/// `handle_load_file`'s raw infrastructure dependencies (`AppStorage`,
/// `FileManager`, `PythonSessionManager`).  The runtime tool calls methods
/// on this trait instead of rebuilding a `PluginContext`.
///
/// `DefaultFileOperations` is the production implementation; tests can
/// substitute a mock.
#[async_trait]
pub trait FileOperations: Send + Sync + std::fmt::Debug {
    /// Load a file using the raw tool arguments.
    ///
    /// This preserves the current `load_file` surface (`file_id`, optional
    /// `sheet`, optional `nrows`) without widening `ToolExecutionContext`.
    async fn load_file(&self, args: &serde_json::Value) -> anyhow::Result<LoadedFileResult>;

    /// Return whether this file is already loaded for the given scope.
    fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool;

    /// Absolute workspace path used by this file-operations instance.
    fn workspace_path(&self) -> &Path;
}

/// Capability-scoped context attached optionally to a [`crate::runtime::tools::ToolExecutionContext`].
///
/// New `RuntimeTool` implementations should access services through this struct
/// rather than through `PluginContext`.  Fields are intentionally limited:
///
/// | Field               | Purpose                                          |
/// |---------------------|--------------------------------------------------|
/// | `storage`           | Workspace path and scoped file-I/O helpers       |
/// | `workspace_id`      | Logical workspace identifier for key-scoping     |
/// | `browser_available` | Whether a browser connector is active            |
/// | `file_ops`          | File loading operations (load_file)              |
///
/// Fields that are *not* present here (e.g. `gateway`, `auth_manager`,
/// `agent_runtime`) must be accessed via dedicated orchestration APIs, not
/// by widening this struct.
#[derive(Clone)]
pub struct CapabilityContext {
    /// Scoped storage capability (workspace path, etc.).
    pub storage: Option<StorageCapability>,
    /// Logical workspace / conversation scope identifier.
    pub workspace_id: Option<String>,
    /// Whether a browser connector is available for this session.
    /// Set to true when a ConnectorEngine is active and ready.
    /// Kept as a plain bool to avoid importing ConnectorEngine into runtime/.
    pub browser_available: bool,
    /// File loading operations accessor.
    ///
    /// When present, `LoadFileRuntimeTool` uses this instead of rebuilding a
    /// `PluginContext`.  `None` for paths that don't require file loading
    /// (workspace tools, browser tools, tests).
    pub file_ops: Option<Arc<dyn FileOperations>>,
    /// Optional cache of recent file-read state keyed by absolute path.
    pub read_file_state: Option<Arc<FileStateCache>>,
    /// Optional limits for file-reading operations.
    pub file_reading_limits: Option<FileReadingLimits>,
    /// Optional sink for user-visible notifications emitted by runtime tools.
    pub notification_sink: Option<Arc<dyn NotificationSink>>,
    /// Whether this execution is running inside a subagent / child context.
    pub is_subagent: bool,
}

impl std::fmt::Debug for CapabilityContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityContext")
            .field("storage", &self.storage)
            .field("workspace_id", &self.workspace_id)
            .field("browser_available", &self.browser_available)
            .field("file_ops", &self.file_ops.as_ref().map(|f| format!("{:?}", f)))
            .field(
                "read_file_state",
                &self.read_file_state.as_ref().map(|_| "<FileStateCache>"),
            )
            .field("file_reading_limits", &self.file_reading_limits)
            .field(
                "notification_sink",
                &self.notification_sink.as_ref().map(|_| "<NotificationSink>"),
            )
            .field("is_subagent", &self.is_subagent)
            .finish()
    }
}

impl CapabilityContext {
    /// Create a minimal capability context with just a workspace path.
    pub fn with_workspace(workspace_path: PathBuf, workspace_id: impl Into<String>) -> Self {
        Self {
            storage: Some(StorageCapability {
                workspace_path,
                authorized_workspace: None,
            }),
            workspace_id: Some(workspace_id.into()),
            browser_available: false,
            file_ops: None,
            read_file_state: None,
            file_reading_limits: None,
            notification_sink: None,
            is_subagent: false,
        }
    }

    /// Mark this context as having an active browser connector.
    pub fn with_browser(mut self) -> Self {
        self.browser_available = true;
        self
    }

    pub fn with_read_file_state(mut self, read_file_state: Arc<FileStateCache>) -> Self {
        self.read_file_state = Some(read_file_state);
        self
    }

    pub fn with_file_reading_limits(mut self, file_reading_limits: FileReadingLimits) -> Self {
        self.file_reading_limits = Some(file_reading_limits);
        self
    }

    pub fn with_notification_sink(
        mut self,
        notification_sink: Arc<dyn NotificationSink>,
    ) -> Self {
        self.notification_sink = Some(notification_sink);
        self
    }

    pub fn with_subagent(mut self, is_subagent: bool) -> Self {
        self.is_subagent = is_subagent;
        self
    }

    /// Returns true if a browser connector capability is active.
    pub fn has_browser_capability(&self) -> bool {
        self.browser_available
    }
}

/// Type alias for the Arc-wrapped capability context threaded through
/// [`crate::runtime::tools::ToolExecutionContext`].
pub type SharedCapabilityContext = Arc<CapabilityContext>;

// ── DefaultFileOperations ────────────────────────────────────────────────────

/// Production [`FileOperations`] implementation.
///
/// Wraps the concrete infrastructure deps (`AppStorage`, `FileManager`,
/// `PythonSessionManager`) and delegates to
/// [`crate::llm::tool_executor::file_load::handle_load_file_core`].
///
/// `app_handle` is `None` because `runtime/` must not import `tauri::` — this
/// is identical to the previous `build_plugin_ctx` bridge behaviour.  When
/// `RuntimeHost` trait injection is implemented, the app handle can be threaded
/// through that mechanism instead.
pub struct DefaultFileOperations {
    pub(crate) storage: Arc<crate::storage::file_store::AppStorage>,
    pub(crate) file_manager: Arc<crate::storage::file_manager::FileManager>,
    pub(crate) workspace_path: PathBuf,
    pub(crate) conversation_id: String,
    pub(crate) run_id: Option<crate::runtime::ids::RunId>,
    pub(crate) python_binary: Option<PathBuf>,
    pub(crate) python_home: Option<PathBuf>,
}

impl DefaultFileOperations {
    /// Construct a `DefaultFileOperations` from its constituent parts.
    ///
    /// This constructor is the canonical way to build a `DefaultFileOperations`
    /// outside of `runtime/` (e.g. in integration tests or transport glue code).
    /// Fields are `pub(crate)` so that internal modules can read them, but external
    /// callers must go through this constructor to ensure all dependencies are
    /// wired correctly.
    pub fn new(
        storage: Arc<crate::storage::file_store::AppStorage>,
        file_manager: Arc<crate::storage::file_manager::FileManager>,
        workspace_path: PathBuf,
        conversation_id: impl Into<String>,
        run_id: Option<crate::runtime::ids::RunId>,
    ) -> Self {
        Self {
            storage,
            file_manager,
            workspace_path,
            conversation_id: conversation_id.into(),
            run_id,
            python_binary: None,
            python_home: None,
        }
    }
}

impl std::fmt::Debug for DefaultFileOperations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultFileOperations")
            .field("workspace_path", &self.workspace_path)
            .field("conversation_id", &self.conversation_id)
            .field("run_id", &self.run_id)
            .field("python_binary", &self.python_binary)
            .field("python_home", &self.python_home)
            .finish()
    }
}

#[async_trait]
impl FileOperations for DefaultFileOperations {
    async fn load_file(&self, args: &serde_json::Value) -> anyhow::Result<LoadedFileResult> {
        use crate::llm::tool_executor::file_load::{handle_load_file_core, LoadFileParams};

        let params = LoadFileParams {
            storage: &self.storage,
            file_manager: &self.file_manager,
            workspace_path: &self.workspace_path,
            conversation_id: &self.conversation_id,
            run_id: self.run_id.as_ref(),
            python_binary: self.python_binary.clone(),
            python_home: self.python_home.clone(),
        };
        let content = handle_load_file_core(&params, args).await?;
        Ok(LoadedFileResult { content })
    }

    fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool {
        let key = format!("loaded:{}:{}", scope_id, file_id);
        matches!(self.storage.get_memory(&key), Ok(Some(_)))
    }

    fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }
}
