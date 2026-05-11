//! P1.4 integration tests: required-tool whitelist validation for Teammate dispatch.
//!
//! An Employee that's spawned as a Teammate (team_name non-empty) MUST whitelist
//! SendMessage, TaskList and TaskGet.  Validation is name-based and fires before
//! the P1.6 idle-loop stub, so we can assert deterministically.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tempfile::TempDir;

use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::agent::{AgentNameRegistry, Member, MemberRole, TeamRegistry};
use app_lib::runtime::employee::store::{
    CreateEmployeeRequest, EmployeeLifecycle, EmployeeStore,
};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

struct NopLauncher;

#[async_trait]
impl SpawnSubagentLauncher for NopLauncher {
    async fn launch_sync(
        &self,
        _request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<String> {
        Ok("nop-output".into())
    }

    async fn launch_async(
        &self,
        _request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        Ok(SpawnAsyncOutcome {
            agent_id: AgentId::new("nop-async-id"),
            name: None,
        })
    }
}

fn build_tool(employee_store: Arc<EmployeeStore>) -> SpawnSubagentRuntimeTool {
    let registry = Arc::new(AgentRegistry::with_builtins());
    SpawnSubagentRuntimeTool::new_with_employees(Arc::new(NopLauncher), registry, employee_store)
}

fn build_ctx(
    session_id: &str,
    team_registry: Arc<TeamRegistry>,
    name_registry: Arc<AgentNameRegistry>,
) -> ToolExecutionContext {
    ToolExecutionContext::for_test(session_id, "run-1", "tc-1")
        .with_team_registry(team_registry)
        .with_agent_names(name_registry)
}

fn create_employee_with_tools(store: &EmployeeStore, tools: Vec<String>) -> String {
    store
        .create(CreateEmployeeRequest {
            name: "tester".to_string(),
            role: "researcher".to_string(),
            description: "research tasks".to_string(),
            avatar: "🔍".to_string(),
            template_id: None,
            tool_whitelist: Some(tools),
            cron: None,
            timezone: None,
            lifecycle: Some(EmployeeLifecycle::Active),
            cron_enabled: Some(true),
            resource_config: None,
            system_prompt_extra: Some("test".to_string()),
            default_skill_id: None,
        })
        .unwrap()
        .id
}

async fn seed_team(team_registry: &TeamRegistry, session_id: &str) {
    let lead = Member {
        agent_id: AgentId::new("lead-id"),
        name: "lead".to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    team_registry
        .create(SessionId::new(session_id), lead, "research-team".to_string())
        .await
        .unwrap();
}

#[tokio::test]
async fn rejects_when_send_message_is_missing() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let emp_id = create_employee_with_tools(
        &store,
        vec!["TaskList".to_string(), "TaskGet".to_string()],
    );
    let tool = build_tool(store);

    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    seed_team(&team_registry, "conv-missing-send").await;
    let ctx = build_ctx("conv-missing-send", team_registry, name_registry);

    let err = tool
        .execute(
            json!({
                "employee_id": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "go",
                "description": "missing send"
            }),
            ctx,
        )
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(msg.contains("missing required tools"), "msg: {msg}");
            assert!(msg.contains("SendMessage"), "msg: {msg}");
            assert!(!msg.contains("TaskList"), "msg should NOT mention TaskList: {msg}");
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn rejects_when_task_list_and_task_get_are_missing() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let emp_id = create_employee_with_tools(&store, vec!["SendMessage".to_string()]);
    let tool = build_tool(store);

    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    seed_team(&team_registry, "conv-missing-task").await;
    let ctx = build_ctx("conv-missing-task", team_registry, name_registry);

    let err = tool
        .execute(
            json!({
                "employee_id": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "go",
                "description": "missing tasks"
            }),
            ctx,
        )
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(msg.contains("missing required tools"), "msg: {msg}");
            assert!(msg.contains("TaskList"), "msg: {msg}");
            assert!(msg.contains("TaskGet"), "msg: {msg}");
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn accepts_when_all_required_tools_present() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let emp_id = create_employee_with_tools(
        &store,
        vec![
            "SendMessage".to_string(),
            "TaskList".to_string(),
            "TaskGet".to_string(),
        ],
    );
    let tool = build_tool(store);

    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    seed_team(&team_registry, "conv-all-tools").await;
    let ctx = build_ctx("conv-all-tools", team_registry, name_registry);

    // Whitelist passes — spawn now actually succeeds (P2.x landed Teammate
    // path).  Verify the call returns Ok (or, if it fails for unrelated
    // reasons, the error must NOT be a missing-tools rejection).
    let result = tool
        .execute(
            json!({
                "employee_id": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "go",
                "description": "all required tools present"
            }),
            ctx,
        )
        .await;

    match result {
        Ok(_) => {}
        Err(ToolError::ExecutionFailed(msg)) => {
            assert!(
                !msg.contains("missing required tools"),
                "should NOT be a required-tools rejection, got: {msg}"
            );
        }
        Err(other) => panic!("unexpected error variant: {other:?}"),
    }
}

#[tokio::test]
async fn legacy_subagent_type_not_subject_to_required_tools() {
    // subagent_type path uses AgentRegistry, not Employee, and is not a Teammate
    // (team_name is empty) → required-tool check must be skipped.
    let dir = TempDir::new().unwrap();
    let store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let tool = build_tool(store);

    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let ctx = build_ctx("conv-legacy", team_registry, name_registry);

    // explore is a built-in agent type with no required-tools metadata; dispatch
    // should reach NopLauncher and succeed.
    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "look around",
                "description": "legacy path"
            }),
            ctx,
        )
        .await;

    assert!(result.is_ok(), "legacy subagent_type should bypass required-tools check; got {result:?}");
}
