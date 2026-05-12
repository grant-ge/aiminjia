//! task_output RuntimeTool — read async sub-agent transcript incrementally.
//!
//! Parent calls `task_output(task_id="<agentId>", offset=N)` to receive any new
//! transcript lines past `offset`. Used together with the `<task-notification>`
//! XML emitted by P7.2 (which carries `<output-file>...</output-file>`).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::agent::output_writer;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::user_scoped_paths::UserScopedPathResolver;

pub struct TaskOutputRuntimeTool {
    paths: Arc<dyn UserScopedPathResolver>,
}

impl TaskOutputRuntimeTool {
    pub fn new(paths: Arc<dyn UserScopedPathResolver>) -> Self {
        Self { paths }
    }
}

fn validate_task_id(s: &str) -> Result<&str, ToolError> {
    if s.contains('/')
        || s.contains('\\')
        || s.contains('\0')
        || s == "."
        || s == ".."
        || s.starts_with('.')
    {
        return Err(ToolError::ExecutionFailed(format!(
            "invalid task_id: {s:?} (must not contain path separators or be a dotfile)"
        )));
    }
    Ok(s)
}

#[async_trait]
impl RuntimeTool for TaskOutputRuntimeTool {
    fn id(&self) -> &str { "TaskOutput" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG.get("TaskOutput").unwrap_or_else(|| {
            ToolDefinition::new("TaskOutput", "Read async sub-agent transcript")
                .with_kind(ToolKind::Support)
                .with_read_only(true)
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let task_id_raw = input
            .get("task_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("missing required field: task_id".into()))?;
        let task_id = validate_task_id(task_id_raw)?;

        let offset = input.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;

        let paths = self
            .paths
            .require_paths()
            .map_err(|e| ToolError::ExecutionFailed(format!("user scope unavailable: {e}")))?;

        let path = output_writer::transcript_path(&paths.subagent_transcripts_dir(), task_id);
        let (lines, new_offset) = output_writer::read_from(&path, offset)
            .map_err(|e| ToolError::ExecutionFailed(format!("read transcript failed: {e}")))?;

        let body = json!({
            "lines": lines,
            "new_offset": new_offset,
        });
        Ok(ToolResult::new("TaskOutput", body.to_string(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::user_scoped_paths::UserScopedPaths;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Test resolver that points at a TempDir-backed UserScopedPaths.
    struct TestResolver {
        paths: UserScopedPaths,
    }
    impl TestResolver {
        fn new(root: PathBuf) -> Self {
            Self {
                paths: UserScopedPaths::new(&root, "t_test__u_test"),
            }
        }
    }
    impl UserScopedPathResolver for TestResolver {
        fn resolve_paths(&self) -> Option<UserScopedPaths> {
            Some(self.paths.clone())
        }
    }

    /// Resolver that always returns None (simulating "not logged in").
    struct UnauthResolver;
    impl UserScopedPathResolver for UnauthResolver {
        fn resolve_paths(&self) -> Option<UserScopedPaths> {
            None
        }
    }

    fn build_tool(tmp: &TempDir) -> TaskOutputRuntimeTool {
        TaskOutputRuntimeTool::new(Arc::new(TestResolver::new(tmp.path().to_path_buf())))
    }

    #[test]
    fn definition_id_is_task_output() {
        let tmp = TempDir::new().unwrap();
        let tool = build_tool(&tmp);
        assert_eq!(tool.id(), "TaskOutput");
    }

    #[test]
    fn is_concurrency_safe_returns_true() {
        let tmp = TempDir::new().unwrap();
        let tool = build_tool(&tmp);
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[tokio::test]
    async fn missing_task_id_returns_error() {
        let tmp = TempDir::new().unwrap();
        let tool = build_tool(&tmp);
        let ctx = ToolExecutionContext::for_test("c", "r", "tc");
        let err = tool.execute(json!({}), ctx).await.unwrap_err();
        match err {
            ToolError::ExecutionFailed(m) => assert!(m.contains("task_id")),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_task_id_returns_error() {
        let tmp = TempDir::new().unwrap();
        let tool = build_tool(&tmp);
        let ctx = ToolExecutionContext::for_test("c", "r", "tc");
        let err = tool
            .execute(json!({"task_id": "   "}), ctx)
            .await
            .unwrap_err();
        matches!(err, ToolError::ExecutionFailed(_));
    }

    #[tokio::test]
    async fn unauth_resolver_returns_error() {
        let tool = TaskOutputRuntimeTool::new(Arc::new(UnauthResolver));
        let ctx = ToolExecutionContext::for_test("c", "r", "tc");
        let err = tool
            .execute(json!({"task_id": "agent-1"}), ctx)
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(m) => assert!(m.contains("user scope")),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nonexistent_transcript_returns_empty_lines_and_zero_offset() {
        let tmp = TempDir::new().unwrap();
        let tool = build_tool(&tmp);
        let ctx = ToolExecutionContext::for_test("c", "r", "tc");
        let result = tool
            .execute(json!({"task_id": "never-existed"}), ctx)
            .await
            .unwrap();
        let body: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(body["lines"].as_array().unwrap().len(), 0);
        assert_eq!(body["new_offset"].as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn reads_existing_transcript_lines_from_offset() {
        let tmp = TempDir::new().unwrap();
        let tool = build_tool(&tmp);
        let resolver_paths = UserScopedPaths::new(tmp.path(), "t_test__u_test");
        let path =
            output_writer::transcript_path(&resolver_paths.subagent_transcripts_dir(), "agent-x");
        for i in 0..3 {
            output_writer::append_line(
                &path,
                &output_writer::TranscriptLine::assistant(format!("msg-{i}")),
            )
            .unwrap();
        }

        // offset=0 -> 3 lines, new_offset=3
        let ctx = ToolExecutionContext::for_test("c", "r", "tc");
        let r1 = tool
            .execute(json!({"task_id": "agent-x", "offset": 0}), ctx)
            .await
            .unwrap();
        let body1: Value = serde_json::from_str(&r1.content).unwrap();
        assert_eq!(body1["lines"].as_array().unwrap().len(), 3);
        assert_eq!(body1["new_offset"].as_u64().unwrap(), 3);

        // offset=3 -> empty, new_offset=3
        let ctx2 = ToolExecutionContext::for_test("c", "r", "tc2");
        let r2 = tool
            .execute(json!({"task_id": "agent-x", "offset": 3}), ctx2)
            .await
            .unwrap();
        let body2: Value = serde_json::from_str(&r2.content).unwrap();
        assert_eq!(body2["lines"].as_array().unwrap().len(), 0);
        assert_eq!(body2["new_offset"].as_u64().unwrap(), 3);
    }
}
