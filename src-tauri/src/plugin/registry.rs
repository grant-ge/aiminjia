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
use crate::runtime::tools::{LegacyToolAdapter, ToolDispatcher};

use super::context::PluginContext;
use super::skill_trait::{Skill, ToolFilter};
use super::tool_trait::{ToolError, ToolOutput, ToolPlugin};

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

        // Legacy tools not yet migrated: get schema from plugin
        for rt in legacy_tools.values() {
            let name = rt.plugin.name();
            if !runtime_tools.contains_key(name) {
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

        // Legacy tools not yet migrated
        for rt in legacy_tools.values() {
            let name = rt.plugin.name();
            if runtime_tools.contains_key(name) {
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
    /// The read lock is released before calling `execute()` so that
    /// long-running tools (Python subprocess, web search) do not block
    /// concurrent `register()`/`unregister()` calls.
    pub async fn execute(
        &self,
        name: &str,
        ctx: &PluginContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
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
        let dispatcher = Arc::new(ToolDispatcher::allow_all());
        let runtime_tools = self.runtime_tools.read().await;
        let legacy_tools = self.tools.read().await;

        // 1. Register native RuntimeTools first (they take priority)
        for (_, tool) in runtime_tools.iter() {
            dispatcher.register(tool.clone());
        }

        // 2. Register legacy ToolPlugin tools that have NOT been migrated
        //    (i.e., not already covered by a RuntimeTool with the same name)
        for rt in legacy_tools.values() {
            let name = rt.plugin.name();
            if !runtime_tools.contains_key(name) {
                dispatcher.register(Arc::new(LegacyToolAdapter::from_plugin(
                    rt.plugin.clone(),
                    plugin_ctx.clone(),
                )));
            }
        }

        dispatcher
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
