//! RefreshSkills — LLM 工具：通知 app 重新扫盘 user_skills_dir 和
//! global_skills_dir，把磁盘上新增或修改后的 SKILL.md 更新到内存 SkillRegistry。
//!
//! 主要由 skill-creator 在 install 或修改已有技能后调用（见 SKILL.md step 8）：
//!
//!   Bash(lotus_skill.py install ...)
//!   RefreshSkills()                   <-- 这里
//!   <下一 turn catalog 和 Skill 加载内容已使用最新磁盘版本>
//!
//! 也可被其他对话场景使用：用户手动 cp、覆盖或编辑 skill 后让 AI 通知 app 刷新。
//!
//! 命名：PascalCase（`RefreshSkills`）对齐 daily 模式其它工具（Read/Write/Skill/...），
//! 避免 LLM 在调用时把 snake_case 误"自动校正"成驼峰名而失败。

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub trait SkillRegistryRefresher: Send + Sync + std::fmt::Debug {
    fn refresh_skill_registry(&self) -> Result<(), String>;
}

pub struct RefreshSkillsTool {
    refresher: Arc<dyn SkillRegistryRefresher>,
}

impl RefreshSkillsTool {
    pub fn new(refresher: Arc<dyn SkillRegistryRefresher>) -> Self {
        Self { refresher }
    }
}

#[async_trait]
impl RuntimeTool for RefreshSkillsTool {
    fn id(&self) -> &str {
        "RefreshSkills"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        ToolDefinition::new(
            "RefreshSkills",
            "通知 AIjia 重新扫描用户技能目录，让新增、覆盖或修改后的技能立刻在对话和技能中心可见。\
             用法：刚通过 lotus_skill.py install、覆盖技能目录、编辑 SKILL.md 或别的方式更改技能文件后调用一次。\
             无参数。返回成功后下一 turn 的 catalog 和 Skill 加载内容会使用最新磁盘版本。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(false)
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        match self.refresher.refresh_skill_registry() {
            Ok(()) => Ok(ToolResult::new(
                "RefreshSkills",
                "✅ Skill registry refreshed. 新增或修改后的技能下一 turn 可用。".to_string(),
                Some(json!({ "refreshed": true })),
            )),
            Err(e) => Ok(ToolResult::new(
                "RefreshSkills",
                format!("⚠️ Refresh failed: {}. 重试或重启 app。", e),
                Some(json!({ "refreshed": false, "error": e })),
            )),
        }
    }
}
