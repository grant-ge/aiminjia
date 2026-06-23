//! Plugin registries — runtime registration and lookup for Tools and Skills.
#![allow(dead_code)]
// This registry intentionally bridges the deprecated ToolPlugin trait into
// the new RuntimeTool dispatcher.  Suppress the deprecation lint here since
// the warning would be noise — the suppression is on the whole legacy zone.
#![allow(deprecated)]

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::llm::streaming::ToolDefinition;
use crate::runtime::dependencies::ManagedRuntimeResolver;
use crate::runtime::store::permission_store::PermissionStore;
use crate::runtime::tools::capability::{CapabilityContext, StorageCapability};
use crate::runtime::tools::permission::StorePolicyPipeline;
use crate::runtime::tools::permission::{PermissionDecision, PermissionMode};
use crate::runtime::tools::{
    CapabilityPermissionPipeline, LegacyToolAdapter, PermissionPipeline, ToolDispatcher,
};

use super::context::PluginContext;
use super::skill_trait::{Skill, ToolFilter};
use super::tool_trait::{ToolError, ToolOutput, ToolPlugin};

#[derive(Clone)]
pub struct RequestScopedRuntimeDeps {
    pub storage: Arc<crate::storage::file_store::AppStorage>,
    pub file_manager: Arc<crate::storage::file_manager::FileManager>,
    pub workspace_path: std::path::PathBuf,
    pub conversation_id: String,
    pub session_id: crate::runtime::ids::SessionId,
    pub run_id: Option<crate::runtime::ids::RunId>,
    pub agent_id: Option<crate::runtime::ids::AgentId>,
    pub app_handle: Option<tauri::AppHandle>,
    pub auth_manager: Option<Arc<crate::auth::AuthManager>>,
    pub model: String,
    pub gateway: Option<Arc<crate::llm::gateway::LlmGateway>>,
    pub tool_registry: Option<Arc<crate::plugin::registry::ToolRegistry>>,
    pub app_settings: Option<Arc<crate::models::settings::AppSettings>>,
    pub agent_runtime: Option<Arc<crate::runtime::agent::AgentRuntime>>,
    pub async_agent_task_store:
        Option<Arc<crate::runtime::agent::async_task_store::AsyncAgentTaskStore>>,
    pub task_notification_queue:
        Option<Arc<crate::runtime::agent::task_notification::TaskNotificationQueue>>,
    pub agent_registry: Option<Arc<crate::runtime::agent::registry::AgentRegistry>>,
    pub user_scoped_path_resolver:
        Option<Arc<dyn crate::storage::user_scoped_paths::UserScopedPathResolver>>,
    pub event_bus: Option<crate::runtime::event_bus::RuntimeEventBus>,
    pub skill_registry:
        Option<Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>>,
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    pub read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    pub cancellation: Option<crate::runtime::cancellation::CancellationToken>,
    pub permission_mode: PermissionMode,
    pub runtime_resolver: Option<ManagedRuntimeResolver>,
    /// Phase 5 path-auth inheritance: the parent turn's merged ToolPermissionContext.
    /// Propagated from `PluginContext.permission_ctx` so that registry.rs can pass the
    /// parent's authorized paths into `StorageCapability` when executing tools.
    /// `None` for non-sub-agent paths (legacy tools, test helpers).
    pub permission_ctx: Option<Arc<crate::runtime::path_auth::ToolPermissionContext>>,
    /// Active persona id resolved by the chat main path. Threaded via
    /// `PluginContext.current_persona_id` so request-scoped tools (e.g. agenda)
    /// can bind organizer identity at construction time. `None` for legacy /
    /// test paths; tools that require it should `?` short-circuit.
    pub current_persona_id: Option<String>,
}

impl RequestScopedRuntimeDeps {
    pub fn from_plugin_context(ctx: &PluginContext) -> Self {
        Self {
            storage: ctx.storage.clone(),
            file_manager: ctx.file_manager.clone(),
            workspace_path: ctx.workspace_path.clone(),
            conversation_id: ctx.conversation_id.clone(),
            session_id: ctx.session_id.clone(),
            run_id: ctx.run_id.clone(),
            agent_id: ctx.agent_id.clone(),
            app_handle: ctx.app_handle.clone(),
            auth_manager: ctx.auth_manager.clone(),
            model: ctx.model.clone(),
            gateway: ctx.gateway.clone(),
            tool_registry: ctx.tool_registry.clone(),
            app_settings: ctx.app_settings.clone(),
            agent_runtime: ctx.agent_runtime.clone(),
            async_agent_task_store: None,
            task_notification_queue: None,
            agent_registry: None,
            user_scoped_path_resolver: None,
            event_bus: ctx.event_bus.clone(),
            skill_registry: ctx.skill_registry.clone(),
            authorized_workspace: ctx.authorized_workspace.clone(),
            read_file_state: ctx.read_file_state.clone(),
            cancellation: ctx.cancellation.clone(),
            permission_mode: ctx.permission_mode,
            runtime_resolver: ctx.runtime_resolver.clone(),
            permission_ctx: ctx.permission_ctx.clone(),
            current_persona_id: ctx.current_persona_id.clone(),
        }
    }

    pub fn with_run_scope(
        &self,
        run_id: Option<crate::runtime::ids::RunId>,
        agent_id: Option<crate::runtime::ids::AgentId>,
        cancellation: Option<crate::runtime::cancellation::CancellationToken>,
        read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    ) -> Self {
        let mut next = self.clone();
        next.run_id = run_id;
        next.agent_id = agent_id;
        next.cancellation = cancellation;
        next.read_file_state = read_file_state;
        next
    }

    pub fn with_runtime_resolver(mut self, runtime_resolver: ManagedRuntimeResolver) -> Self {
        self.runtime_resolver = Some(runtime_resolver);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::ToolDescriptionContext;
    use serde_json::json;

    #[tokio::test]
    async fn find_skills_market_tools_are_request_scoped_and_override_only() {
        let registry = ToolRegistry::new();
        let empty = std::collections::HashMap::new();

        let without_overrides = registry
            .get_schemas_filtered(&ToolFilter::All, &ToolDescriptionContext::empty(), &empty)
            .await;

        assert!(!without_overrides
            .iter()
            .any(|def| def.name == "SkillMarketSearch"));
        assert!(!without_overrides
            .iter()
            .any(|def| def.name == "SkillMarketInstall"));

        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "SkillMarketSearch".to_string(),
            ToolDefinition {
                name: "SkillMarketSearch".to_string(),
                description: "Search market skills".to_string(),
                parameters: json!({"type": "object"}),
            },
        );
        overrides.insert(
            "SkillMarketInstall".to_string(),
            ToolDefinition {
                name: "SkillMarketInstall".to_string(),
                description: "Install market skills".to_string(),
                parameters: json!({"type": "object"}),
            },
        );

        let with_overrides = registry
            .get_schemas_filtered(
                &ToolFilter::All,
                &ToolDescriptionContext::empty(),
                &overrides,
            )
            .await;

        assert!(with_overrides
            .iter()
            .any(|def| def.name == "SkillMarketSearch"));
        assert!(with_overrides
            .iter()
            .any(|def| def.name == "SkillMarketInstall"));
    }
}

#[derive(Debug)]
struct AppSkillRegistryRefresher {
    app: tauri::AppHandle,
}

impl crate::runtime::tools::builtin::refresh_skills::SkillRegistryRefresher
    for AppSkillRegistryRefresher
{
    fn refresh_skill_registry(&self) -> Result<(), String> {
        crate::commands::skill_management::refresh_skill_registry(&self.app)
    }
}

const REQUEST_SCOPED_RUNTIME_TOOL_NAMES: &[&str] = &[
    "WebSearch",
    "Agent",
    "WriteMemory",
    "SearchMemory",
    "Skill",
    "ImageTask",
    "TaskOutput",
    "TaskStop",
    #[cfg(not(windows))]
    "Bash",
    #[cfg(windows)]
    "PowerShell",
    // Agenda tools (request-scoped — built per-turn from RequestScopedRuntimeDeps.current_persona_id)
    "create_agenda_item",
    "list_agenda_items",
    "update_agenda_item",
    "cancel_agenda_item",
    "skip_occurrence",
    "list_agenda_occurrences",
    // RefreshSkills — request-scoped because it needs AppHandle from ctx
    "RefreshSkills",
    // find-skills gated market tools — request-scoped because exposure depends
    // on the current user's enabled skill catalog and auth/app state.
    "SkillMarketSearch",
    "SkillMarketInstall",
];

fn request_scoped_tool_requires_override(id: &str) -> bool {
    matches!(id, "SkillMarketSearch" | "SkillMarketInstall")
}

/// Info about a registered tool (for management UI).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub source: String, // "builtin" or "plugin"
}

/// Info about a registered skill (for management UI).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub source: String,
    pub has_workflow: bool,
    pub icon: String,
    pub short_description: String,
    pub trigger_text: String,
    pub category: String,
    pub display_name_en: String,
    pub short_description_en: String,
}

// ─────────────────────────────────────────────────
// ToolRegistry
// ─────────────────────────────────────────────────

struct RegisteredTool {
    plugin: Arc<dyn ToolPlugin>,
    source: String,
}

/// Runtime registry for tool plugins.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, RegisteredTool>>,
    /// Runtime-native tools — take precedence over legacy ToolPlugin in dispatcher.
    runtime_tools: RwLock<HashMap<String, Arc<dyn crate::runtime::tools::RuntimeTool>>>,
    /// Persistent/session-scoped authorization decisions for capability scopes.
    permission_store: RwLock<Option<Arc<PermissionStore>>>,
}

fn partition_sort_tool_schemas(mut schemas: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    let (mut builtin_schemas, mut mcp_schemas): (Vec<_>, Vec<_>) = schemas
        .drain(..)
        .partition(|schema| !schema.name.starts_with("mcp__"));
    builtin_schemas.sort_by(|a, b| a.name.cmp(&b.name));
    mcp_schemas.sort_by(|a, b| a.name.cmp(&b.name));
    builtin_schemas.extend(mcp_schemas);
    builtin_schemas
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            runtime_tools: RwLock::new(HashMap::new()),
            permission_store: RwLock::new(None),
        }
    }

    pub async fn set_permission_store(&self, store: Arc<PermissionStore>) {
        *self.permission_store.write().await = Some(store);
    }

    fn has_runtime_schema_source(
        runtime_tools: &HashMap<String, Arc<dyn crate::runtime::tools::RuntimeTool>>,
        name: &str,
    ) -> bool {
        runtime_tools.contains_key(name)
            || REQUEST_SCOPED_RUNTIME_TOOL_NAMES
                .iter()
                .any(|runtime_name| runtime_name == &name)
    }

    /// Register a native RuntimeTool.
    /// When `to_runtime_dispatcher()` builds the dispatcher, registered RuntimeTools
    /// take priority over legacy ToolPlugin adapters for the same tool name.
    pub async fn register_runtime(&self, tool: Arc<dyn crate::runtime::tools::RuntimeTool>) {
        use crate::runtime::tools::catalog::{CatalogEntry, TOOL_CATALOG};

        // Registration uses an empty description context — the tool's
        // base definition (no session-derived catalog) is what goes into
        // TOOL_CATALOG as the static fallback. Per-turn LLM tools assembly
        // re-renders with a populated context, see `get_schemas_filtered`.
        let empty_ctx = crate::runtime::tools::ToolDescriptionContext::empty();
        let def = tool.definition(&empty_ctx).await;
        let id = def.id.clone();
        log::info!("Registering runtime tool: {}", id);
        self.runtime_tools.write().await.insert(id, tool);

        if TOOL_CATALOG.get_entry(&def.id).is_none() {
            TOOL_CATALOG.register_entry(CatalogEntry::new(def, Self::infer_json_schema()));
        }
    }

    /// 移除 runtime-first 路径下已注册的工具。
    ///
    /// 用于动态 MCP server 在 disconnect / refresh 时清理其工具池。
    pub async fn unregister_runtime_tools(&self, ids: &[String]) {
        use crate::runtime::tools::catalog::TOOL_CATALOG;

        let mut runtime_tools = self.runtime_tools.write().await;
        for id in ids {
            runtime_tools.remove(id);
            let _ = TOOL_CATALOG.remove_entry(id);
        }
    }

    /// Register all tools exposed by an MCP server connection.
    ///
    /// Returns the fully-qualified tool ids so a caller can later unregister
    /// the exact dynamic tool set on disconnect / refresh.
    pub async fn register_mcp_server(
        &self,
        connection: Arc<dyn crate::runtime::mcp::McpConnection>,
    ) -> Result<Vec<String>, String> {
        use crate::runtime::mcp::McpRuntimeTool;
        use crate::runtime::tools::catalog::TOOL_CATALOG;

        if !connection.is_connected() {
            connection
                .connect()
                .await
                .map_err(|err| format!("Failed to connect MCP server: {err}"))?;
        }

        let mcp_tools = connection
            .list_tools()
            .await
            .map_err(|err| format!("Failed to list MCP tools: {err}"))?;

        let mut registered_ids = Vec::with_capacity(mcp_tools.len());
        for tool in mcp_tools {
            TOOL_CATALOG.register_entry(tool.to_catalog_entry());
            registered_ids.push(tool.qualified_name());
            self.register_runtime(Arc::new(McpRuntimeTool::new(tool, connection.clone())))
                .await;
        }

        Ok(registered_ids)
    }

    fn infer_json_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    /// Validate that every runtime tool exposed through the runtime-first path
    /// has a matching entry in TOOL_CATALOG.
    pub async fn validate_catalog_consistency(&self) {
        use crate::runtime::tools::catalog::TOOL_CATALOG;

        let runtime_tools = self.runtime_tools.read().await;
        for id in runtime_tools.keys() {
            assert!(
                TOOL_CATALOG.get_entry(id).is_some(),
                "ToolRegistry consistency error: RuntimeTool '{}' is registered but missing from TOOL_CATALOG",
                id
            );
        }
        for id in REQUEST_SCOPED_RUNTIME_TOOL_NAMES {
            assert!(
                TOOL_CATALOG.get_entry(id).is_some(),
                "ToolRegistry consistency error: request-scoped RuntimeTool '{}' is missing from TOOL_CATALOG",
                id
            );
        }
        log::info!(
            "ToolRegistry catalog consistency check passed ({} global runtime tools, {} request-scoped runtime tools)",
            runtime_tools.len(),
            REQUEST_SCOPED_RUNTIME_TOOL_NAMES.len()
        );
    }

    /// Register a tool plugin.
    /// Warns and rejects if a builtin tool would be shadowed by a plugin.
    pub async fn register(&self, tool: Arc<dyn ToolPlugin>, source: &str) {
        let name = tool.name().to_string();
        let mut tools = self.tools.write().await;
        if let Some(existing) = tools.get(&name) {
            if existing.source == "builtin" && source != "builtin" {
                log::warn!(
                    "Rejecting plugin tool '{}': cannot shadow builtin tool",
                    name
                );
                return;
            }
        }
        log::info!("Registering tool: {} (source: {})", name, source);
        tools.insert(
            name,
            RegisteredTool {
                plugin: tool,
                source: source.to_string(),
            },
        );
    }

    /// Unregister a tool by name.
    pub async fn unregister(&self, name: &str) {
        let mut tools = self.tools.write().await;
        if tools.remove(name).is_some() {
            log::info!("Unregistered tool: {}", name);
        }
    }

    /// Get all tool definitions (for LLM context).
    /// Runtime tools take precedence: their schema comes from the catalog.
    /// Legacy tools not yet migrated fall back to plugin.input_schema().
    pub async fn get_all_schemas(&self) -> Vec<ToolDefinition> {
        use crate::runtime::tools::catalog::TOOL_CATALOG;
        let runtime_tools = self.runtime_tools.read().await;
        let legacy_tools = self.tools.read().await;
        let mut schemas = Vec::new();

        // Runtime tools: get schema from catalog (single source of truth)
        for (id, _) in runtime_tools.iter() {
            if let Some(entry) = TOOL_CATALOG.get_entry(id) {
                schemas.push(ToolDefinition {
                    name: entry.definition.id.clone(),
                    description: entry.definition.description.clone(),
                    parameters: entry.json_schema.clone(),
                });
            }
        }
        for id in REQUEST_SCOPED_RUNTIME_TOOL_NAMES {
            if runtime_tools.contains_key(*id) {
                continue;
            }
            if request_scoped_tool_requires_override(id) {
                continue;
            }
            if let Some(entry) = TOOL_CATALOG.get_entry(id) {
                schemas.push(ToolDefinition {
                    name: entry.definition.id.clone(),
                    description: entry.definition.description.clone(),
                    parameters: entry.json_schema.clone(),
                });
            }
        }

        // Legacy tools not yet migrated: get schema from plugin
        for rt in legacy_tools.values() {
            let name = rt.plugin.name();
            if !Self::has_runtime_schema_source(&runtime_tools, name) {
                schemas.push(ToolDefinition {
                    name: name.to_string(),
                    description: rt.plugin.description().to_string(),
                    parameters: rt.plugin.input_schema(),
                });
            }
        }

        partition_sort_tool_schemas(schemas)
    }

    /// Get tool definitions filtered by a ToolFilter.
    /// Runtime tools take precedence over legacy tools with the same name.
    ///
    /// `ctx` carries session-scoped context (available subagent / employee
    /// names, MCP servers) so RuntimeTools can render dynamic descriptions
    /// per turn. Pass [`ToolDescriptionContext::empty()`] when no session
    /// info is available (catalog dump, list endpoints).
    ///
    /// `request_scoped_overrides` lets the caller supply pre-rendered
    /// `ToolDefinition`s for request-scoped tools (notably `Agent`) that
    /// have no live registered instance in `runtime_tools`.  The
    /// `ToolDispatcher::try_build_request_scoped_tool` path must be
    /// driven by the caller (chat layer has the AgentRegistry +
    /// EmployeeStore handles). Override id matches the tool's `id()`.
    pub async fn get_schemas_filtered(
        &self,
        filter: &ToolFilter,
        ctx: &crate::runtime::tools::ToolDescriptionContext,
        request_scoped_overrides: &std::collections::HashMap<String, ToolDefinition>,
    ) -> Vec<ToolDefinition> {
        use crate::runtime::tools::catalog::TOOL_CATALOG;
        let runtime_tools = self.runtime_tools.read().await;
        let legacy_tools = self.tools.read().await;
        let mut schemas = Vec::new();

        let override_keys: Vec<&str> = request_scoped_overrides
            .keys()
            .map(|k| k.as_str())
            .collect();
        let employee_count = ctx
            .agents
            .iter()
            .filter(|a| {
                matches!(
                    a.source,
                    crate::runtime::agent::definition::AgentSource::Employee
                )
            })
            .count();
        log::info!(
            "[tool-desc-trace] entered get_schemas_filtered ctx_employees={} ctx_agents={} overrides_keys={:?}",
            employee_count,
            ctx.agents.len(),
            override_keys,
        );

        // Runtime tools: render description per-turn so tools whose
        // catalog depends on session state (Agent → employee/agent list)
        // are correct each turn. Map runtime ToolDefinition → llm
        // streaming ToolDefinition (name/description/parameters).  The
        // input_schema lives in TOOL_CATALOG (registered alongside the
        // tool at boot) — runtime ToolDefinition itself doesn't carry it.
        for (id, tool) in runtime_tools.iter() {
            let matches = match filter {
                ToolFilter::All => true,
                ToolFilter::Only(names) => names.iter().any(|n| n == id),
                ToolFilter::Exclude(names) => names.iter().all(|n| n != id),
            };
            if matches {
                let rendered = tool.definition(ctx).await;
                let parameters = TOOL_CATALOG
                    .get_entry(id)
                    .map(|e| e.json_schema.clone())
                    .unwrap_or_else(|| serde_json::json!({"type": "object"}));
                let has_emp = rendered.description.contains("<available_subagent_types>");
                log::info!(
                    "[tool-desc-trace] pushed runtime tool: id={} desc_len={} has_emp_section={}",
                    rendered.id,
                    rendered.description.len(),
                    has_emp,
                );
                schemas.push(ToolDefinition {
                    name: rendered.id,
                    description: rendered.description,
                    parameters,
                });
            }
        }
        for id in REQUEST_SCOPED_RUNTIME_TOOL_NAMES {
            if runtime_tools.contains_key(*id) {
                continue;
            }
            let matches = match filter {
                ToolFilter::All => true,
                ToolFilter::Only(names) => names.iter().any(|n| n == id),
                ToolFilter::Exclude(names) => names.iter().all(|n| n != id),
            };
            if matches {
                if let Some(override_def) = request_scoped_overrides.get(*id) {
                    // Caller supplied a freshly-rendered description (e.g.
                    // Agent with employee_id catalog) — use it verbatim.
                    let has_emp = override_def
                        .description
                        .contains("<available_subagent_types>");
                    log::info!(
                        "[tool-desc-trace] used override for tool: id={} desc_len={} has_emp_section={}",
                        id,
                        override_def.description.len(),
                        has_emp,
                    );
                    schemas.push(override_def.clone());
                } else if request_scoped_tool_requires_override(id) {
                    log::info!(
                        "[tool-desc-trace] skipped request-scoped tool: id={} reason=requires_override",
                        id,
                    );
                } else if let Some(entry) = TOOL_CATALOG.get_entry(id) {
                    // Fallback: static catalog entry. Used for tools whose
                    // description doesn't depend on session state, or
                    // when the caller couldn't construct an override.
                    log::info!(
                        "[tool-desc-trace] fell back to static catalog for tool: id={} reason=no_override",
                        id,
                    );
                    schemas.push(ToolDefinition {
                        name: entry.definition.id.clone(),
                        description: entry.definition.description.clone(),
                        parameters: entry.json_schema.clone(),
                    });
                }
            }
        }

        // Legacy tools not yet migrated
        for rt in legacy_tools.values() {
            let name = rt.plugin.name();
            if Self::has_runtime_schema_source(&runtime_tools, name) {
                continue;
            }
            let matches = match filter {
                ToolFilter::All => true,
                ToolFilter::Only(names) => names.iter().any(|n| n == name),
                ToolFilter::Exclude(names) => names.iter().all(|n| n != name),
            };
            if matches {
                schemas.push(ToolDefinition {
                    name: name.to_string(),
                    description: rt.plugin.description().to_string(),
                    parameters: rt.plugin.input_schema(),
                });
            }
        }

        let result = partition_sort_tool_schemas(schemas);
        let agent_found = result.iter().any(|d| d.name == "Agent");
        let agent_has_emp = result
            .iter()
            .find(|d| d.name == "Agent")
            .map(|d| d.description.contains("<available_subagent_types>"))
            .unwrap_or(false);
        log::info!(
            "[tool-desc-trace] get_schemas_filtered done: total={} agent_found={} agent_has_emp_section={}",
            result.len(),
            agent_found,
            agent_has_emp,
        );
        result
    }

    /// DEPRECATED: PluginContext 桥接入口，不与 claude-code-best 架构对齐。
    ///
    /// 正确路径：`ToolDispatcher::dispatch()` → `RuntimeTool::execute(input, ToolExecutionContext)`
    /// 所有走此路径的 ToolPlugin 工具均为过期工具，等待删除。
    ///
    /// 此方法保留仅为测试辅助路径（`commands/chat.rs` 的 `WorkspaceFirstToolTrace`），
    /// 生产代码禁止新增调用。
    #[deprecated(
        since = "0.0.0",
        note = "Use ToolDispatcher::dispatch() instead. This path bridges PluginContext and will be removed with legacy ToolPlugin tools."
    )]
    pub async fn execute(
        &self,
        name: &str,
        ctx: &RequestScopedRuntimeDeps,
        input: serde_json::Value,
        cancel_token: crate::runtime::cancellation::CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        // Step 1: Check global runtime_tools (stateless, e.g. workspace tools)
        let runtime_tool: Option<Arc<dyn crate::runtime::tools::RuntimeTool>> = {
            let rt = self.runtime_tools.read().await;
            rt.get(name).cloned()
        };

        // Step 2: Try request-level factory (session-scoped deps built from PluginContext)
        // TRANSITIONAL: BrowserDeps carry conversation_id / run_id which
        // cannot be stored in the global singleton registry.
        let runtime_tool = match runtime_tool {
            Some(t) => Some(t),
            None => Self::try_build_request_scoped_tool(name, ctx).await,
        };

        if let Some(tool) = runtime_tool {
            // Build CapabilityContext from PluginContext fields
            let capability = {
                let storage = StorageCapability {
                    workspace_path: ctx.workspace_path.clone(),
                    authorized_workspace: ctx.authorized_workspace.clone(),
                    // Phase 5: inherit parent's permission_ctx when available
                    // (sub-agent path), otherwise fall back to empty() (legacy/test).
                    permission_ctx: ctx.permission_ctx.clone().unwrap_or_else(|| {
                        std::sync::Arc::new(
                            crate::runtime::path_auth::ToolPermissionContext::empty(),
                        )
                    }),
                };
                let cap = CapabilityContext {
                    storage: Some(storage),
                    workspace_id: Some(ctx.conversation_id.clone()),
                    file_ops: None,
                    read_file_state: ctx.read_file_state.clone(),
                    file_reading_limits: Some(
                        crate::runtime::tools::capability::FileReadingLimits::default(),
                    ),
                    notification_sink: None,
                    // Tool progress sink is wired by the per-turn QueryEngine
                    // (BusBackedToolProgressSink) and propagates into the
                    // ToolExecutionContext that actually reaches bash/powershell.
                    // The legacy `plugin/registry.rs` builder doesn't have a
                    // bus reference here, so leave it None — long-running tools
                    // dispatched through this path will degrade silently to
                    // "no live tail", which matches pre-2026-05-26 behavior.
                    tool_progress_sink: None,
                    runtime_resolver: ctx.runtime_resolver.clone(),
                    is_subagent: ctx.agent_id.is_some(),
                };
                std::sync::Arc::new(cap)
            };

            let run_id = ctx.run_id.clone().unwrap_or_else(|| {
                crate::runtime::ids::RunId::new(format!("run-{}", ctx.conversation_id))
            });
            let exec_ctx = crate::runtime::tools::ToolExecutionContext::new(
                ctx.session_id.clone(),
                run_id,
                ctx.agent_id.clone(),
                format!("tool-{}", name),
                cancel_token.child_token(),
            )
            .with_permission_mode(ctx.permission_mode)
            .with_capability(capability);

            // Permission check: prefer StorePolicyPipeline if permission_store is available
            let pipeline: Box<dyn PermissionPipeline> =
                match self.permission_store.read().await.as_ref() {
                    Some(store) => Box::new(StorePolicyPipeline::new(store.clone())),
                    None => Box::new(CapabilityPermissionPipeline),
                };
            let def = tool
                .definition(&crate::runtime::tools::ToolDescriptionContext::empty())
                .await;
            let permission_decision =
                if let Some(decision) = tool.check_permissions(&input, &exec_ctx).await {
                    decision
                } else {
                    pipeline.authorize(&def, &input, &exec_ctx)
                };

            match permission_decision {
                PermissionDecision::Allow { .. } => {}
                PermissionDecision::Deny { message, .. } => {
                    return Err(ToolError::PermissionDenied(format!(
                        "Permission denied: {}",
                        message
                    )));
                }
                decision @ PermissionDecision::Ask { .. } => {
                    return Err(ToolError::AskRequired(decision));
                }
            }

            let result = match tool.execute(input, exec_ctx).await {
                Ok(result) => result,
                Err(crate::runtime::tools::ToolError::AskRequired(decision)) => {
                    return Err(ToolError::AskRequired(decision));
                }
                Err(crate::runtime::tools::ToolError::InteractionRequired(_)) => {
                    return Err(ToolError::ExecutionFailed(
                        "Runtime tool requires user interaction, but legacy registry execution cannot route interaction requests.".into(),
                    ));
                }
                Err(crate::runtime::tools::ToolError::PermissionDenied(message)) => {
                    return Err(ToolError::PermissionDenied(message));
                }
                Err(crate::runtime::tools::ToolError::ExecutionFailed(message)) => {
                    return Err(ToolError::ExecutionFailed(message));
                }
                Err(crate::runtime::tools::ToolError::InputValidationError {
                    tool_name,
                    message,
                }) => {
                    return Err(ToolError::ExecutionFailed(format!(
                        "input validation error for tool '{tool_name}': {message}"
                    )));
                }
                Err(crate::runtime::tools::ToolError::Other(err)) => {
                    return Err(ToolError::Other(err));
                }
            };

            let mut output = ToolOutput::success(result.content);
            output.data = result.data;
            output.file_meta = result.file_meta;
            output.is_degraded = result.is_degraded;
            output.degradation_notice = result.degradation_notice;
            return Ok(output);
        }

        // Step 3: Fallback to legacy ToolPlugin
        let plugin = {
            let tools = self.tools.read().await;
            let rt = tools
                .get(name)
                .ok_or_else(|| ToolError::ExecutionFailed(format!("Unknown tool: {}", name)))?;
            rt.plugin.clone() // Arc::clone is cheap — release lock before executing
        };
        let pipeline: Arc<dyn PermissionPipeline> =
            match self.permission_store.read().await.as_ref() {
                Some(store) => Arc::new(StorePolicyPipeline::new(store.clone())),
                None => Arc::new(CapabilityPermissionPipeline),
            };
        let dispatcher = ToolDispatcher::new(pipeline);
        dispatcher.register(Arc::new(LegacyToolAdapter::from_plugin(
            plugin,
            PluginContext {
                storage: ctx.storage.clone(),
                file_manager: ctx.file_manager.clone(),
                workspace_path: ctx.workspace_path.clone(),
                conversation_id: ctx.conversation_id.clone(),
                session_id: ctx.session_id.clone(),
                run_id: ctx.run_id.clone(),
                agent_id: ctx.agent_id.clone(),
                app_handle: ctx.app_handle.clone(),
                auth_manager: ctx.auth_manager.clone(),
                model: ctx.model.clone(),
                gateway: ctx.gateway.clone(),
                tool_registry: ctx.tool_registry.clone(),
                app_settings: ctx.app_settings.clone(),
                agent_runtime: ctx.agent_runtime.clone(),
                event_bus: ctx.event_bus.clone(),
                skill_registry: ctx.skill_registry.clone(),
                authorized_workspace: ctx.authorized_workspace.clone(),
                read_file_state: ctx.read_file_state.clone(),
                cancellation: ctx.cancellation.clone(),
                permission_mode: ctx.permission_mode,
                runtime_resolver: ctx.runtime_resolver.clone(),
                dingtalk_bridge: None,
                permission_ctx: ctx.permission_ctx.clone(),
                current_persona_id: ctx.current_persona_id.clone(),
            },
        )));
        let runtime_ctx = crate::runtime::tools::ToolExecutionContext::new(
            ctx.session_id.clone(),
            ctx.run_id.clone().unwrap_or_else(|| {
                crate::runtime::ids::RunId::new(format!("run-{}", ctx.conversation_id))
            }),
            ctx.agent_id.clone(),
            format!("tool-{}", name),
            cancel_token.child_token(),
        )
        .with_permission_mode(ctx.permission_mode);
        let outcome = dispatcher
            .dispatch(name, input, runtime_ctx)
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
        match outcome {
            crate::runtime::tools::ToolDispatchOutcome::Completed { result, .. } => {
                let mut output = ToolOutput::success(result.content);
                output.data = result.data;
                output.file_meta = result.file_meta;
                output.is_degraded = result.is_degraded;
                output.degradation_notice = result.degradation_notice;
                Ok(output)
            }
            crate::runtime::tools::ToolDispatchOutcome::AskRequired(decision) => {
                // Legacy path: surface Ask semantics to the caller.
                // Callers that cannot show a UI prompt should treat this as deny.
                Err(ToolError::AskRequired(decision))
            }
            crate::runtime::tools::ToolDispatchOutcome::InteractionRequired(_) => {
                Err(ToolError::ExecutionFailed(
                    "Runtime tool requires user interaction, but legacy registry execution cannot route interaction requests.".into(),
                ))
            }
        }
    }

    /// List all registered tools (for management UI).
    pub async fn list(&self) -> Vec<ToolInfo> {
        let tools = self.tools.read().await;
        tools
            .values()
            .map(|rt| ToolInfo {
                name: rt.plugin.name().to_string(),
                description: rt.plugin.description().to_string(),
                source: rt.source.clone(),
            })
            .collect()
    }

    /// Build a session-static dispatcher containing only the already-registered
    /// `runtime_tools`.  Request-scoped tools (web_search, browser, etc.) are NOT
    /// included — the caller must handle those separately or use
    /// `to_runtime_dispatcher` when request-scoped deps are available.
    ///
    /// Intended for paths where a `QueryEngine` must be wired up before the first
    /// request arrives (e.g. `TauriChatCommandAdapter::new()`).
    pub async fn to_static_dispatcher(&self) -> Arc<ToolDispatcher> {
        let pipeline: Arc<dyn PermissionPipeline> =
            match self.permission_store.read().await.as_ref() {
                Some(store) => Arc::new(StorePolicyPipeline::new(store.clone())),
                None => Arc::new(CapabilityPermissionPipeline),
            };
        let dispatcher = Arc::new(ToolDispatcher::new(pipeline));
        let runtime_tools = self.runtime_tools.read().await;
        for (_, tool) in runtime_tools.iter() {
            dispatcher.register(tool.clone());
        }
        dispatcher
    }

    /// Build a runtime-first dispatcher while keeping legacy tool implementations
    /// behind an adapter. This is the bridge point for incrementally moving the
    /// production query path onto the new runtime contract.
    pub async fn to_runtime_dispatcher(
        &self,
        request_scoped: RequestScopedRuntimeDeps,
    ) -> Arc<ToolDispatcher> {
        let pipeline: Arc<dyn PermissionPipeline> =
            match self.permission_store.read().await.as_ref() {
                Some(store) => Arc::new(StorePolicyPipeline::new(store.clone())),
                None => Arc::new(CapabilityPermissionPipeline),
            };
        let dispatcher = Arc::new(ToolDispatcher::new(pipeline));
        let runtime_tools = self.runtime_tools.read().await;

        // 1. Register native RuntimeTools first (they take priority)
        for (_, tool) in runtime_tools.iter() {
            dispatcher.register(tool.clone());
        }

        // 2. Register request-scoped RuntimeTools built from explicit runtime deps.
        for tool_name in REQUEST_SCOPED_RUNTIME_TOOL_NAMES {
            if runtime_tools.contains_key(*tool_name) {
                continue;
            }
            if let Some(tool) =
                Self::try_build_request_scoped_tool(tool_name, &request_scoped).await
            {
                dispatcher.register(tool);
            }
        }

        dispatcher
    }

    /// Request-scoped factory: build a `RuntimeTool` from `PluginContext` on the fly.
    ///
    /// Called by `execute()` between the global `runtime_tools` lookup (Step 1)
    /// and the legacy `ToolPlugin` fallback (Step 3).  This handles tools whose
    /// `Deps` structs carry session-level state (`conversation_id`, `run_id`)
    /// that cannot be stored in the global singleton registry.
    ///
    /// Returns `None` for unknown tool names (falls through to legacy path).
    fn find_skills_market_tools_enabled(ctx: &RequestScopedRuntimeDeps) -> bool {
        use tauri::Manager;

        let Some(app) = ctx.app_handle.as_ref() else {
            return false;
        };
        let Some(skill_registry) = ctx.skill_registry.as_ref() else {
            return false;
        };
        let Some(enablement_store) =
            app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>()
        else {
            return false;
        };
        let state = enablement_store.inner().load_or_default();
        skill_registry
            .lock()
            .map(|reg| reg.get_enabled("find-skills", &state).is_some())
            .unwrap_or(false)
    }

    async fn try_build_request_scoped_tool(
        name: &str,
        ctx: &RequestScopedRuntimeDeps,
    ) -> Option<Arc<dyn crate::runtime::tools::RuntimeTool>> {
        use crate::runtime::tools::builtin;
        use std::sync::Arc;

        if ctx.app_handle.is_none() {
            match name {
                "Agent" => {
                    let agent_registry = ctx.agent_registry.clone()?;
                    let task_store = ctx.async_agent_task_store.clone()?;
                    let notif_queue = ctx.task_notification_queue.clone()?;
                    let path_resolver = ctx.user_scoped_path_resolver.clone()?;
                    return Some(Arc::new(
                        builtin::spawn_subagent::SpawnSubagentRuntimeTool::new(
                            Arc::new(
                                crate::llm::tool_executor::DefaultSpawnSubagentLauncher::from_runtime_deps(
                                    ctx.clone(),
                                    agent_registry.clone(),
                                    task_store,
                                    notif_queue,
                                    path_resolver,
                                ),
                            ),
                            agent_registry,
                        ),
                    ) as Arc<dyn crate::runtime::tools::RuntimeTool>);
                }
                "TaskOutput" => {
                    let resolver = ctx.user_scoped_path_resolver.clone()?;
                    return Some(
                        Arc::new(builtin::task_output::TaskOutputRuntimeTool::new(resolver))
                            as Arc<dyn crate::runtime::tools::RuntimeTool>,
                    );
                }
                "TaskStop" => {
                    let task_store = ctx.async_agent_task_store.clone()?;
                    return Some(Arc::new(builtin::task_stop::TaskStopRuntimeTool {
                        store: task_store,
                    })
                        as Arc<dyn crate::runtime::tools::RuntimeTool>);
                }
                #[cfg(not(windows))]
                "Bash" => {
                    let task_store = ctx.async_agent_task_store.clone()?;
                    let notif_queue = ctx.task_notification_queue.clone()?;
                    return Some(
                        Arc::new(builtin::bash::BashTool::new(task_store, notif_queue))
                            as Arc<dyn crate::runtime::tools::RuntimeTool>,
                    );
                }
                #[cfg(windows)]
                "PowerShell" => {
                    let task_store = ctx.async_agent_task_store.clone()?;
                    let notif_queue = ctx.task_notification_queue.clone()?;
                    return Some(Arc::new(builtin::powershell::PowerShellTool::new(
                        task_store,
                        notif_queue,
                    ))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>);
                }
                _ => {}
            }
        }

        match name {
            "WebSearch" => {
                let deps = builtin::network::SearchDeps {
                    auth_manager: ctx.auth_manager.clone(),
                };
                Some(Arc::new(builtin::network::WebSearchRuntimeTool::new(deps)))
            }
            "SkillMarketSearch" => {
                if !Self::find_skills_market_tools_enabled(ctx) {
                    return None;
                }
                let auth_manager = ctx.auth_manager.clone()?;
                let skill_registry = ctx.skill_registry.clone()?;
                let enablement_store = ctx.app_handle.as_ref().and_then(|app| {
                    use tauri::Manager;
                    app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>()
                        .map(|store| store.inner().clone())
                });
                Some(Arc::new(
                    builtin::skill_market::SkillMarketSearchRuntimeTool::new(
                        auth_manager,
                        skill_registry,
                        enablement_store,
                    ),
                ))
            }
            "SkillMarketInstall" => {
                if !Self::find_skills_market_tools_enabled(ctx) {
                    return None;
                }
                let app = ctx.app_handle.clone()?;
                let auth_manager = ctx.auth_manager.clone()?;
                let skill_registry = ctx.skill_registry.clone()?;
                use tauri::Manager;
                let enablement_store = app
                    .try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>()
                    .map(|store| store.inner().clone());
                Some(Arc::new(
                    builtin::skill_market::SkillMarketInstallRuntimeTool::new(
                        app,
                        auth_manager,
                        skill_registry,
                        enablement_store,
                    ),
                ))
            }
            "Agent" => {
                use tauri::Manager;

                // Fail-closed: if runtime deps/app state are missing any of the Arcs,
                // we MUST NOT silently fall back to fresh instances — async
                // sub-agent updates would write to orphan stores/queues that
                // nobody else holds, leaving notifications lost and the
                // parent observing Running forever. Refuse to register the
                // tool instead so the failure is observable as
                // "spawn_subagent missing from catalog".
                let app = match ctx.app_handle.as_ref() {
                    Some(a) => a,
                    None => {
                        log::error!(
                            "[spawn_subagent registry] no app_handle in PluginContext — \
                             cannot resolve AgentRegistry/AsyncAgentTaskStore/TaskNotificationQueue; \
                             refusing to register tool"
                        );
                        return None;
                    }
                };
                let agent_registry = match app
                    .try_state::<Arc<crate::runtime::agent::registry::AgentRegistry>>()
                {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[spawn_subagent registry] AgentRegistry not in app state — \
                             refusing to register tool (call app.manage(Arc<AgentRegistry>) at startup)"
                        );
                        return None;
                    }
                };
                let task_store = match app
                    .try_state::<Arc<crate::runtime::agent::async_task_store::AsyncAgentTaskStore>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[spawn_subagent registry] AsyncAgentTaskStore not in app state — \
                             async notifications would be lost; refusing to register tool"
                        );
                        return None;
                    }
                };
                let notif_queue = match app
                    .try_state::<Arc<crate::runtime::agent::task_notification::TaskNotificationQueue>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[spawn_subagent registry] TaskNotificationQueue not in app state — \
                             async notifications would be lost; refusing to register tool"
                        );
                        return None;
                    }
                };
                let path_resolver = match app
                    .try_state::<Arc<dyn crate::storage::user_scoped_paths::UserScopedPathResolver>>()
                {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[spawn_subagent registry] UserScopedPathResolver not in app state — \
                             transcript writes disabled; refusing to register tool"
                        );
                        return None;
                    }
                };
                Some(Arc::new(
                    builtin::spawn_subagent::SpawnSubagentRuntimeTool::new(
                        Arc::new(
                            crate::llm::tool_executor::DefaultSpawnSubagentLauncher::from_runtime_deps(
                                ctx.clone(),
                                agent_registry.clone(),
                                task_store,
                                notif_queue,
                                path_resolver,
                            ),
                        ),
                        agent_registry,
                    ),
                ) as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "TaskOutput" => {
                use tauri::Manager;
                let app = match ctx.app_handle.as_ref() {
                    Some(a) => a,
                    None => {
                        log::error!(
                            "[task_output registry] no app_handle in PluginContext — refusing to register tool"
                        );
                        return None;
                    }
                };
                let resolver = match app
                    .try_state::<Arc<dyn crate::storage::user_scoped_paths::UserScopedPathResolver>>()
                {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[task_output registry] UserScopedPathResolver not in app state — refusing to register tool"
                        );
                        return None;
                    }
                };
                Some(
                    Arc::new(builtin::task_output::TaskOutputRuntimeTool::new(resolver))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "WriteMemory" => Some(Arc::new(builtin::memory::WriteMemoryRuntimeTool::new(
                builtin::memory::MemoryDeps {
                    app_data_dir: ctx.storage.base_dir().to_path_buf(),
                    workspace_path: ctx.workspace_path.clone(),
                },
            ))
                as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "SearchMemory" => Some(Arc::new(builtin::memory::SearchMemoryRuntimeTool::new(
                builtin::memory::MemoryDeps {
                    app_data_dir: ctx.storage.base_dir().to_path_buf(),
                    workspace_path: ctx.workspace_path.clone(),
                },
            ))
                as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "ImageTask" => Some(Arc::new(builtin::image_task::ImageTaskRuntimeTool::new(
                builtin::image_task::ImageTaskDeps {
                    auth_manager: ctx.auth_manager.clone(),
                    storage: ctx.storage.clone(),
                    file_manager: ctx.file_manager.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    authorized_workspace: ctx.authorized_workspace.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                    run_id: ctx
                        .run_id
                        .as_ref()
                        .map(|run_id| run_id.as_str().to_string()),
                    gateway_base_url: None,
                },
            ))
                as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "Skill" => {
                let registry = ctx.skill_registry.clone()?;
                // 注入 transport 层 refresher，让 runtime tool 不直接依赖 Tauri。
                // ctx.app_handle 为 None 的 test/legacy 路径退回到无 refresh 的旧行为。
                let tool = match ctx.app_handle.as_ref() {
                    Some(app) => {
                        use tauri::Manager;
                        match app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>() {
                            Some(enablement_store) => {
                                builtin::load_skill::LoadSkillRuntimeTool::with_refresher_and_enablement(
                                    registry,
                                    Arc::new(AppSkillRegistryRefresher { app: app.clone() }),
                                    enablement_store.inner().clone(),
                                )
                            }
                            None => builtin::load_skill::LoadSkillRuntimeTool::with_refresher(
                                registry,
                                Arc::new(AppSkillRegistryRefresher { app: app.clone() }),
                            ),
                        }
                    }
                    None => builtin::load_skill::LoadSkillRuntimeTool::new(registry),
                };
                Some(Arc::new(tool) as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "TaskStop" => {
                use tauri::Manager;
                let app = match ctx.app_handle.as_ref() {
                    Some(a) => a,
                    None => {
                        log::error!(
                            "[task_stop registry] no app_handle in PluginContext — refusing to register tool"
                        );
                        return None;
                    }
                };
                let task_store = match app
                    .try_state::<Arc<crate::runtime::agent::async_task_store::AsyncAgentTaskStore>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[task_stop registry] AsyncAgentTaskStore not in app state — refusing to register tool"
                        );
                        return None;
                    }
                };
                Some(
                    Arc::new(builtin::task_stop::TaskStopRuntimeTool { store: task_store })
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            #[cfg(not(windows))]
            "Bash" => {
                use tauri::Manager;
                let app = ctx.app_handle.as_ref()?;
                let task_store = match app
                    .try_state::<Arc<crate::runtime::agent::async_task_store::AsyncAgentTaskStore>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[bash registry] AsyncAgentTaskStore not in app state — refusing to register tool"
                        );
                        return None;
                    }
                };
                let notif_queue = match app
                    .try_state::<Arc<crate::runtime::agent::task_notification::TaskNotificationQueue>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[bash registry] TaskNotificationQueue not in app state — refusing to register tool"
                        );
                        return None;
                    }
                };
                Some(
                    Arc::new(builtin::bash::BashTool::new(task_store, notif_queue))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            #[cfg(windows)]
            "PowerShell" => {
                use tauri::Manager;
                let app = ctx.app_handle.as_ref()?;
                let task_store = match app
                    .try_state::<Arc<crate::runtime::agent::async_task_store::AsyncAgentTaskStore>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[powershell registry] AsyncAgentTaskStore not in app state — refusing to register tool"
                        );
                        return None;
                    }
                };
                let notif_queue = match app
                    .try_state::<Arc<crate::runtime::agent::task_notification::TaskNotificationQueue>>(
                    ) {
                    Some(s) => s.inner().clone(),
                    None => {
                        log::error!(
                            "[powershell registry] TaskNotificationQueue not in app state — refusing to register tool"
                        );
                        return None;
                    }
                };
                Some(Arc::new(builtin::powershell::PowerShellTool::new(
                    task_store,
                    notif_queue,
                ))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "create_agenda_item"
            | "list_agenda_items"
            | "update_agenda_item"
            | "cancel_agenda_item"
            | "skip_occurrence"
            | "list_agenda_occurrences" => {
                let deps = Self::try_build_agenda_deps(ctx)?;
                Some(Self::make_agenda_tool(name, deps))
            }
            "RefreshSkills" => {
                let app = ctx.app_handle.as_ref()?;
                Some(
                    Arc::new(builtin::refresh_skills::RefreshSkillsTool::new(Arc::new(
                        AppSkillRegistryRefresher { app: app.clone() },
                    ))) as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            _ => None,
        }
    }

    fn try_build_agenda_deps(
        ctx: &RequestScopedRuntimeDeps,
    ) -> Option<Arc<crate::runtime::tools::builtin::agenda::AgendaToolDeps>> {
        // user-scoped 根目录走 ctx.storage（同 builtin::memory 的 WriteMemory/SearchMemory）
        let base_dir = ctx.storage.base_dir().to_path_buf();
        // 任务 45 注入的 active persona id；未解析（test/legacy）→ 拒绝构造工具
        let persona_id = ctx.current_persona_id.clone()?;
        Some(Arc::new(
            crate::runtime::tools::builtin::agenda::AgendaToolDeps::new(base_dir, persona_id),
        ))
    }

    fn make_agenda_tool(
        name: &str,
        deps: Arc<crate::runtime::tools::builtin::agenda::AgendaToolDeps>,
    ) -> Arc<dyn crate::runtime::tools::RuntimeTool> {
        use crate::runtime::tools::builtin::agenda::*;
        match name {
            "create_agenda_item" => Arc::new(CreateAgendaItemRuntimeTool { deps }),
            "list_agenda_items" => Arc::new(ListAgendaItemsRuntimeTool { deps }),
            "update_agenda_item" => Arc::new(UpdateAgendaItemRuntimeTool { deps }),
            "cancel_agenda_item" => Arc::new(CancelAgendaItemRuntimeTool { deps }),
            "skip_occurrence" => Arc::new(SkipOccurrenceRuntimeTool { deps }),
            "list_agenda_occurrences" => Arc::new(ListAgendaOccurrencesRuntimeTool { deps }),
            _ => unreachable!("agenda tool name list out of sync"),
        }
    }
}

// ─────────────────────────────────────────────────
// SkillRegistry
// ─────────────────────────────────────────────────

struct RegisteredSkill {
    skill: Arc<dyn Skill>,
    source: String,
}

/// Runtime registry for skill plugins.
pub struct SkillRegistry {
    skills: RwLock<HashMap<String, RegisteredSkill>>,
    default_skill_id: String,
}

impl SkillRegistry {
    pub fn new(default_skill_id: &str) -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            default_skill_id: default_skill_id.to_string(),
        }
    }

    /// Register a skill plugin.
    pub async fn register(&self, skill: Arc<dyn Skill>, source: &str) {
        let id = skill.id().to_string();
        log::info!(
            "Registering skill: {} '{}' (source: {})",
            id,
            skill.display_name(),
            source
        );
        let mut skills = self.skills.write().await;
        skills.insert(
            id,
            RegisteredSkill {
                skill,
                source: source.to_string(),
            },
        );
    }

    /// Get a Skill by ID.
    pub async fn get(&self, id: &str) -> Option<Arc<dyn Skill>> {
        let skills = self.skills.read().await;
        skills.get(id).map(|rs| rs.skill.clone())
    }

    /// Get the default Skill. Returns `None` if the default skill is not registered.
    pub async fn get_default(&self) -> Option<Arc<dyn Skill>> {
        self.get(&self.default_skill_id).await
    }

    /// The default skill ID.
    pub fn default_skill_id(&self) -> &str {
        &self.default_skill_id
    }

    /// Build a markdown skill catalog for stateless skill discovery.
    ///
    /// The default assistant is omitted; only specialist skills are listed.
    pub async fn build_catalog_markdown(&self) -> String {
        let skills = self.skills.read().await;
        let mut entries: Vec<_> = skills
            .values()
            .filter(|rs| rs.skill.id() != self.default_skill_id)
            .collect();

        if entries.is_empty() {
            return String::new();
        }

        entries.sort_by(|a, b| a.skill.id().cmp(b.skill.id()));

        let mut md = String::from("## 可用专项技能\n\n");
        md.push_str(
            "当用户的需求与以下某个技能的领域匹配时，请调用 `load_skill` 工具加载详细指令。\n\n",
        );
        for rs in entries {
            let skill = &rs.skill;
            let desc = if !skill.short_description().is_empty() {
                skill.short_description()
            } else {
                skill.description()
            };
            md.push_str(&format!(
                "- `{}` — {} {}: {}\n",
                skill.id(),
                skill.icon(),
                skill.display_name(),
                desc
            ));
        }
        md.push_str("\n如果没有匹配的技能，直接用通用能力回答。\n");
        md
    }

    /// Unregister a skill by ID.
    pub async fn unregister(&self, id: &str) {
        let mut skills = self.skills.write().await;
        if skills.remove(id).is_some() {
            log::info!("Unregistered skill: {}", id);
        }
    }

    /// List all registered skills (for management UI).
    pub async fn list(&self) -> Vec<SkillInfo> {
        let skills = self.skills.read().await;
        skills
            .values()
            .map(|rs| SkillInfo {
                id: rs.skill.id().to_string(),
                display_name: rs.skill.display_name().to_string(),
                description: rs.skill.description().to_string(),
                source: rs.source.clone(),
                has_workflow: rs.skill.workflow().is_some(),
                icon: rs.skill.icon().to_string(),
                short_description: rs.skill.short_description().to_string(),
                trigger_text: rs.skill.trigger_text().to_string(),
                category: rs.skill.category().to_string(),
                display_name_en: rs.skill.display_name_en().to_string(),
                short_description_en: rs.skill.short_description_en().to_string(),
            })
            .collect()
    }
}

#[cfg(test)]
mod current_persona_id_tests {
    use super::*;

    fn make_plugin_ctx(persona_id: Option<String>) -> crate::plugin::context::PluginContext {
        let tmp = tempfile::TempDir::new().unwrap();
        let storage = Arc::new(crate::storage::file_store::AppStorage::new(tmp.path()).unwrap());
        let file_manager = Arc::new(crate::storage::file_manager::FileManager::new(tmp.path()));
        let mut ctx = crate::plugin::context::PluginContext {
            storage,
            file_manager,
            workspace_path: tmp.path().to_path_buf(),
            conversation_id: "conv-current-persona-test".to_string(),
            session_id: crate::runtime::ids::SessionId::new("conv-current-persona-test"),
            run_id: None,
            agent_id: None,
            app_handle: None,
            auth_manager: None,
            dingtalk_bridge: None,
            model: String::new(),
            gateway: None,
            tool_registry: None,
            app_settings: None,
            agent_runtime: None,
            event_bus: None,
            skill_registry: None,
            authorized_workspace: None,
            read_file_state: None,
            cancellation: None,
            permission_mode: crate::runtime::tools::permission::PermissionMode::Default,
            runtime_resolver: None,
            permission_ctx: None,
            current_persona_id: None,
        };
        ctx.current_persona_id = persona_id;
        // 顺手保留 tmp 直到测试结束
        std::mem::forget(tmp);
        ctx
    }

    #[test]
    fn from_plugin_context_propagates_current_persona_id() {
        let plugin = make_plugin_ctx(Some("alice".into()));
        let deps = RequestScopedRuntimeDeps::from_plugin_context(&plugin);
        assert_eq!(deps.current_persona_id.as_deref(), Some("alice"));
    }

    #[test]
    fn from_plugin_context_persona_defaults_to_none() {
        let plugin = make_plugin_ctx(None);
        let deps = RequestScopedRuntimeDeps::from_plugin_context(&plugin);
        assert!(deps.current_persona_id.is_none());
    }
}

#[cfg(test)]
mod skill_registry_tests {
    use super::*;
    use crate::plugin::skill_trait::{Skill, SkillState, ToolFilter};
    use std::sync::Arc;

    struct MockSkill {
        id: String,
        name: String,
        desc: String,
        short_desc: String,
        icon_str: String,
    }

    impl MockSkill {
        fn new(id: &str, name: &str, desc: &str) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                desc: desc.to_string(),
                short_desc: desc.to_string(),
                icon_str: "📋".to_string(),
            }
        }
    }

    impl Skill for MockSkill {
        fn id(&self) -> &str {
            &self.id
        }

        fn display_name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.desc
        }

        fn short_description(&self) -> &str {
            &self.short_desc
        }

        fn icon(&self) -> &str {
            &self.icon_str
        }

        fn system_prompt(&self, _: &SkillState) -> String {
            String::new()
        }

        fn tool_filter(&self, _: &SkillState) -> ToolFilter {
            ToolFilter::All
        }
    }

    #[tokio::test]
    async fn build_catalog_empty_when_no_non_default_skills() {
        let registry = SkillRegistry::new("daily-assistant");
        registry
            .register(
                Arc::new(MockSkill::new("daily-assistant", "Daily", "default")),
                "builtin",
            )
            .await;

        let catalog = registry.build_catalog_markdown().await;

        assert!(catalog.is_empty());
    }

    #[tokio::test]
    async fn build_catalog_excludes_default_includes_others() {
        let registry = SkillRegistry::new("daily-assistant");
        registry
            .register(
                Arc::new(MockSkill::new("daily-assistant", "Daily", "default")),
                "builtin",
            )
            .await;
        registry
            .register(
                Arc::new(MockSkill::new("biz-writing", "商务写作", "邮件/报告")),
                "plugin",
            )
            .await;

        let catalog = registry.build_catalog_markdown().await;

        assert!(catalog.contains("biz-writing"));
        assert!(catalog.contains("商务写作"));
        assert!(!catalog.contains("daily-assistant"));
    }

    #[tokio::test]
    async fn build_catalog_sorted_by_id() {
        let registry = SkillRegistry::new("daily-assistant");
        registry
            .register(
                Arc::new(MockSkill::new("daily-assistant", "Daily", "default")),
                "builtin",
            )
            .await;
        registry
            .register(
                Arc::new(MockSkill::new("zzz-skill", "ZZZ", "last")),
                "plugin",
            )
            .await;
        registry
            .register(
                Arc::new(MockSkill::new("aaa-skill", "AAA", "first")),
                "plugin",
            )
            .await;

        let catalog = registry.build_catalog_markdown().await;

        let pos_aaa = catalog.find("aaa-skill").unwrap();
        let pos_zzz = catalog.find("zzz-skill").unwrap();
        assert!(pos_aaa < pos_zzz);
    }
}
