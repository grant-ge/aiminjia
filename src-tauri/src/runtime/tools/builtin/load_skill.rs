//! Stateless skill instruction loading.
//!
//! `load_skill` returns a skill's prompt body as a tool result. It does not
//! mutate session state, change the system prompt, restrict tools, or emit a
//! SkillRuntimePatch.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::plugin::SkillRegistry;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct LoadSkillRuntimeTool {
    skill_registry: Arc<SkillRegistry>,
    /// Snapshot of loadable specialist skill IDs for the definition text.
    skill_ids: Vec<String>,
}

impl LoadSkillRuntimeTool {
    pub async fn new(skill_registry: Arc<SkillRegistry>) -> Self {
        let default_id = skill_registry.default_skill_id().to_string();
        let mut skill_ids = skill_registry
            .list()
            .await
            .into_iter()
            .map(|skill| skill.id)
            .filter(|id| id != &default_id)
            .collect::<Vec<_>>();
        skill_ids.sort();
        Self {
            skill_registry,
            skill_ids,
        }
    }

    fn available_skill_ids(&self) -> String {
        if self.skill_ids.is_empty() {
            "无可用专项技能".to_string()
        } else {
            self.skill_ids.join(", ")
        }
    }
}

#[async_trait]
impl RuntimeTool for LoadSkillRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        let description = format!(
            "加载一个专项技能的详细指令到当前对话。当用户需求匹配技能目录中的某个专项技能时，\
             调用此工具并传入 skill_id。无副作用：不改变系统提示、不限制工具、不持久化。\
             可用 skill_id：{}。",
            self.available_skill_ids()
        );

        ToolDefinition::new("load_skill", description)
            .with_kind(ToolKind::Support)
            .with_read_only(true)
            .with_max_result_size_chars(16_000)
            .with_preserve_tool_use_results(true)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let skill_id = input
            .get("skill_id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: skill_id".into()))?;

        if !self.skill_ids.iter().any(|id| id == skill_id) {
            return Err(ToolError::ExecutionFailed(format!(
                "Unknown or unavailable skill: {}. Available skills: {}",
                skill_id,
                self.available_skill_ids()
            )));
        }

        let skill =
            self.skill_registry.get(skill_id).await.ok_or_else(|| {
                ToolError::ExecutionFailed(format!("Unknown skill: {}", skill_id))
            })?;
        let body = skill.body_prompt();
        let content = if body.trim().is_empty() {
            format!(
                "## {} ({})\n\n该技能没有详细指令。请根据技能描述执行：{}",
                skill.display_name(),
                skill_id,
                skill.description()
            )
        } else {
            format!("## {} ({})\n\n{}", skill.display_name(), skill_id, body)
        };

        Ok(ToolResult::new(
            "load_skill",
            content,
            Some(json!({
                "skill_id": skill_id,
                "display_name": skill.display_name(),
            })),
        ))
    }
}
