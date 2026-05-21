//! Memory RuntimeTools — write_memory and search_memory.
//!
//! These tools do NOT use PluginContext. Runtime-level memory dependencies are
//! injected at construction via `MemoryDeps`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::project_memory::{
    ProjectMemoryEntryDraft, ProjectMemoryService, ProjectMemoryType,
};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

/// Narrow memory dependencies injected at construction time.
pub struct MemoryDeps {
    pub app_data_dir: PathBuf,
    pub workspace_path: PathBuf,
}

pub struct WriteMemoryRuntimeTool {
    deps: Arc<MemoryDeps>,
}

impl WriteMemoryRuntimeTool {
    pub fn new(deps: MemoryDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for WriteMemoryRuntimeTool {
    fn id(&self) -> &str {
        "WriteMemory"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("WriteMemory")
            .unwrap_or_else(|| ToolDefinition::new("WriteMemory", "保存项目记忆"))
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let name = required_str(&input, "name")?.to_string();
        let memory_type = parse_memory_type(required_str(&input, "memory_type")?)?;
        let description = required_str(&input, "description")?.to_string();
        let content = required_str(&input, "content")?.to_string();

        let service = ProjectMemoryService::new(&self.deps.app_data_dir, &self.deps.workspace_path);
        let saved = service
            .save_memory(ProjectMemoryEntryDraft {
                memory_type,
                name: name.clone(),
                description,
                content,
                source: None,
            })
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        let result = json!({
            "status": "saved",
            "name": name,
            "path": saved.relative_path.display().to_string(),
        });
        Ok(ToolResult::new(
            "WriteMemory",
            serde_json::to_string_pretty(&result).unwrap_or_default(),
            Some(result),
        ))
    }
}

pub struct SearchMemoryRuntimeTool {
    deps: Arc<MemoryDeps>,
}

impl SearchMemoryRuntimeTool {
    pub fn new(deps: MemoryDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for SearchMemoryRuntimeTool {
    fn id(&self) -> &str {
        "SearchMemory"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("SearchMemory")
            .unwrap_or_else(|| ToolDefinition::new("SearchMemory", "搜索项目记忆"))
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        let service = ProjectMemoryService::new(&self.deps.app_data_dir, &self.deps.workspace_path);
        let ctx = service
            .load_context(query)
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        let results = ctx
            .recalled_entries
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "type": entry.memory_type.as_str(),
                    "description": entry.description,
                    "content": entry.content,
                })
            })
            .collect::<Vec<_>>();

        let result = json!({
            "status": "ok",
            "count": results.len(),
            "results": results,
        });
        Ok(ToolResult::new(
            "SearchMemory",
            serde_json::to_string_pretty(&result).unwrap_or_default(),
            Some(result),
        ))
    }
}

fn required_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::ExecutionFailed(format!("missing '{}'", key)))
}

fn parse_memory_type(raw: &str) -> Result<ProjectMemoryType, ToolError> {
    match raw {
        "user_preference" => Ok(ProjectMemoryType::UserPreference),
        "project_constraint" => Ok(ProjectMemoryType::ProjectConstraint),
        "reference_info" => Ok(ProjectMemoryType::ReferenceInfo),
        "feedback" => Ok(ProjectMemoryType::Feedback),
        other => Err(ToolError::ExecutionFailed(format!(
            "unknown memory_type '{}'. Valid: user_preference, project_constraint, reference_info, feedback",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_deps(dir: &std::path::Path) -> MemoryDeps {
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        MemoryDeps {
            app_data_dir: dir.to_path_buf(),
            workspace_path: workspace,
        }
    }

    fn make_ctx() -> ToolExecutionContext {
        ToolExecutionContext::for_test("test-session", "test-run", "tool-call")
    }

    #[tokio::test]
    async fn test_write_memory_saves_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = WriteMemoryRuntimeTool::new(make_deps(dir.path()));

        let result = tool
            .execute(
                json!({
                    "name": "user-prefers-boxplot",
                    "memory_type": "user_preference",
                    "description": "用户偏好用箱型图展示薪资分布",
                    "content": "用户明确表示喜欢用箱型图（boxplot）展示薪资分布。"
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "saved");
        assert_eq!(parsed["name"], "user-prefers-boxplot");
        assert!(parsed["path"].as_str().unwrap().ends_with(".md"));
    }

    #[tokio::test]
    async fn test_write_memory_invalid_type() {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = WriteMemoryRuntimeTool::new(make_deps(dir.path()));

        let result = tool
            .execute(
                json!({
                    "name": "bad-type",
                    "memory_type": "unknown",
                    "description": "desc",
                    "content": "content"
                }),
                make_ctx(),
            )
            .await;

        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown memory_type"));
    }

    #[tokio::test]
    async fn test_search_memory_returns_results() {
        let dir = tempfile::TempDir::new().unwrap();
        let write_tool = WriteMemoryRuntimeTool::new(make_deps(dir.path()));
        write_tool
            .execute(
                json!({
                    "name": "user-prefers-boxplot",
                    "memory_type": "user_preference",
                    "description": "用户偏好用箱型图展示薪资分布",
                    "content": "用户明确表示喜欢用箱型图（boxplot）展示薪资分布。"
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        let search_tool = SearchMemoryRuntimeTool::new(make_deps(dir.path()));
        let result = search_tool
            .execute(json!({ "query": "boxplot 箱型图" }), make_ctx())
            .await
            .unwrap();

        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["count"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_search_memory_empty_results() {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = SearchMemoryRuntimeTool::new(make_deps(dir.path()));

        let result = tool
            .execute(json!({ "query": "完全不相关的查询词语" }), make_ctx())
            .await
            .unwrap();

        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["count"], 0);
    }
}
