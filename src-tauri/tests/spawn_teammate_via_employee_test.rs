//! Integration tests for spawn_subagent Teammate dispatch.
//!
//! Tests the source-resolution and validation logic:
//! - `team_name` requires an existing Team (TeamCreate must be called first)
//! - duplicate `name` in same session is suppressed as an idempotent success
//! - happy-path: Employee-sourced Teammate (subagent_type=emp-…) registers name + joins Team
//!
//! After the协议合并 (2026-05-12)：spawn_subagent 只有单一 `subagent_type`
//! 字段；emp-id 通过 `register_dynamic` 投影进 AgentRegistry，跟 builtin
//! 共用同一查询入口。互斥参数 `employee_id` 已删除。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tempfile::TempDir;

use app_lib::runtime::agent::employee_projection::project_employee_to_agent;
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::agent::{AgentNameRegistry, Member, MemberRole, TeamRegistry};
use app_lib::runtime::employee::store::{CreateEmployeeRequest, EmployeeLifecycle, EmployeeStore};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

// ─── Stub launcher ────────────────────────────────────────────────────────────

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

// ─── Helper: build tool with employee store seeded into registry ─────────────

fn build_tool_with_employee_store(employee_store: Arc<EmployeeStore>) -> SpawnSubagentRuntimeTool {
    let registry = Arc::new(AgentRegistry::with_builtins());
    // Seed all Active employees as dynamic AgentDefinitions so spawn_subagent
    // can resolve `subagent_type=emp-…` via the single registry query path.
    if let Ok(records) = employee_store.list() {
        for rec in records {
            if matches!(rec.lifecycle, EmployeeLifecycle::Active) {
                registry.register_dynamic(project_employee_to_agent(&rec));
            }
        }
    }
    SpawnSubagentRuntimeTool::new(Arc::new(NopLauncher), registry)
}

fn build_ctx_with_registries(
    session_id: &str,
    team_registry: Arc<TeamRegistry>,
    name_registry: Arc<AgentNameRegistry>,
) -> ToolExecutionContext {
    ToolExecutionContext::for_test(session_id, "run-1", "tc-1")
        .with_team_registry(team_registry)
        .with_agent_names(name_registry)
}

fn create_employee(store: &EmployeeStore) -> String {
    let record = store
        .create(CreateEmployeeRequest {
            name: "test-researcher".to_string(),
            role: "researcher".to_string(),
            description: "research tasks".to_string(),
            avatar: "🔍".to_string(),
            template_id: None,
            // Must include SendMessage / TaskList / TaskGet to pass the required-tool
            // validation gate in spawn_subagent (added P1.3).
            tool_whitelist: Some(vec![
                "WebSearch".to_string(),
                "SendMessage".to_string(),
                "TaskList".to_string(),
                "TaskGet".to_string(),
            ]),
            cron: None,
            timezone: None,
            lifecycle: Some(EmployeeLifecycle::Active),
            cron_enabled: Some(true),
            resource_config: None,
            system_prompt_extra: Some("You are a research assistant.".to_string()),
            default_skill_id: None,
            skill_ids: None,
        })
        .unwrap();
    record.id
}

// ─── Test 1: team_name with no existing Team → error ─────────────────────────

#[tokio::test]
async fn rejects_team_name_when_no_team_exists() {
    let dir = TempDir::new().unwrap();
    let employee_store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let emp_id = create_employee(&employee_store);
    let tool = build_tool_with_employee_store(employee_store);

    // No TeamCreate → TeamRegistry has no team for this session.
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();
    let ctx = build_ctx_with_registries("conv-no-team", team_registry, name_registry);

    let err = tool
        .execute(
            json!({
                "subagent_type": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "do research",
                "description": "no team test"
            }),
            ctx,
        )
        .await
        .unwrap_err();

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("no team") || msg.contains("TeamCreate"),
                "error should mention no team or TeamCreate, got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed for no team, got: {:?}", other),
    }
}

// ─── Test 2: duplicate name in same session ───────────────────────────────────

#[tokio::test]
async fn suppresses_duplicate_name_in_same_session() {
    let dir = TempDir::new().unwrap();
    let employee_store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let emp_id = create_employee(&employee_store);
    let tool = Arc::new(build_tool_with_employee_store(employee_store));

    let session_id = "conv-dup-name";
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();

    // Seed a Lead member and create the team so team_registry.get() returns Some.
    let lead = Member {
        agent_id: AgentId::new("lead-agent-id"),
        name: "lead".to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    team_registry
        .create(
            SessionId::new(session_id),
            lead,
            "research-team".to_string(),
        )
        .await
        .unwrap();

    // First dispatch with name "researcher" should register the teammate.
    let ctx1 = build_ctx_with_registries(session_id, team_registry.clone(), name_registry.clone());
    let first = tool
        .execute(
            json!({
                "subagent_type": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "do research",
                "description": "first dispatch"
            }),
            ctx1,
        )
        .await
        .expect("first teammate spawn should succeed");
    let first_payload: serde_json::Value =
        serde_json::from_str(&first.content).expect("first teammate response should be JSON");
    let first_agent_id = first_payload["agent_id"]
        .as_str()
        .expect("first teammate response should include agent_id")
        .to_string();

    // Second dispatch with the SAME name in the same session is idempotently
    // suppressed and returns the originally registered teammate identity.
    let ctx2 = build_ctx_with_registries(session_id, team_registry.clone(), name_registry.clone());
    let result = tool
        .execute(
            json!({
                "subagent_type": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "do research again",
                "description": "second dispatch"
            }),
            ctx2,
        )
        .await
        .expect("duplicate teammate spawn should be suppressed, not fail");

    let payload = result
        .data
        .expect("duplicate teammate response should include structured data");
    assert_eq!(payload["status"], "teammate_spawned");
    assert_eq!(payload["name"], "researcher");
    assert_eq!(payload["agent_id"], first_agent_id);
    assert_eq!(payload["duplicate_suppressed"], true);
}

// ─── Test 3: happy path (subagent_type=emp-… + Team + Name) ───────────────────

#[tokio::test]
async fn happy_path_employee_subagent_type_creates_teammate_and_registers_name() {
    let dir = TempDir::new().unwrap();
    let employee_store = Arc::new(EmployeeStore::new(dir.path().to_path_buf()));
    let emp_id = create_employee(&employee_store);
    let tool = build_tool_with_employee_store(employee_store);

    let session_id = "conv-happy-path";
    let team_registry = TeamRegistry::new();
    let name_registry = AgentNameRegistry::new();

    // Create team with a lead so TeamRegistry.get() returns Some.
    let lead = Member {
        agent_id: AgentId::new("lead-happy"),
        name: "lead".to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    team_registry
        .create(
            SessionId::new(session_id),
            lead,
            "research-team".to_string(),
        )
        .await
        .unwrap();

    let ctx = build_ctx_with_registries(session_id, team_registry.clone(), name_registry.clone());

    // This will error with the P1.6 stub, but name registration happens BEFORE
    // the stub is called, so we can assert on AgentNameRegistry state.
    let _ = tool
        .execute(
            json!({
                "subagent_type": emp_id,
                "team_name": "research-team",
                "name": "researcher",
                "prompt": "do research",
                "description": "happy path test"
            }),
            ctx,
        )
        .await; // Expect Err from P1.6 stub — that's OK.

    // Assert: name "researcher" was registered in AgentNameRegistry under
    // the team_name resolved from `team_handle` ("research-team"), not the
    // single-team era "default" placeholder.
    let sid = SessionId::new(session_id);
    let resolved = name_registry
        .resolve(&sid, "research-team", "researcher")
        .await;
    assert!(
        resolved.is_some(),
        "AgentNameRegistry should have 'researcher' registered under 'research-team'"
    );
}
