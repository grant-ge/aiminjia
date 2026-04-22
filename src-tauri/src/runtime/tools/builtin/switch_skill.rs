use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::plugin::SkillRegistry;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::transport::tauri_commands::chat::chat_runtime_impl;

pub struct SwitchSkillRuntimeTool {
    skill_registry: Arc<SkillRegistry>,
    skill_sessions: Arc<crate::runtime::chat::SkillSessionStore>,
    tool_registry: Arc<crate::plugin::ToolRegistry>,
}

impl SwitchSkillRuntimeTool {
    pub fn new(
        skill_registry: Arc<SkillRegistry>,
        skill_sessions: Arc<crate::runtime::chat::SkillSessionStore>,
        tool_registry: Arc<crate::plugin::ToolRegistry>,
    ) -> Self {
        Self {
            skill_registry,
            skill_sessions,
            tool_registry,
        }
    }
}

#[async_trait]
impl RuntimeTool for SwitchSkillRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("switch_skill")
            .unwrap_or_else(|| ToolDefinition::new("switch_skill", "Switch skill"))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let skill_id = input
            .get("skill_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: skill_id".into()))?;
        self.skill_registry
            .get(skill_id)
            .await
            .ok_or_else(|| ToolError::ExecutionFailed(format!("Unknown skill: {}", skill_id)))?;
        let all_tools = self
            .tool_registry
            .get_all_schemas()
            .await
            .into_iter()
            .map(|def| def.name)
            .collect::<Vec<_>>();
        let has_authorized_workspace = _ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.storage.as_ref())
            .and_then(|storage| storage.authorized_workspace.as_ref())
            .is_some();
        let skill_ctx = self
            .skill_sessions
            .switch_skill(
                self.skill_registry.as_ref(),
                &all_tools,
                _ctx.session_id.as_str(),
                skill_id,
                false,
            )
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
        let allowed_tool_set = skill_ctx.allowed_tools.as_ref();
        let visible_tool_defs = chat_runtime_impl::build_visible_tool_defs(
            self.tool_registry.as_ref(),
            has_authorized_workspace,
            allowed_tool_set,
        )
        .await;
        let tool_defs = visible_tool_defs
            .into_iter()
            .filter_map(|definition| serde_json::to_value(&definition).ok())
            .collect::<Vec<_>>();
        let patch = json!({
            "skill_control": {
                "skill_id": skill_ctx.skill_id,
                "system_prompt": skill_ctx.system_prompt,
                "allowed_tools": skill_ctx.allowed_tools.map(|set| {
                    let mut names = set.into_iter().collect::<Vec<_>>();
                    names.sort();
                    names
                }),
                "tool_defs": tool_defs,
                "max_iterations": self
                    .skill_registry
                    .get(skill_id)
                    .await
                    .map(|skill| skill.max_iterations(&skill_ctx.state))
                    .unwrap_or(10),
                "token_budget": self
                    .skill_registry
                    .get(skill_id)
                    .await
                    .map(|skill| skill.token_budget(&skill_ctx.state))
                    .unwrap_or(4096),
            }
        });

        Ok(ToolResult::new(
            "switch_skill",
            format!("Switched to skill '{}'.", skill_id),
            Some(patch),
        ))
    }
}
