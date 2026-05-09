use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::agenda::AgendaItemId;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct ListAgendaOccurrencesRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for ListAgendaOccurrencesRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("list_agenda_occurrences", "查看自己日程的执行历史")
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let id_str = input
            .get("agenda_item_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InputValidationError {
                tool_name: "list_agenda_occurrences".into(),
                message: "missing 'agenda_item_id'".into(),
            })?;
        let id = AgendaItemId(id_str.to_string());

        let item = self
            .deps
            .store
            .get(&id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if item.organizer_persona_id != self.deps.current_persona_id {
            return Err(ToolError::PermissionDenied(
                "can only list own agenda occurrences".into(),
            ));
        }

        let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
        let occs = self
            .deps
            .store
            .list_occurrences(&id, limit)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let json = serde_json::to_value(&occs).unwrap();
        Ok(ToolResult::new(
            "list_agenda_occurrences",
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

    async fn make_owned_item(dir: &std::path::Path, persona: &str) -> String {
        let create = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), persona.into())),
        };
        let result = create
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
    async fn list_occurrences_for_owned_item() {
        let dir = TempDir::new().unwrap();
        let id = make_owned_item(dir.path(), "alice").await;
        let tool = ListAgendaOccurrencesRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(
                dir.path().to_path_buf(),
                "alice".into(),
            )),
        };
        let result = tool
            .execute(
                json!({ "agenda_item_id": id }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();
        assert_eq!(arr.len(), 0);
    }

    #[tokio::test]
    async fn list_occurrences_others_item_denied() {
        let dir = TempDir::new().unwrap();
        let id = make_owned_item(dir.path(), "alice").await;
        let tool = ListAgendaOccurrencesRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "bob".into())),
        };
        let err = tool
            .execute(
                json!({ "agenda_item_id": id }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
    }
}
