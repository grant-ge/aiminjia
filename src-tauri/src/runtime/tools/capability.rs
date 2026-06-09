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

use std::num::NonZeroUsize;
use std::sync::Mutex;

use crate::runtime::path_auth::ToolPermissionContext;

use crate::runtime::dependencies::{
    RuntimeDependencyError, RuntimeDependencyResult, RuntimeResolver, WorkspaceDependencies,
};

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
    /// Per-turn path authorization context built from PermissionStore + session
    /// attachment directories.  Wrapped in `Arc` so sub-agents can clone cheaply.
    /// Phase 4 will switch read/write/list/search/sandbox tools to consult this;
    /// in Phase 3 it is carried but not yet read by tools.
    pub permission_ctx: Arc<ToolPermissionContext>,
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
        let guard = other.cache.lock().expect("FileStateCache mutex poisoned");
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

/// Narrow sink for long-running tools (Bash / PowerShell) to push
/// incremental stdout/stderr snapshots while a single tool call is still
/// executing.
///
/// **Architecture note**: tools must not reach into `RuntimeEventBus`
/// directly (`CapabilityContext` is a deliberately narrow interface, see
/// `CLAUDE.md` §4). The orchestration layer injects a concrete impl that
/// translates `on_progress` into `RuntimeEventKind::ToolProgress` and
/// forwards it through the bus.
///
/// **Throttling**: this trait makes no throttling guarantees — the caller
/// (typically a bash spawner) is responsible for coalescing rapid updates
/// to ~500ms so the IPC bus doesn't melt. The default implementation
/// (`BusBackedToolProgressSink`) accepts every call.
pub trait ToolProgressSink: Send + Sync + std::fmt::Debug {
    /// Push a progress snapshot for the currently executing tool call.
    ///
    /// `stdout_tail` should be the most recent N lines of merged stdout +
    /// stderr (UTF-8 safe). `total_bytes` is the total bytes captured so
    /// far (lets the UI show "已收到 N KB" without keeping its own counter).
    fn on_progress(&self, tool_call_id: &str, stdout_tail: &str, total_bytes: u64);
}

// ── FileOperations trait ─────────────────────────────────────────────────────

/// Narrow file-operations capability exposed to runtime tools.
///
/// The `load_file` method was removed in Phase 3 of the tool cleanup.
/// The trait is retained because `CapabilityContext` and `QueryEngine` still
/// carry an `Option<Arc<dyn FileOperations>>` field for workspace path
/// resolution.
pub trait FileOperations: Send + Sync + std::fmt::Debug {
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
/// | `file_ops`          | File operations stub (retained for compile compat) |
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
    /// Runtime dependency resolver exposed as a narrow interface for tools.
    pub runtime_resolver: Option<Arc<dyn RuntimeResolver>>,
    /// File loading operations accessor.
    ///
    /// When present, `LoadFileRuntimeTool` uses this instead of rebuilding a
    /// `PluginContext`.  `None` for paths that don't require file loading
    /// (workspace tools, tests).
    pub file_ops: Option<Arc<dyn FileOperations>>,
    /// Optional cache of recent file-read state keyed by absolute path.
    pub read_file_state: Option<Arc<FileStateCache>>,
    /// Optional limits for file-reading operations.
    pub file_reading_limits: Option<FileReadingLimits>,
    /// Optional sink for user-visible notifications emitted by runtime tools.
    pub notification_sink: Option<Arc<dyn NotificationSink>>,
    /// Optional sink for incremental tool progress (live stdout/stderr).
    /// `None` for legacy/test paths or short-lived tools that don't stream.
    pub tool_progress_sink: Option<Arc<dyn ToolProgressSink>>,
    /// Whether this execution is running inside a subagent / child context.
    pub is_subagent: bool,
}

impl std::fmt::Debug for CapabilityContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityContext")
            .field("storage", &self.storage)
            .field("workspace_id", &self.workspace_id)
            .field(
                "runtime_resolver",
                &self.runtime_resolver.as_ref().map(|_| "<RuntimeResolver>"),
            )
            .field(
                "file_ops",
                &self.file_ops.as_ref().map(|f| format!("{:?}", f)),
            )
            .field(
                "read_file_state",
                &self.read_file_state.as_ref().map(|_| "<FileStateCache>"),
            )
            .field("file_reading_limits", &self.file_reading_limits)
            .field(
                "notification_sink",
                &self
                    .notification_sink
                    .as_ref()
                    .map(|_| "<NotificationSink>"),
            )
            .field(
                "tool_progress_sink",
                &self
                    .tool_progress_sink
                    .as_ref()
                    .map(|_| "<ToolProgressSink>"),
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
                permission_ctx: Arc::new(ToolPermissionContext::empty()),
            }),
            workspace_id: Some(workspace_id.into()),
            runtime_resolver: None,
            file_ops: None,
            read_file_state: None,
            file_reading_limits: None,
            notification_sink: None,
            tool_progress_sink: None,
            is_subagent: false,
        }
    }

    pub fn with_runtime_resolver(mut self, runtime_resolver: Arc<dyn RuntimeResolver>) -> Self {
        self.runtime_resolver = Some(runtime_resolver);
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

    pub fn with_notification_sink(mut self, notification_sink: Arc<dyn NotificationSink>) -> Self {
        self.notification_sink = Some(notification_sink);
        self
    }

    pub fn with_tool_progress_sink(mut self, sink: Arc<dyn ToolProgressSink>) -> Self {
        self.tool_progress_sink = Some(sink);
        self
    }

    pub fn with_subagent(mut self, is_subagent: bool) -> Self {
        self.is_subagent = is_subagent;
        self
    }

    pub fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        self.runtime_resolver
            .as_ref()
            .ok_or_else(|| {
                RuntimeDependencyError::ResolverUnavailable(
                    "CapabilityContext has no RuntimeResolver".to_string(),
                )
            })?
            .workspace_dependencies()
    }
}

/// Type alias for the Arc-wrapped capability context threaded through
/// [`crate::runtime::tools::ToolExecutionContext`].
pub type SharedCapabilityContext = Arc<CapabilityContext>;

// ── DefaultFileOperations ────────────────────────────────────────────────────

/// Production [`FileOperations`] stub implementation.
///
/// The `load_file` tool has been removed in Phase 3 of the tool cleanup.
/// This struct is retained as a no-op stub so that existing construction sites
/// (worker_runtime.rs, legacy registry) continue to compile.
pub struct DefaultFileOperations {
    pub(crate) workspace_path: PathBuf,
    pub(crate) conversation_id: String,
    pub(crate) run_id: Option<crate::runtime::ids::RunId>,
    pub(crate) python_binary: Option<PathBuf>,
    pub(crate) python_home: Option<PathBuf>,
}

impl DefaultFileOperations {
    pub fn new(
        workspace_path: PathBuf,
        conversation_id: impl Into<String>,
        run_id: Option<crate::runtime::ids::RunId>,
    ) -> Self {
        Self {
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

impl FileOperations for DefaultFileOperations {
    fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }
}
