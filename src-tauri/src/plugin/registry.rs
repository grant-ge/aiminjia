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
    pub tavily_api_key: Option<String>,
    pub bocha_api_key: Option<String>,
    pub app_handle: Option<tauri::AppHandle>,
    pub session_manager: Arc<crate::python::session::PythonSessionManager>,
    pub auth_manager: Option<Arc<crate::auth::AuthManager>>,
    pub connector_engine: Option<Arc<crate::connector::ConnectorEngine>>,
    pub use_cloud: bool,
    pub model: String,
    pub gateway: Option<Arc<crate::llm::gateway::LlmGateway>>,
    pub tool_registry: Option<Arc<crate::plugin::registry::ToolRegistry>>,
    pub app_settings: Option<Arc<crate::models::settings::AppSettings>>,
    pub agent_runtime: Option<Arc<crate::runtime::agent::AgentRuntime>>,
    pub event_bus: Option<crate::runtime::event_bus::RuntimeEventBus>,
    pub skill_registry:
        Option<Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>>,
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    pub read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    pub cancellation: Option<crate::runtime::cancellation::CancellationToken>,
    pub permission_mode: PermissionMode,
    pub runtime_resolver: Option<ManagedRuntimeResolver>,
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
            tavily_api_key: ctx.tavily_api_key.clone(),
            bocha_api_key: ctx.bocha_api_key.clone(),
            app_handle: ctx.app_handle.clone(),
            session_manager: ctx.session_manager.clone(),
            auth_manager: ctx.auth_manager.clone(),
            connector_engine: ctx.connector_engine.clone(),
            use_cloud: ctx.use_cloud,
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

    fn python_runtime(
        &self,
    ) -> crate::runtime::dependencies::RuntimeDependencyResult<(
        std::path::PathBuf,
        Option<std::path::PathBuf>,
    )> {
        if let Some(resolver) = &self.runtime_resolver {
            let deps = resolver.workspace_dependencies()?;
            return Ok((deps.python, None));
        }

        Err(
            crate::runtime::dependencies::RuntimeDependencyError::ResolverUnavailable(
                "RequestScopedRuntimeDeps has no RuntimeResolver".to_string(),
            ),
        )
    }
}

const REQUEST_SCOPED_RUNTIME_TOOL_NAMES: &[&str] = &[
    "web_search",
    "browse_navigate",
    "read_page_content",
    "page_execute_js",
    "extract_table_data",
    "extract_with_pagination",
    "browse_and_extract",
    "load_file",
    "browse_data",
    "spawn_subagent",
    "execute_python",
    "generate_report",
    "generate_chart",
    "write_memory",
    "search_memory",
    "load_skill",
];

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

        let def = tool.definition();
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
    pub async fn get_schemas_filtered(&self, filter: &ToolFilter) -> Vec<ToolDefinition> {
        use crate::runtime::tools::catalog::TOOL_CATALOG;
        let runtime_tools = self.runtime_tools.read().await;
        let legacy_tools = self.tools.read().await;
        let mut schemas = Vec::new();

        // Runtime tools: filter and get schema from catalog
        for (id, _) in runtime_tools.iter() {
            let matches = match filter {
                ToolFilter::All => true,
                ToolFilter::Only(names) => names.iter().any(|n| n == id),
                ToolFilter::Exclude(names) => names.iter().all(|n| n != id),
            };
            if matches {
                if let Some(entry) = TOOL_CATALOG.get_entry(id) {
                    schemas.push(ToolDefinition {
                        name: entry.definition.id.clone(),
                        description: entry.definition.description.clone(),
                        parameters: entry.json_schema.clone(),
                    });
                }
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
                if let Some(entry) = TOOL_CATALOG.get_entry(id) {
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

        partition_sort_tool_schemas(schemas)
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
                };
                let browser_available = ctx.connector_engine.is_some();
                let file_ops = if name == "load_file" {
                    let (python_binary, python_home) = ctx
                        .python_runtime()
                        .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
                    Some(
                        Arc::new(crate::runtime::tools::capability::DefaultFileOperations {
                            storage: ctx.storage.clone(),
                            file_manager: ctx.file_manager.clone(),
                            workspace_path: ctx.workspace_path.clone(),
                            conversation_id: ctx.conversation_id.clone(),
                            run_id: ctx.run_id.clone(),
                            python_binary: Some(python_binary),
                            python_home,
                        })
                            as Arc<dyn crate::runtime::tools::capability::FileOperations>,
                    )
                } else {
                    None
                };
                let cap = CapabilityContext {
                    storage: Some(storage),
                    workspace_id: Some(ctx.conversation_id.clone()),
                    browser_available,
                    file_ops,
                    read_file_state: ctx.read_file_state.clone(),
                    file_reading_limits: Some(
                        crate::runtime::tools::capability::FileReadingLimits::default(),
                    ),
                    notification_sink: None,
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
            let def = tool.definition();
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
                tavily_api_key: ctx.tavily_api_key.clone(),
                bocha_api_key: ctx.bocha_api_key.clone(),
                app_handle: ctx.app_handle.clone(),
                session_manager: ctx.session_manager.clone(),
                auth_manager: ctx.auth_manager.clone(),
                connector_engine: ctx.connector_engine.clone(),
                use_cloud: ctx.use_cloud,
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
    /// `Deps` structs carry session-level state (`conversation_id`, `run_id`,
    /// `connector_engine`) that cannot be stored in the global singleton registry.
    ///
    /// Returns `None` for unknown tool names (falls through to legacy path).
    /// For browser tools, also returns `None` when `connector_engine` is absent.
    async fn try_build_request_scoped_tool(
        name: &str,
        ctx: &RequestScopedRuntimeDeps,
    ) -> Option<Arc<dyn crate::runtime::tools::RuntimeTool>> {
        use crate::runtime::tools::builtin;
        use std::sync::Arc;

        match name {
            "web_search" => {
                let deps = builtin::network::SearchDeps {
                    tavily_api_key: ctx.tavily_api_key.clone(),
                    bocha_api_key: ctx.bocha_api_key.clone(),
                    use_cloud: ctx.use_cloud,
                    auth_manager: ctx.auth_manager.clone(),
                };
                Some(Arc::new(builtin::network::WebSearchRuntimeTool::new(deps)))
            }
            "browse_navigate" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(
                    Arc::new(builtin::browser::BrowseNavigateRuntimeTool::new(deps))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "read_page_content" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(
                    Arc::new(builtin::browser::ReadPageContentRuntimeTool::new(deps))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "page_execute_js" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(
                    Arc::new(builtin::browser::PageExecuteJsRuntimeTool::new(deps))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "extract_table_data" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(
                    Arc::new(builtin::browser::ExtractTableDataRuntimeTool::new(deps))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "extract_with_pagination" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(
                    Arc::new(builtin::browser::ExtractWithPaginationRuntimeTool::new(
                        deps,
                    )) as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "browse_and_extract" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(
                    Arc::new(builtin::browser::BrowseAndExtractRuntimeTool::new(deps))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "load_file" => Some(Arc::new(builtin::file::LoadFileRuntimeTool::new())),
            "browse_data" => Some(Arc::new(
                builtin::browse_data::BrowseDataRuntimeTool::with_launcher(Arc::new(
                    crate::llm::tool_executor::DefaultBrowseDataLauncher::from_runtime_deps(
                        ctx.clone(),
                    ),
                )),
            ) as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "spawn_subagent" => {
                use tauri::Manager;

                // Fail-closed: if app state is missing any of the three Arcs,
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
            "task_output" => {
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
                Some(Arc::new(builtin::task_output::TaskOutputRuntimeTool::new(resolver))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "execute_python" => {
                use crate::runtime::tools::builtin::python_execution::DefaultPythonExecution;

                let (python_binary, python_home) = match ctx.python_runtime() {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        return Some(Arc::new(builtin::python::ExecutePythonRuntimeTool::error(
                            err.to_string(),
                        ))
                            as Arc<dyn crate::runtime::tools::RuntimeTool>);
                    }
                };
                let python = Arc::new(DefaultPythonExecution::new(
                    ctx.session_manager.clone(),
                    python_binary.clone(),
                    python_home.clone(),
                ));
                Some(Arc::new(
                    builtin::python::ExecutePythonRuntimeTool::with_runtime_deps(
                        python,
                        ctx.storage.clone(),
                        ctx.file_manager.clone(),
                        ctx.run_id.clone(),
                        ctx.model.clone(),
                        python_binary,
                        python_home,
                    ),
                )
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "generate_report" => {
                use crate::runtime::tools::builtin::report_capability::DefaultReportCapability;

                let (python_binary, python_home) = match ctx.python_runtime() {
                    Ok(runtime) => runtime,
                    Err(_) => return None,
                };
                let capability = Arc::new(DefaultReportCapability {
                    storage: ctx.storage.clone(),
                    file_manager: ctx.file_manager.clone(),
                    auth_manager: ctx.auth_manager.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    python_binary,
                    python_home,
                });
                Some(
                    Arc::new(builtin::report::GenerateReportRuntimeTool::with_capability(
                        capability,
                    )) as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "generate_chart" => {
                use crate::runtime::tools::builtin::chart_capability::DefaultChartCapability;

                let (python_binary, python_home) = match ctx.python_runtime() {
                    Ok(runtime) => runtime,
                    Err(_) => return None,
                };
                let capability = Arc::new(DefaultChartCapability {
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    python_binary,
                    python_home,
                });
                Some(
                    Arc::new(builtin::chart::GenerateChartRuntimeTool::with_capability(
                        capability,
                    )) as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            "write_memory" => Some(Arc::new(builtin::memory::WriteMemoryRuntimeTool::new(
                builtin::memory::MemoryDeps {
                    app_data_dir: ctx.storage.base_dir().to_path_buf(),
                    workspace_path: ctx.workspace_path.clone(),
                },
            ))
                as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "search_memory" => Some(Arc::new(builtin::memory::SearchMemoryRuntimeTool::new(
                builtin::memory::MemoryDeps {
                    app_data_dir: ctx.storage.base_dir().to_path_buf(),
                    workspace_path: ctx.workspace_path.clone(),
                },
            ))
                as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "load_skill" => {
                let registry = ctx.skill_registry.clone()?;
                Some(
                    Arc::new(builtin::load_skill::LoadSkillRuntimeTool::new(registry))
                        as Arc<dyn crate::runtime::tools::RuntimeTool>,
                )
            }
            _ => None,
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
