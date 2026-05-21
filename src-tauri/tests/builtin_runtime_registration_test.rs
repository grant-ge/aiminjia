//! Verify that runtime-native workspace tools are registered at startup
//! and that ToolRegistry::execute() dispatches to them (not legacy ToolPlugin).

#![allow(deprecated)]

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::plugin::skill_trait::{Skill, SkillState, ToolFilter};
use app_lib::runtime::dependencies::StaticRuntimeResolver;
use app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
use app_lib::runtime::tools::ToolExecutionContext;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

// ─── Helper ─────────────────────────────────────────────────────────────────

fn build_test_plugin_ctx(
    workspace_path: std::path::PathBuf,
) -> app_lib::plugin::context::PluginContext {
    let storage = Arc::new(
        app_lib::storage::file_store::AppStorage::new(&workspace_path)
            .expect("AppStorage::new failed"),
    );
    let file_manager = Arc::new(app_lib::storage::file_manager::FileManager::new(
        &workspace_path,
    ));
    #[allow(deprecated)]
    app_lib::plugin::context::PluginContext {
        storage,
        file_manager,
        workspace_path: workspace_path.clone(),
        conversation_id: "test-conv".to_string(),
        session_id: app_lib::runtime::ids::SessionId::new("test-conv"),
        run_id: None,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        auth_manager: None,
        dingtalk_bridge: None,
        use_cloud: false,
        model: String::new(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        skill_registry: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
        runtime_resolver: Some(Arc::new(StaticRuntimeResolver::new(
            PathBuf::from("/tmp/renlijia-managed-python/bin/python3"),
            PathBuf::from("/tmp/renlijia-managed-node/bin/node"),
            PathBuf::from("/tmp/renlijia-managed-node/bin/npm"),
            PathBuf::from("/tmp/renlijia-managed-node/bin/npx"),
            PathBuf::from("/tmp/renlijia-managed-uv/bin/uv"),
            PathBuf::from("/tmp/renlijia-managed-uv/bin/uvx"),
            PathBuf::from("/tmp/renlijia-managed-node/node_modules"),
            PathBuf::from("/tmp/renlijia-managed-python/site-packages"),
        ))),
        permission_ctx: None,
        current_persona_id: None,
    }
}

struct BodySkill {
    id: String,
    body: String,
}

impl BodySkill {
    fn new(id: &str, body: &str) -> Self {
        Self {
            id: id.to_string(),
            body: body.to_string(),
        }
    }
}

impl Skill for BodySkill {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.id
    }

    fn system_prompt(&self, _state: &SkillState) -> String {
        String::new()
    }

    fn body_prompt(&self) -> String {
        self.body.clone()
    }

    fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
        ToolFilter::All
    }
}

// ─── Test 1: register_builtin_tools registers workspace RuntimeTools ─────────

/// register_builtin_tools should call register_runtime for the workspace/file
/// RuntimeTool implementations plus bash/grep_content. After registration the runtime tool
/// schemas should include all of them.
#[tokio::test]
async fn register_builtin_tools_registers_workspace_runtime_tools() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("test.csv"), b"col\n1\n").unwrap();

    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    // read_workspace_file should be routed to the RuntimeTool (not legacy).
    // The tool requires workspace capability; our ctx has workspace_path set,
    // which is enough to satisfy the CapabilityPermissionPipeline check.
    let result = registry
        .execute(
            "Read",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"file_path": "test.csv"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await;
    assert!(
        result.is_ok(),
        "read_workspace_file should succeed via runtime tool: {:?}",
        result
    );
    let output = result.unwrap();
    assert!(
        output.content.contains("col"),
        "Should read test.csv content, got: {}",
        output.content
    );
}

// ─── Test 2: workspace/bash/grep runtime tools are registered ────────────────

#[tokio::test]
async fn all_workspace_runtime_tools_are_registered() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    // Verify all tools appear in get_all_schemas()
    let schemas = registry.get_all_schemas().await;
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();

    for tool_name in &[
        "Read",
        "Glob",
        "Write",
        "Edit",
        "Bash",
        "Grep",
    ] {
        assert!(
            names.contains(tool_name),
            "Expected '{}' in schemas, got: {:?}",
            tool_name,
            names
        );
    }
}

#[tokio::test]
async fn request_scoped_memory_runtime_tools_are_visible_and_executable() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let schemas = registry.get_all_schemas().await;
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    for tool_name in &["WriteMemory", "SearchMemory"] {
        assert!(
            names.contains(tool_name),
            "Expected '{}' in schemas, got: {:?}",
            tool_name,
            names
        );
    }

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    let write_result = registry
        .execute(
            "WriteMemory",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({
                "name": "user-prefers-boxplot",
                "memory_type": "user_preference",
                "description": "用户偏好用箱型图展示薪资分布",
                "content": "用户明确表示喜欢用箱型图（box plot）展示薪资分布，不喜欢柱状图。"
            }),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await
        .expect("write_memory should execute via request-scoped runtime tool");

    assert!(
        write_result.content.contains("\"status\": \"saved\""),
        "write_memory should return saved JSON result: {}",
        write_result.content
    );

    let search_result = registry
        .execute(
            "SearchMemory",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({
                "query": "boxplot 箱型图"
            }),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await
        .expect("search_memory should execute via request-scoped runtime tool");

    assert!(
        search_result.content.contains("user-prefers-boxplot"),
        "search_memory should recall the saved entry: {}",
        search_result.content
    );
}

#[tokio::test]
async fn bash_runtime_tool_executes_via_registry() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    let result = registry
        .execute(
            "Bash",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"command": "echo hi"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await
        .expect("bash should execute via runtime tool");

    assert!(
        result.content.contains("hi"),
        "bash output should contain echo result: {}",
        result.content
    );
}

#[tokio::test]
async fn grep_runtime_tool_executes_via_registry() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("note.txt"), "hello grep\n").unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    let result = registry
        .execute(
            "Grep",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"pattern": "hello", "output_mode": "files_with_matches"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await
        .expect("grep_content should execute via runtime tool");

    assert!(
        result.content.contains("note.txt"),
        "grep_content output should contain matched file: {}",
        result.content
    );
}

// ─── Test 3: execute() routes to RuntimeTool, not legacy ToolPlugin ──────────

/// Confirm that ToolRegistry::execute() dispatches to the RuntimeTool path
/// (not the legacy path) by checking the output format.
/// RuntimeTool produces JSON content; legacy ToolPlugin produces plain text.
#[tokio::test]
async fn execute_dispatches_to_runtime_tool_not_legacy() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("probe.txt"), b"hello").unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    let result = registry
        .execute(
            "Glob",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"pattern": "*.txt"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await;
    assert!(
        result.is_ok(),
        "execute should succeed for search_files: {:?}",
        result
    );
    let output = result.unwrap();
    // RuntimeTool produces JSON-formatted content (via tool_result())
    let parsed: serde_json::Value =
        serde_json::from_str(&output.content).expect("RuntimeTool should return valid JSON");
    assert!(
        parsed.get("matches").is_some(),
        "Expected JSON output from RuntimeTool, got: {}",
        output.content
    );
}

// ─── Test 3b: authorized workspace wins over internal workspace ──────────────

#[tokio::test]
async fn workspace_runtime_tool_uses_authorized_workspace_when_present() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let internal = TempDir::new().unwrap();
    let external = TempDir::new().unwrap();
    std::fs::write(internal.path().join("internal-only.txt"), b"internal").unwrap();
    std::fs::write(external.path().join("external-only.txt"), b"external").unwrap();

    let mut ctx = build_test_plugin_ctx(internal.path().to_path_buf());
    ctx.authorized_workspace = Some(app_lib::runtime::store::AuthorizedWorkspaceRef {
        id: "aw-test".to_string(),
        root_path: external.path().to_path_buf(),
        display_name: "external".to_string(),
    });

    let result = registry
        .execute(
            "Glob",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"pattern": "*"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await
        .expect("search_files should succeed with authorized workspace");

    assert!(
        result.content.contains("external-only.txt"),
        "result should read authorized workspace contents: {}",
        result.content
    );
    assert!(
        !result.content.contains("internal-only.txt"),
        "authorized workspace should take precedence over internal workspace: {}",
        result.content
    );
}

// ─── Test 5: web_search routes to RuntimeTool via factory (not legacy) ──────

/// web_search is NOT in global runtime_tools (session-scoped deps),
/// but the request-scoped factory should build it from PluginContext so it
/// never falls through to "Unknown tool".
#[tokio::test]
async fn web_search_routes_to_runtime_tool_via_factory() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    // web_search is NOT globally registered, but factory should build it.
    // With empty/fake credentials it will fail with a search error — that's fine.
    // The critical assertion is that it does NOT return "Unknown tool: web_search".
    let result = registry
        .execute(
            "WebSearch",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"query": "test"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await;
    if let Err(e) = &result {
        assert!(
            !e.to_string().contains("Unknown tool"),
            "web_search should not be 'Unknown tool' — factory should route to RuntimeTool. Got: {}",
            e
        );
    }
}

// ─── Test 7: runtime path in ToolRegistry::execute honors check_permissions ──

#[tokio::test]
async fn registry_execute_uses_runtime_tool_check_permissions() {
    use app_lib::runtime::tools::permission::{PermissionDecision, PermissionReason};
    use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{RuntimeTool, ToolDefinition, ToolError, ToolResult};
    use async_trait::async_trait;
    use serde_json::Value;

    struct DenyRuntimeTool;

    #[async_trait]
    impl RuntimeTool for DenyRuntimeTool {
        fn id(&self) -> &str {
            "deny_runtime_tool"
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            ToolDefinition::new("deny_runtime_tool", "deny runtime tool")
        }

        async fn check_permissions(
            &self,
            _input: &Value,
            _ctx: &ToolExecutionContext,
        ) -> Option<PermissionDecision> {
            Some(PermissionDecision::Deny {
                message: "blocked by runtime tool".into(),
                reason: PermissionReason::Other("test".into()),
            })
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(
                "deny_runtime_tool",
                "should not execute",
                None,
            ))
        }
    }

    let registry = ToolRegistry::new();
    registry.register_runtime(Arc::new(DenyRuntimeTool)).await;

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    let result = registry
        .execute(
            "deny_runtime_tool",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await;

    let err = result.expect_err("runtime tool check_permissions should deny before execute");
    assert!(
        err.to_string().contains("blocked by runtime tool"),
        "expected tool-level deny to surface, got: {}",
        err
    );
}

// ─── Test 4: to_runtime_dispatcher uses CapabilityPermissionPipeline ─────────

/// Verify that to_runtime_dispatcher() wraps tools with CapabilityPermissionPipeline.
/// A workspace RuntimeTool dispatched WITHOUT capability context should be
/// rejected (workspace:read scope requires storage capability).
#[tokio::test]
async fn to_runtime_dispatcher_uses_capability_permission_pipeline() {
    use app_lib::runtime::tools::builtin::workspace::ReadWorkspaceFileRuntimeTool;

    let registry = ToolRegistry::new();
    registry
        .register_runtime(Arc::new(ReadWorkspaceFileRuntimeTool))
        .await;

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());
    let dispatcher = registry
        .to_runtime_dispatcher(RequestScopedRuntimeDeps::from_plugin_context(&ctx))
        .await;

    // Dispatch WITHOUT capability context → CapabilityPermissionPipeline should
    // reject the call because workspace:read requires storage capability.
    let exec_ctx =
        app_lib::runtime::tools::ToolExecutionContext::for_test("test-conv", "run-1", "tc-1");
    // No capability attached → permission denied
    let outcome = dispatcher
        .dispatch("Read", serde_json::json!({"file_path": "x.txt"}), exec_ctx)
        .await;
    assert!(
        outcome.is_err(),
        "read_workspace_file without capability should be rejected by CapabilityPermissionPipeline"
    );
}

#[tokio::test]
async fn load_skill_routes_through_request_scoped_runtime_factory() {
    let registry = Arc::new(ToolRegistry::new());
    register_builtin_tools(registry.as_ref()).await;

    let schemas = registry.get_all_schemas().await;
    assert!(
        schemas.iter().any(|schema| schema.name == "Skill"),
        "load_skill schema must be visible to the LLM"
    );
    let daily_filter = ToolFilter::Only(
        DAILY_ALLOWED_TOOLS
            .iter()
            .map(|tool| tool.to_string())
            .collect(),
    );
    let daily_schemas = registry.get_schemas_filtered(&daily_filter, &app_lib::runtime::tools::ToolDescriptionContext::default(), &std::collections::HashMap::new()).await;
    assert!(
        daily_schemas
            .iter()
            .any(|schema| schema.name == "Skill"),
        "load_skill schema must remain visible after daily tool filtering"
    );

    let tmp = TempDir::new().unwrap();
    let mut ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    // Build a disk-backed SkillRegistry with a single biz-writing skill.
    let skills_root = tmp.path().join("skills");
    let biz_dir = skills_root.join("biz-writing");
    std::fs::create_dir_all(&biz_dir).unwrap();
    std::fs::write(
        biz_dir.join("SKILL.md"),
        "---\nname: biz-writing\ndescription: 商务写作\n---\n\nFollow the biz writing checklist.\n",
    )
    .unwrap();
    let loaded = app_lib::plugin::skill::loader::load_skill_roots(&[skills_root]).unwrap();
    let skill_registry = std::sync::Arc::new(std::sync::Mutex::new(
        app_lib::plugin::skill::registry::SkillRegistry::from_skills(
            loaded.into_values().collect(),
        ),
    ));
    ctx.skill_registry = Some(skill_registry);
    ctx.tool_registry = Some(registry.clone());

    let output = registry
        .execute(
            "Skill",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            serde_json::json!({"skill_id": "biz-writing"}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await
        .expect("load_skill should route through request-scoped runtime factory");

    assert!(!output.is_error);
    assert!(output.content.contains("Follow the biz writing checklist."));
    assert!(
        output
            .data
            .as_ref()
            .and_then(|data| data.get("skill_control"))
            .is_none(),
        "load_skill must not emit SkillRuntimePatch data"
    );
}
