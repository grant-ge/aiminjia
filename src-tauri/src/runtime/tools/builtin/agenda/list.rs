use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::agenda::ItemStatus;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct ListAgendaItemsRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for ListAgendaItemsRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("list_agenda_items", "列出当前数字员工的日程")
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let items = self
            .deps
            .store
            .list()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let status_filter: Option<Vec<ItemStatus>> = input
            .get("status_in")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50) as usize;
        let mut filtered: Vec<_> = items
            .into_iter()
            .filter(|i| i.organizer_persona_id == self.deps.current_persona_id)
            .filter(|i| match &status_filter {
                Some(allowed) => allowed.contains(&i.status),
                None => true,
            })
            .collect();
        filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        filtered.truncate(limit);
        let json = serde_json::to_value(&filtered).unwrap();
        Ok(ToolResult::new(
            "list_agenda_items",
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

    #[tokio::test]
    async fn list_returns_only_current_persona_items() {
        let dir = TempDir::new().unwrap();
        for persona in ["alice", "bob"] {
            let tool = super::super::create::CreateAgendaItemRuntimeTool {
                deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), persona.into())),
            };
            tool.execute(
                json!({ "title": "T", "prompt": "P", "start_at": "2999-05-07T01:00:00Z" }),
                ToolExecutionContext::for_test("s", "r", "c"),
            )
            .await
            .unwrap();
        }

        let tool = ListAgendaItemsRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(
                dir.path().to_path_buf(),
                "alice".into(),
            )),
        };
        let result = tool
            .execute(json!({}), ToolExecutionContext::for_test("s", "r", "c"))
            .await
            .unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["organizerPersonaId"], "alice");
    }
}
