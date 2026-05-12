use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::agenda::{compute_next_fire_at, AgendaItemId, ItemStatus, RecurrenceRule};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct UpdateAgendaItemRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for UpdateAgendaItemRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "update_agenda_item",
            "【自用】修改你自己创建的日程（标题/触发内容/频率/启用状态）。",
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
                tool_name: "update_agenda_item".into(),
                message: "missing 'id'".into(),
            })?;
        let id = AgendaItemId(id_str.to_string());
        let mut item = self
            .deps
            .store
            .get(&id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if item.organizer_employee_id != self.deps.current_persona_id {
            return Err(ToolError::PermissionDenied(
                "can only update own agenda items".into(),
            ));
        }
        if let Some(t) = input.get("title").and_then(Value::as_str) {
            item.title = t.to_string();
        }
        if let Some(p) = input.get("prompt").and_then(Value::as_str) {
            item.prompt = p.to_string();
        }
        if let Some(rule_v) = input.get("rule") {
            item.rule = if rule_v.is_null() {
                None
            } else {
                Some(
                    serde_json::from_value::<RecurrenceRule>(rule_v.clone())
                        .map_err(|e| ToolError::ExecutionFailed(format!("rule: {e}")))?,
                )
            };
        }
        if let Some(st) = input.get("status").and_then(Value::as_str) {
            item.status = match st {
                "active" => ItemStatus::Active,
                "paused" => ItemStatus::Paused,
                other => {
                    return Err(ToolError::InputValidationError {
                        tool_name: "update_agenda_item".into(),
                        message: format!(
                            "status only supports active|paused, got '{other}'"
                        ),
                    });
                }
            };
        }
        let now = chrono::Utc::now();
        item.updated_at = now;
        item.next_fire_at = compute_next_fire_at(&item, now);
        let saved = self
            .deps
            .store
            .update(item)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let json = serde_json::to_value(&saved).unwrap();
        Ok(ToolResult::new(
            "update_agenda_item",
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

    async fn create_item(dir: &std::path::Path, employee_id: &str) -> String {
        let tool = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), employee_id.into())),
        };
        let result = tool
            .execute(
                json!({ "title": "T", "prompt": "P", "start_at": "2999-05-07T01:00:00Z" }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        parsed["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn update_succeeds_for_owned_item() {
        let dir = TempDir::new().unwrap();
        let id = create_item(dir.path(), "alice").await;
        let tool = UpdateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(
                dir.path().to_path_buf(),
                "alice".into(),
            )),
        };
        let result = tool
            .execute(
                json!({ "id": id, "title": "T2" }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["title"], "T2");
    }

    #[tokio::test]
    async fn update_rejects_other_employees_item() {
        let dir = TempDir::new().unwrap();
        let id = create_item(dir.path(), "alice").await;
        let tool = UpdateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "bob".into())),
        };
        let err = tool
            .execute(
                json!({ "id": id, "title": "T2" }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ToolError::PermissionDenied(_) | ToolError::ExecutionFailed(_)
        ));
    }
}
