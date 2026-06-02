//! Stateless skill instruction loading via SKILL.md system.
//!
//! `load_skill` returns a skill's prompt body as a tool result. It does not
//! mutate session state, change the system prompt, or restrict tools.
//!
//! On registry miss it transparently retries once after a refresh — covers
//! the "same-turn install then use" case (LLM runs lotus_skill.py install
//! → immediately calls Skill('new-skill') before RefreshSkills RuntimeTool
//! runs). Throttled to at most one refresh per 5 seconds to avoid abuse.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::substitution::{substitute_skill_body, SkillSubstitutionContext};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

/// Format the result of a forked skill execution.
pub fn format_fork_result(skill_name: &str, result_text: &str) -> String {
    format!(
        "Skill \"{}\" completed (forked execution).\n\nResult:\n{}",
        skill_name, result_text
    )
}

/// Throttle for miss-retry: at most one refresh per 5 seconds.
const REFRESH_THROTTLE: Duration = Duration::from_secs(5);

pub struct LoadSkillRuntimeTool {
    skill_registry: Arc<Mutex<SkillRegistry>>,
    /// 用于触发 refresh_skill_registry 的 AppHandle。test / legacy 路径
    /// 可以传 None，此时 miss-retry 会跳过 refresh，直接走原 "not found" 路径。
    app_handle: Option<Arc<AppHandle>>,
    /// 最近一次因 miss 触发的 refresh 时间。throttle 用。
    last_refresh: Arc<Mutex<Option<Instant>>>,
}

impl LoadSkillRuntimeTool {
    pub fn new(skill_registry: Arc<Mutex<SkillRegistry>>) -> Self {
        Self {
            skill_registry,
            app_handle: None,
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_app_handle(
        skill_registry: Arc<Mutex<SkillRegistry>>,
        app_handle: Arc<AppHandle>,
    ) -> Self {
        Self {
            skill_registry,
            app_handle: Some(app_handle),
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    /// 判断是否允许触发 refresh（throttle）。允许后立刻记录本次时间。
    fn try_acquire_refresh_slot(&self) -> bool {
        let mut last = self.last_refresh.lock().unwrap();
        let now = Instant::now();
        let allow = match *last {
            None => true,
            Some(t) => now.duration_since(t) >= REFRESH_THROTTLE,
        };
        if allow {
            *last = Some(now);
        }
        allow
    }
}

#[async_trait]
impl RuntimeTool for LoadSkillRuntimeTool {
    fn id(&self) -> &str {
        "Skill"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        let ids = self
            .skill_registry
            .lock()
            .map(|reg| reg.skill_ids().join(", "))
            .unwrap_or_default();
        let available = if ids.is_empty() {
            "无可用专项技能".to_string()
        } else {
            ids
        };

        let description = format!(
            "加载一个专项技能的详细指令到当前对话。当用户需求匹配技能目录中的某个专项技能时，\
             调用此工具并传入 skill_id。无副作用：不改变系统提示、不限制工具、不持久化。\
             可用 skill_id：{}。",
            available
        );

        ToolDefinition::new("Skill", description)
            .with_kind(ToolKind::Support)
            .with_read_only(true)
            .with_max_result_size_chars(16_000)
            .with_preserve_tool_use_results(true)
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let skill_id = input
            .get("skill_id")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: skill_id".into()))?;

        let args = input
            .get("args")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // Clone the DiskSkill out of the registry (short lock window)
        let skill = {
            let reg = self
                .skill_registry
                .lock()
                .map_err(|e| ToolError::ExecutionFailed(format!("Registry lock failed: {}", e)))?;
            reg.get(&skill_id).cloned()
        };

        // 兜底：registry miss 时，throttle 内尝试 refresh-then-retry。
        // 覆盖"LLM 同 turn install + 立即 Skill('new-skill')"的边缘场景。
        let skill = match skill {
            Some(s) => s,
            None => {
                if self.try_acquire_refresh_slot() {
                    if let Some(app) = self.app_handle.as_ref() {
                        let _ = crate::commands::skill_management::refresh_skill_registry(app);
                    }
                }
                // 重查
                let reg = self.skill_registry.lock().map_err(|e| {
                    ToolError::ExecutionFailed(format!("Registry lock failed: {}", e))
                })?;
                let available_ids = reg.skill_ids().join(", ");
                reg.get(&skill_id).cloned().ok_or_else(|| {
                    ToolError::ExecutionFailed(format!(
                        "Unknown or unavailable skill: {}. Available: {}",
                        skill_id, available_ids
                    ))
                })?
            }
        };

        // Check for fork mode (placeholder — full sub-agent wiring in follow-up)
        if skill.frontmatter.context.as_deref() == Some("fork") {
            // TODO: wire to AgentRuntime in follow-up
            let placeholder = format_fork_result(
                &skill.frontmatter.name,
                "fork mode: subagent dispatch will be wired in a follow-up task. Returning a placeholder body so the call doesn't fail.",
            );
            return Ok(ToolResult::new(
                "Skill",
                placeholder,
                Some(json!({
                    "skill_id": skill_id,
                    "display_name": skill.frontmatter.metadata.label.clone()
                        .unwrap_or_else(|| skill.frontmatter.name.clone()),
                    "context": "fork",
                })),
            ));
        }

        // Build substitution context
        let session_id_str = ctx.session_id.as_str().to_string();
        let sub_ctx = SkillSubstitutionContext {
            skill_dir: skill.root.clone(),
            session_id: session_id_str,
            args,
            argument_names: skill.frontmatter.arguments.clone(),
            execute_shell: false,
        };

        let substituted_body = substitute_skill_body(&skill.body, &sub_ctx)
            .map_err(|e| ToolError::ExecutionFailed(format!("Body substitution failed: {}", e)))?;

        let content = format!(
            "## {} ({})\n\nBase directory for this skill: {}\n\n{}",
            skill.frontmatter.name,
            skill_id,
            skill.root.display(),
            substituted_body
        );

        // Track invoked skill
        {
            if let Ok(mut reg) = self.skill_registry.lock() {
                reg.remember_invoked(None, &skill_id, substituted_body.clone());
            }
        }

        Ok(ToolResult::new(
            "Skill",
            content,
            Some(json!({
                "skill_id": skill_id,
                "display_name": skill.frontmatter.metadata.label.clone()
                    .unwrap_or_else(|| skill.frontmatter.name.clone()),
            })),
        ))
    }
}
