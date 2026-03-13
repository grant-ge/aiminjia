//! Plugin registries — runtime registration and lookup for Tools and Skills.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::llm::streaming::ToolDefinition;

use super::context::PluginContext;
use super::tool_trait::{ToolError, ToolOutput, ToolPlugin};
use super::skill_trait::{Skill, ToolFilter};

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
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
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
        tools.insert(name, RegisteredTool {
            plugin: tool,
            source: source.to_string(),
        });
    }

    /// Unregister a tool by name.
    pub async fn unregister(&self, name: &str) {
        let mut tools = self.tools.write().await;
        if tools.remove(name).is_some() {
            log::info!("Unregistered tool: {}", name);
        }
    }

    /// Get all tool definitions (for LLM context).
    pub async fn get_all_schemas(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        tools.values().map(|rt| ToolDefinition {
            name: rt.plugin.name().to_string(),
            description: rt.plugin.description().to_string(),
            parameters: rt.plugin.input_schema(),
        }).collect()
    }

    /// Get tool definitions filtered by a ToolFilter.
    pub async fn get_schemas_filtered(&self, filter: &ToolFilter) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        tools.values()
            .filter(|rt| match filter {
                ToolFilter::All => true,
                ToolFilter::Only(names) => names.iter().any(|n| n == rt.plugin.name()),
                ToolFilter::Exclude(names) => names.iter().all(|n| n != rt.plugin.name()),
            })
            .map(|rt| ToolDefinition {
                name: rt.plugin.name().to_string(),
                description: rt.plugin.description().to_string(),
                parameters: rt.plugin.input_schema(),
            })
            .collect()
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
            let rt = tools.get(name).ok_or_else(|| {
                ToolError::ExecutionFailed(format!("Unknown tool: {}", name))
            })?;
            rt.plugin.clone() // Arc::clone is cheap — release lock before executing
        };
        plugin.execute(ctx, input).await
    }

    /// List all registered tools (for management UI).
    pub async fn list(&self) -> Vec<ToolInfo> {
        let tools = self.tools.read().await;
        tools.values().map(|rt| ToolInfo {
            name: rt.plugin.name().to_string(),
            description: rt.plugin.description().to_string(),
            source: rt.source.clone(),
        }).collect()
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
        log::info!("Registering skill: {} '{}' (source: {})", id, skill.display_name(), source);
        let mut skills = self.skills.write().await;
        skills.insert(id, RegisteredSkill {
            skill,
            source: source.to_string(),
        });
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
            if rs.skill.should_activate(message, has_files, current_skill_id) {
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

    /// List all registered skills (for management UI).
    pub async fn list(&self) -> Vec<SkillInfo> {
        let skills = self.skills.read().await;
        skills.values().map(|rs| SkillInfo {
            id: rs.skill.id().to_string(),
            display_name: rs.skill.display_name().to_string(),
            description: rs.skill.description().to_string(),
            source: rs.source.clone(),
            has_workflow: rs.skill.workflow().is_some(),
            icon: rs.skill.icon().to_string(),
            short_description: rs.skill.short_description().to_string(),
            trigger_text: rs.skill.trigger_text().to_string(),
        }).collect()
    }
}
