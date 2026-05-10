use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::agenda::AgendaItemId;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

/// Soft-delete: status → Cancelled, next_fire_at → None.
/// Hard delete (磁盘清除) only via UI command `delete_agenda_item`.
pub struct CancelAgendaItemRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for CancelAgendaItemRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "cancel_agenda_item",
            "【自用】取消你自己创建的日程（软删除，可在 UI 恢复）。",
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
                tool_name: "cancel_agenda_item".into(),
                message: "missing 'id'".into(),
            })?;
        let id = AgendaItemId(id_str.to_string());
        let item = self
            .deps
            .store
            .get(&id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if item.organizer_persona_id != self.deps.current_persona_id {
            return Err(ToolError::PermissionDenied(
                "can only cancel own agenda items".into(),
            ));
        }
        let cancelled = self
            .deps
            .store
            .cancel(&id, chrono::Utc::now())
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let json = serde_json::json!({
            "id": id_str,
            "status": cancelled.status,
        });
        Ok(ToolResult::new(
            "cancel_agenda_item",
            serde_json::to_string_pretty(&json).unwrap(),
            Some(json),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agenda::{AgendaStore, ItemStatus};
    use serde_json::json;
    use tempfile::TempDir;

    async fn make_item(dir: &std::path::Path, persona: &str) -> String {
        let tool = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), persona.into())),
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
    async fn cancel_owned_item_soft_deletes() {
        let dir = TempDir::new().unwrap();
        let id = make_item(dir.path(), "alice").await;
        let tool = CancelAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(
                dir.path().to_path_buf(),
                "alice".into(),
            )),
        };
        let result = tool
            .execute(
                json!({ "id": id }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "cancelled");

        let store = AgendaStore::new(dir.path());
        let fetched = store
            .get(&AgendaItemId(parsed["id"].as_str().unwrap().into()))
            .unwrap();
        assert!(matches!(fetched.status, ItemStatus::Cancelled));
        assert!(fetched.next_fire_at.is_none());
    }

    #[tokio::test]
    async fn cancel_others_item_denied() {
        let dir = TempDir::new().unwrap();
        let id = make_item(dir.path(), "alice").await;
        let tool = CancelAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "bob".into())),
        };
        let err = tool
            .execute(
                json!({ "id": id }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
    }
}
