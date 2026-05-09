use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::deps::AgendaToolDeps;
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

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::ExecutionFailed("not yet implemented".into()))
    }
}
