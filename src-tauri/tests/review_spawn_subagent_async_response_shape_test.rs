//! review: async JSON response from spawn_subagent has both agent_id and task_id fields.
//!
//! Scope: verifies that when SpawnSubagentRuntimeTool executes with
//! run_in_background=true the returned JSON string contains both
//! "agent_id" and "task_id" (identical values), as required by the
//! TaskStop/TaskGet lookup contract (they use task_id for lookup).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

// ─── Stub launcher ────────────────────────────────────────────────────────────

struct FixedIdLauncher {
    agent_id: AgentId,
}

#[async_trait]
impl SpawnSubagentLauncher for FixedIdLauncher {
    async fn launch_sync(
        &self,
        _req: SpawnSubagentRequest,
        _ctx: SpawnSubagentContext,
    ) -> Result<String> {
        unimplemented!("sync path not tested here")
    }

    async fn launch_async(
        &self,
        req: SpawnSubagentRequest,
        _ctx: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        Ok(SpawnAsyncOutcome {
            agent_id: self.agent_id.clone(),
            name: req.name.clone(),
        })
    }
}

fn build_tool(agent_id: &str) -> SpawnSubagentRuntimeTool {
    SpawnSubagentRuntimeTool::new(
        Arc::new(FixedIdLauncher {
            agent_id: AgentId::new(agent_id),
        }),
        Arc::new(AgentRegistry::with_builtins()),
    )
}

// ─── Test 1: agent_id is present ─────────────────────────────────────────────

#[tokio::test]
async fn async_response_has_agent_id_field() {
    let tool = build_tool("stable-test-agent-id-001");
    let ctx = ToolExecutionContext::for_test("sess-spawn", "run-spawn", "tc-spawn");
    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "do something",
                "description": "test async",
                "run_in_background": true,
            }),
            ctx,
        )
        .await
        .expect("async spawn must succeed");

    let parsed: Value = serde_json::from_str(&result.content)
        .expect("response content must be valid JSON");

    let agent_id = parsed
        .get("agent_id")
        .and_then(|v| v.as_str())
        .expect("response must have a non-null agent_id string");
    assert_eq!(
        agent_id, "stable-test-agent-id-001",
        "agent_id must equal the id returned by launch_async"
    );
}

// ─── Test 2: task_id is present and equals agent_id ──────────────────────────

#[tokio::test]
async fn async_response_has_task_id_equal_to_agent_id() {
    let tool = build_tool("stable-test-agent-id-002");
    let ctx = ToolExecutionContext::for_test("sess-spawn-2", "run-spawn-2", "tc-spawn-2");
    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "background work",
                "description": "test task_id presence",
                "run_in_background": true,
                "name": "worker1"
            }),
            ctx,
        )
        .await
        .expect("async spawn must succeed");

    let parsed: Value = serde_json::from_str(&result.content)
        .expect("response content must be valid JSON");

    // Both fields must be present
    let agent_id = parsed
        .get("agent_id")
        .and_then(|v| v.as_str())
        .expect("agent_id must be present in async response");
    let task_id = parsed
        .get("task_id")
        .and_then(|v| v.as_str())
        .expect("task_id must be present in async response — required for TaskStop/TaskGet");

    assert_eq!(
        agent_id, task_id,
        "task_id must equal agent_id so that TaskStop can look up by either field"
    );
    assert_eq!(task_id, "stable-test-agent-id-002");
}
