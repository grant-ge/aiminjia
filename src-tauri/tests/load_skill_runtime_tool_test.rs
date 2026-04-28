use std::sync::Arc;

use app_lib::plugin::skill_trait::{Skill, SkillState, ToolFilter};
use app_lib::plugin::SkillRegistry;
use app_lib::runtime::tools::builtin::load_skill::LoadSkillRuntimeTool;
use app_lib::runtime::tools::definition::ToolKind;
use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext};
use serde_json::json;

struct BodySkill {
    id: String,
    name: String,
    description: String,
    body: String,
}

impl BodySkill {
    fn new(id: &str, name: &str, description: &str, body: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            body: body.to_string(),
        }
    }
}

impl Skill for BodySkill {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn short_description(&self) -> &str {
        &self.description
    }

    fn system_prompt(&self, _state: &SkillState) -> String {
        String::new()
    }

    fn body_prompt(&self) -> String {
        self.body.clone()
    }

    fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
        ToolFilter::All
    }
}

#[tokio::test]
async fn load_skill_definition_lists_non_default_skill_ids() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    registry
        .register(
            Arc::new(BodySkill::new(
                "daily-assistant",
                "Daily",
                "default",
                "default body",
            )),
            "test",
        )
        .await;
    registry
        .register(
            Arc::new(BodySkill::new(
                "biz-writing",
                "商务写作",
                "邮件/报告",
                "Biz writing body",
            )),
            "test",
        )
        .await;

    let tool = LoadSkillRuntimeTool::new(registry).await;
    let def = tool.definition();

    assert_eq!(def.id, "load_skill");
    assert_eq!(def.kind, ToolKind::Support);
    assert!(def.description.contains("biz-writing"));
    assert!(!def.description.contains("daily-assistant"));
}

#[tokio::test]
async fn load_skill_execute_returns_skill_body_without_runtime_patch() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    registry
        .register(
            Arc::new(BodySkill::new(
                "biz-writing",
                "商务写作",
                "邮件/报告",
                "Follow the biz writing checklist.",
            )),
            "test",
        )
        .await;
    let tool = LoadSkillRuntimeTool::new(registry).await;

    let result = tool
        .execute(
            json!({"skill_id": "biz-writing"}),
            ToolExecutionContext::for_test("conv-load-skill", "run", "tc"),
        )
        .await
        .expect("load_skill should return a tool result");

    assert_eq!(result.tool_name, "load_skill");
    assert!(result.content.contains("Follow the biz writing checklist."));
    assert!(!result.content.contains("Switched to skill"));
    assert!(result.data.is_none(), "load_skill must not emit runtime-control data");
}

#[tokio::test]
async fn load_skill_execute_rejects_unknown_skill() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    registry
        .register(
            Arc::new(BodySkill::new(
                "biz-writing",
                "商务写作",
                "邮件/报告",
                "Follow the biz writing checklist.",
            )),
            "test",
        )
        .await;
    let tool = LoadSkillRuntimeTool::new(registry).await;

    let err = tool
        .execute(
            json!({"skill_id": "missing-skill"}),
            ToolExecutionContext::for_test("conv-load-skill", "run", "tc"),
        )
        .await
        .expect_err("unknown skill should be rejected");

    match err {
        ToolError::ExecutionFailed(message) => {
            assert!(message.contains("missing-skill"));
            assert!(message.contains("biz-writing"));
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}
