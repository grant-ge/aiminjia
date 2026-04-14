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
use crate::runtime::tools::{
    CapabilityPermissionPipeline, LegacyToolAdapter, PermissionPipeline, ToolDispatcher,
};
use crate::runtime::tools::capability::{CapabilityContext, StorageCapability};

use super::context::PluginContext;
use super::skill_trait::{Skill, ToolFilter};
use super::tool_trait::{ToolError, ToolOutput, ToolPlugin};

const REQUEST_SCOPED_RUNTIME_TOOL_NAMES: &[&str] = &[
    "web_search",
    "browse_navigate",
    "read_page_content",
    "page_execute_js",
    "extract_table_data",
    "extract_with_pagination",
    "load_file",
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
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            runtime_tools: RwLock::new(HashMap::new()),
        }
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
        let id = tool.definition().id.clone();
        log::info!("Registering runtime tool: {}", id);
        self.runtime_tools.write().await.insert(id, tool);
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

        schemas
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

        schemas
    }

    /// Execute a tool by name.
    ///
    /// Runtime-first: if the tool is registered as a `RuntimeTool`, it is
    /// executed via `CapabilityPermissionPipeline` using a `ToolExecutionContext`
    /// built from `PluginContext`.  Falls back to legacy `ToolPlugin` if no
    /// runtime tool is found.
    ///
    /// The read lock is released before calling `execute()` so that
    /// long-running tools (Python subprocess, web search) do not block
    /// concurrent `register()`/`unregister()` calls.
    pub async fn execute(
        &self,
        name: &str,
        ctx: &PluginContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        // Step 1: Check global runtime_tools (stateless, e.g. workspace tools)
        let runtime_tool: Option<Arc<dyn crate::runtime::tools::RuntimeTool>> = {
            let rt = self.runtime_tools.read().await;
            rt.get(name).cloned()
        };

        // Step 2: Try request-level factory (session-scoped deps built from PluginContext)
        // TRANSITIONAL: BrowserDeps / LoadFileDeps carry conversation_id / run_id which
        // cannot be stored in the global singleton registry.
        let runtime_tool = runtime_tool.or_else(|| Self::try_build_request_scoped_tool(name, ctx));

        if let Some(tool) = runtime_tool {
            // Build CapabilityContext from PluginContext fields
            let capability = {
                let storage = StorageCapability {
                    workspace_path: ctx.workspace_path.clone(),
                    authorized_workspace: ctx.authorized_workspace.clone(),
                };
                let browser_available = ctx.connector_engine.is_some();
                let cap = CapabilityContext {
                    storage: Some(storage),
                    workspace_id: Some(ctx.conversation_id.clone()),
                    browser_available,
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
                crate::runtime::cancellation::CancellationToken::new(),
            )
            .with_capability(capability);

            // Permission check via CapabilityPermissionPipeline
            let pipeline = CapabilityPermissionPipeline;
            let def = tool.definition();
            pipeline
                .authorize(&def, &input, &exec_ctx)
                .map_err(|e| ToolError::ExecutionFailed(format!("Permission denied: {}", e)))?;

            let result = tool
                .execute(input, exec_ctx)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

            let mut output = ToolOutput::success(result.content);
            output.data = result.data;
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
        let dispatcher = ToolDispatcher::allow_all();
        dispatcher.register(Arc::new(LegacyToolAdapter::from_plugin(
            plugin,
            ctx.clone(),
        )));
        let runtime_ctx = crate::runtime::tools::ToolExecutionContext::new(
            ctx.session_id.clone(),
            ctx.run_id.clone().unwrap_or_else(|| {
                crate::runtime::ids::RunId::new(format!("run-{}", ctx.conversation_id))
            }),
            ctx.agent_id.clone(),
            format!("tool-{}", name),
            crate::runtime::cancellation::CancellationToken::new(),
        );
        let outcome = dispatcher
            .dispatch(name, input, runtime_ctx)
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
        let mut output = ToolOutput::success(outcome.result.content);
        output.data = outcome.result.data;
        Ok(output)
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

    /// Build a runtime-first dispatcher while keeping legacy tool implementations
    /// behind an adapter. This is the bridge point for incrementally moving the
    /// production query path onto the new runtime contract.
    pub async fn to_runtime_dispatcher(&self, plugin_ctx: PluginContext) -> Arc<ToolDispatcher> {
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(CapabilityPermissionPipeline)));
        let runtime_tools = self.runtime_tools.read().await;
        let legacy_tools = self.tools.read().await;
        let mut request_scoped_registered = std::collections::HashSet::new();

        // 1. Register native RuntimeTools first (they take priority)
        for (_, tool) in runtime_tools.iter() {
            dispatcher.register(tool.clone());
        }

        // 2. Register request-scoped RuntimeTools built from PluginContext.
        for tool_name in REQUEST_SCOPED_RUNTIME_TOOL_NAMES {
            if runtime_tools.contains_key(*tool_name) {
                continue;
            }
            if let Some(tool) = Self::try_build_request_scoped_tool(tool_name, &plugin_ctx) {
                dispatcher.register(tool);
                request_scoped_registered.insert((*tool_name).to_string());
            }
        }

        // 3. Register legacy ToolPlugin tools that have NOT been migrated
        //    (i.e., not already covered by a RuntimeTool with the same name)
        for rt in legacy_tools.values() {
            let name = rt.plugin.name();
            if !runtime_tools.contains_key(name) && !request_scoped_registered.contains(name) {
                dispatcher.register(Arc::new(LegacyToolAdapter::from_plugin(
                    rt.plugin.clone(),
                    plugin_ctx.clone(),
                )));
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
    fn try_build_request_scoped_tool(
        name: &str,
        ctx: &PluginContext,
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
                Some(Arc::new(builtin::browser::BrowseNavigateRuntimeTool::new(deps))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "read_page_content" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(Arc::new(builtin::browser::ReadPageContentRuntimeTool::new(deps))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "page_execute_js" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(Arc::new(builtin::browser::PageExecuteJsRuntimeTool::new(deps))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "extract_table_data" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(Arc::new(builtin::browser::ExtractTableDataRuntimeTool::new(deps))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "extract_with_pagination" => {
                let deps = builtin::browser::BrowserDeps {
                    connector_engine: ctx.connector_engine.clone(),
                    file_manager: ctx.file_manager.clone(),
                    storage: ctx.storage.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                };
                Some(Arc::new(builtin::browser::ExtractWithPaginationRuntimeTool::new(deps))
                    as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            "load_file" => {
                let deps = builtin::file::LoadFileDeps {
                    storage: ctx.storage.clone(),
                    file_manager: ctx.file_manager.clone(),
                    workspace_path: ctx.workspace_path.clone(),
                    session_manager: ctx.session_manager.clone(),
                    conversation_id: ctx.conversation_id.clone(),
                    run_id: ctx.run_id.clone(),
                };
                Some(Arc::new(builtin::file::LoadFileRuntimeTool::new(deps)))
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

    /// Detect which Skill should activate for a message.
    ///
    /// Returns the ID of the highest-priority matching Skill, or None
    /// if the current Skill should remain active.
    pub async fn detect_activation(
        &self,
        message: &str,
        has_files: bool,
        current_skill_id: &str,
    ) -> Option<String> {
        let skills = self.skills.read().await;
        let mut best: Option<(u32, String)> = None;

        for rs in skills.values() {
            if rs
                .skill
                .should_activate(message, has_files, current_skill_id)
            {
                let priority = rs.skill.priority();
                let id = rs.skill.id().to_string();
                match &best {
                    Some((bp, _)) if priority <= *bp => {}
                    _ => best = Some((priority, id)),
                }
            }
        }

        best.map(|(_, id)| id)
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
