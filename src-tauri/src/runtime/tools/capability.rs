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

use std::path::PathBuf;
use std::sync::Arc;

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

/// Capability-scoped context attached optionally to a [`crate::runtime::tools::ToolExecutionContext`].
///
/// New `RuntimeTool` implementations should access services through this struct
/// rather than through `PluginContext`.  Fields are intentionally limited:
///
/// | Field           | Purpose                                          |
/// |-----------------|--------------------------------------------------|
/// | `storage`       | Workspace path and scoped file-I/O helpers       |
/// | `workspace_id`  | Logical workspace identifier for key-scoping     |
///
/// Fields that are *not* present here (e.g. `gateway`, `auth_manager`,
/// `agent_runtime`) must be accessed via dedicated orchestration APIs, not
/// by widening this struct.
#[derive(Clone, Debug)]
pub struct CapabilityContext {
    /// Scoped storage capability (workspace path, etc.).
    pub storage: Option<StorageCapability>,
    /// Logical workspace / conversation scope identifier.
    pub workspace_id: Option<String>,
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
        }
    }
}

/// Type alias for the Arc-wrapped capability context threaded through
/// [`crate::runtime::tools::ToolExecutionContext`].
pub type SharedCapabilityContext = Arc<CapabilityContext>;
