use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::agenda::AgendaItemId;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct SkipOccurrenceRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for SkipOccurrenceRuntimeTool {
    fn id(&self) -> &str { "skip_occurrence" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(
            "skip_occurrence",
            "【自用】跳过你自己循环日程的某一次触发。",
        )
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let id_str = input
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InputValidationError {
                tool_name: "skip_occurrence".into(),
                message: "missing 'id'".into(),
            })?;
        let at_str = input
            .get("at")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InputValidationError {
                tool_name: "skip_occurrence".into(),
                message: "missing 'at'".into(),
            })?;
        let at: DateTime<Utc> = at_str
            .parse()
            .map_err(|e: chrono::ParseError| {
                ToolError::ExecutionFailed(format!("at parse: {e}"))
            })?;
        let id = AgendaItemId(id_str.to_string());

        let item = self
            .deps
            .store
            .get(&id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if item.organizer_employee_id != self.deps.current_persona_id {
            return Err(ToolError::PermissionDenied(
                "can only skip own agenda items".into(),
            ));
        }

        let updated = self
            .deps
            .store
            .set_skip(&id, at)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let json = serde_json::to_value(&updated).unwrap();
        Ok(ToolResult::new(
            "skip_occurrence",
            serde_json::to_string_pretty(&json).unwrap(),
            Some(json),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    async fn make_recurring_item(dir: &std::path::Path, employee_id: &str) -> String {
        let tool = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), employee_id.into())),
        };
        let result = tool
            .execute(
                json!({
                    "title": "T", "prompt": "P", "start_at": "2999-05-07T01:00:00Z",
                    "rule": {
                        "freq": "daily",
                        "interval": 1,
                        "endCondition": { "kind": "never" },
                        "byDay": [],
                        "byMonthDay": []
                    },
                }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        parsed["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn skip_adds_to_skip_dates() {
        let dir = TempDir::new().unwrap();
        let id = make_recurring_item(dir.path(), "alice").await;
        let tool = SkipOccurrenceRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(
                dir.path().to_path_buf(),
                "alice".into(),
            )),
        };
        let result = tool
            .execute(
                json!({ "id": id, "at": "2999-05-08T01:00:00Z" }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let skip_dates = parsed["skipDates"].as_array().unwrap();
        assert!(skip_dates.iter().any(|s| s == "2999-05-08T01:00:00Z"));
    }
}
