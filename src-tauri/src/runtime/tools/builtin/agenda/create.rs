use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::agenda::{
    compute_next_fire_at, AgendaItem, AgendaItemId, ItemStatus, Participant, RecurrenceRule,
};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct CreateAgendaItemRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for CreateAgendaItemRuntimeTool {
    fn id(&self) -> &str { "create_agenda_item" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(
            "create_agenda_item",
            "【自用】为你（当前数字员工）自己创建一条到点自动触发的日程：一次性或循环（每天/每周/每月/每年），到点会以你（同一个 persona）的身份自动执行内置 prompt。",
        )
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let title = required_str(&input, "title")?.to_string();
        let prompt = required_str(&input, "prompt")?.to_string();
        let start_at: DateTime<Utc> = required_str(&input, "start_at")?
            .parse()
            .map_err(|e: chrono::ParseError| {
                ToolError::ExecutionFailed(format!("start_at parse: {e}"))
            })?;
        let timezone = input
            .get("timezone")
            .and_then(Value::as_str)
            .unwrap_or("Asia/Shanghai")
            .to_string();
        let rule: Option<RecurrenceRule> = match input.get("rule") {
            Some(Value::Null) | None => None,
            Some(v) => Some(
                serde_json::from_value(v.clone())
                    .map_err(|e| ToolError::ExecutionFailed(format!("rule: {e}")))?,
            ),
        };
        let workspace_path = input
            .get("workspace_path")
            .and_then(Value::as_str)
            .map(str::to_string);

        let now = Utc::now();
        let mut item = AgendaItem {
            id: AgendaItemId::new(),
            title,
            prompt,
            organizer_employee_id: self.deps.current_persona_id.clone(),
            participants: vec![Participant {
                employee_id: self.deps.current_persona_id.clone(),
                joined_at: now,
            }],
            start_at,
            timezone,
            rule,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            workspace_path,
            created_at: now,
            updated_at: now,
        };
        item.next_fire_at = compute_next_fire_at(&item, now);

        let saved = self
            .deps
            .store
            .create(item)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let json = serde_json::to_value(&saved).unwrap();
        Ok(ToolResult::new(
            "create_agenda_item",
            serde_json::to_string_pretty(&json).unwrap(),
            Some(json),
        ))
    }
}

fn required_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "create_agenda_item".into(),
            message: format!("missing field '{}'", key),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn make_tool(dir: &std::path::Path, employee_id: &str) -> CreateAgendaItemRuntimeTool {
        CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), employee_id.into())),
        }
    }

    #[tokio::test]
    async fn create_returns_item_with_organizer_forced_to_current_employee_id() {
        let dir = TempDir::new().unwrap();
        let tool = make_tool(dir.path(), "alice");
        let input = json!({
            "title": "T",
            "prompt": "P",
            "start_at": "2026-05-07T01:00:00Z",
        });
        let ctx = ToolExecutionContext::for_test("s", "r", "c");
        let result = tool.execute(input, ctx).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["organizerEmployeeId"], "alice");
    }

    #[tokio::test]
    async fn create_rejects_when_title_missing() {
        let dir = TempDir::new().unwrap();
        let tool = make_tool(dir.path(), "alice");
        let input = json!({ "prompt": "P", "start_at": "2026-05-07T01:00:00Z" });
        let ctx = ToolExecutionContext::for_test("s", "r", "c");
        let err = tool.execute(input, ctx).await.unwrap_err();
        assert!(matches!(
            err,
            ToolError::ExecutionFailed(_) | ToolError::InputValidationError { .. }
        ));
    }
}
