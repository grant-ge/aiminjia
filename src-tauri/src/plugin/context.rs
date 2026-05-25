//! Plugin context — shared services available to all plugins.
//!
//! # ⚠ Legacy service locator — do not use in new tools
//!
//! [`PluginContext`] is a **legacy, full-service-locator context** that pre-dates
//! the capability-boundary architecture introduced in Phase 2 of the runtime
//! refactor.  It bundles *every* system service (storage, LLM gateway, agent
//! runtime, auth, connectors, …) into one struct and hands the whole thing to
//! each tool.  This is an anti-pattern that makes it hard to reason about what
//! a tool actually needs and couples tool code to orchestration infrastructure.
//!
//! ## What to use instead
//!
//! | Use case | Preferred type |
//! |---|---|
//! | New runtime tool (identity + cancellation) | [`crate::runtime::tools::ToolExecutionContext`] |
//! | Scoped services (workspace, storage) | [`crate::runtime::tools::CapabilityContext`] via `ctx.capability` |
//! | Bridging a legacy `ToolPlugin` impl | [`crate::runtime::tools::LegacyToolAdapter::from_plugin`] |
//!
//! ## Migration guide
//!
//! 1. Implement [`crate::runtime::tools::RuntimeTool`] instead of
//!    [`crate::plugin::tool_trait::ToolPlugin`].
//! 2. Replace `&PluginContext` parameters with `ToolExecutionContext`.
//! 3. Access workspace/storage data via `ctx.capability` (a
//!    [`crate::runtime::tools::CapabilityContext`]).
//! 4. Services that are not in `CapabilityContext` (e.g. `LlmGateway`,
//!    `AgentRuntime`, `AuthManager`) belong in dedicated orchestration helpers —
//!    do **not** add them to `CapabilityContext` just to port a field from here.
//!
//! ## Existing code
//!
//! All existing builtin tools continue to work through
//! [`crate::runtime::tools::LegacyToolAdapter`].  They do **not** need to be
//! migrated atomically; migrate tool-by-tool when touching each file.

use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::AuthManager;
use crate::connector::dingtalk::DingtalkBridge;
use crate::llm::gateway::LlmGateway;
use crate::models::settings::AppSettings;
use crate::plugin::registry::ToolRegistry;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::dependencies::ManagedRuntimeResolver;
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::ids::{AgentId, RunId, SessionId};
use crate::runtime::path_auth::ToolPermissionContext;
use crate::runtime::tools::capability::FileStateCache;
use crate::runtime::tools::permission::PermissionMode;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

/// Legacy full-service-locator context passed to every [`crate::plugin::tool_trait::ToolPlugin`]
/// execution.
///
/// # Deprecated — prefer `ToolExecutionContext` + `CapabilityContext`
///
/// `PluginContext` is the **legacy** way to hand services to tools.  It exposes
/// every system service as public fields, making it a classic service locator
/// anti-pattern.  New runtime tools should **not** accept this type.
///
/// See the [module-level documentation](self) for the full migration guide.
///
/// All fields are `Arc`-wrapped or cheaply clonable, making it safe to share
/// across concurrent tool executions via `Clone`.  This property is preserved
/// for the legacy bridge and must **not** be removed during the migration window.
#[deprecated(
    since = "0.4.0",
    note = "New tools must use `ToolExecutionContext` + `CapabilityContext` instead. \
            Existing tools are bridged via `LegacyToolAdapter::from_plugin` and do not \
            need an immediate rewrite — migrate tool-by-tool. \
            See module-level doc in `src/plugin/context.rs` for guidance."
)]
#[derive(Clone)]
pub struct PluginContext {
    pub storage: Arc<AppStorage>,
    pub file_manager: Arc<FileManager>,
    pub workspace_path: PathBuf,
    pub conversation_id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub agent_id: Option<AgentId>,
    pub app_handle: Option<tauri::AppHandle>,
    pub auth_manager: Option<Arc<AuthManager>>,
    pub dingtalk_bridge: Option<Arc<DingtalkBridge>>,
    pub model: String,
    /// LLM gateway for sub-agent execution (delegation tools).
    pub gateway: Option<Arc<LlmGateway>>,
    /// Tool registry for sub-agent tool execution.
    pub tool_registry: Option<Arc<ToolRegistry>>,
    /// App settings for sub-agent LLM calls.
    pub app_settings: Option<Arc<AppSettings>>,
    /// Agent runtime for child-run orchestration.
    pub agent_runtime: Option<Arc<AgentRuntime>>,
    /// Event bus for emitting runtime events (e.g. AgentIdle on background completion).
    pub event_bus: Option<RuntimeEventBus>,
    /// Skill registry for request-scoped skill switch runtime tools.
    pub skill_registry:
        Option<Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>>,
    /// 用户通过 UI 授权的本地目录（workspace-first 专项新增）
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    /// Transitional bridge for request-scoped runtime executions that still
    /// enter through PluginContext (for example subagent tool loops).
    pub read_file_state: Option<Arc<FileStateCache>>,
    /// Transitional bridge for request-scoped runtime executions that still
    /// enter through PluginContext but need the per-call cancellation token.
    pub cancellation: Option<CancellationToken>,
    /// Transitional bridge for legacy ToolPlugin paths that still need the
    /// current permission mode for nested runtime launches.
    pub permission_mode: PermissionMode,
    /// Transitional bridge for legacy ToolPlugin paths that still need managed
    /// Node/Python runtime dependencies while they migrate to runtime tools.
    pub runtime_resolver: Option<ManagedRuntimeResolver>,
    /// Phase 5 path-auth inheritance: the parent turn's merged ToolPermissionContext.
    /// Set when the PluginContext is constructed inside a sub-agent call so that
    /// registry.rs can pass the parent's authorized paths into StorageCapability.
    /// `None` for all non-sub-agent paths (legacy tools, test helpers) — they get
    /// `ToolPermissionContext::empty()` as before.
    pub permission_ctx: Option<Arc<ToolPermissionContext>>,
    /// Active persona id resolved by the chat main path before tools are built.
    /// Used by request-scoped tools (e.g. agenda) to bind organizer identity at
    /// construction time so that LLM cannot forge it via tool input. `None` for
    /// legacy / test paths where no persona context is available; tools that
    /// require it should `?` short-circuit and decline to build.
    pub current_persona_id: Option<String>,
}

impl PluginContext {
    pub fn loaded_scope_id(&self) -> &str {
        self.run_id
            .as_ref()
            .map(RunId::as_str)
            .unwrap_or(&self.conversation_id)
    }

}
