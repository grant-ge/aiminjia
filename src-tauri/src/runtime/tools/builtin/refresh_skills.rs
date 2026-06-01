//! refresh_skills — LLM 工具：通知 app 重新扫盘 user_skills_dir 和
//! global_skills_dir，把磁盘上新的 SKILL.md 更新到内存 SkillRegistry。
//!
//! 主要由 skill-creator 在 install 后调用（见 SKILL.md step 8）：
//!
//!   Bash(lotus_skill.py install ...)
//!   refresh_skills()                  <-- 这里
//!   <下一 turn catalog 已含 new skill>
//!
//! 也可被其他对话场景使用：用户手动 cp 装 skill 后让 AI 通知 app 刷新。

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct RefreshSkillsTool {
    app: Arc<AppHandle>,
}

impl RefreshSkillsTool {
    pub fn new(app: Arc<AppHandle>) -> Self {
        Self { app }
    }
}

#[async_trait]
impl RuntimeTool for RefreshSkillsTool {
    fn id(&self) -> &str {
        "refresh_skills"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        ToolDefinition::new(
            "refresh_skills",
            "通知 AIjia 重新扫描用户技能目录，让新装的技能立刻在对话和技能中心可见。\
             用法：刚通过 lotus_skill.py install 或别的方式装完技能后调用一次。\
             无参数。返回成功后下一 turn 的 catalog 含新技能。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(false)
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        match crate::commands::skill_management::refresh_skill_registry(&self.app) {
            Ok(()) => Ok(ToolResult::new(
                "refresh_skills",
                "✅ Skill registry refreshed. 新装的技能下一 turn 可用。".to_string(),
                Some(json!({ "refreshed": true })),
            )),
            Err(e) => Ok(ToolResult::new(
                "refresh_skills",
                format!("⚠️ Refresh failed: {}. 重试或重启 app。", e),
                Some(json!({ "refreshed": false, "error": e })),
            )),
        }
    }
}
