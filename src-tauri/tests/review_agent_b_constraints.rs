use async_trait::async_trait;
use std::sync::Arc;
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::RuntimeTool;

#[test]
fn agent_modules_do_not_use_tauri_directly() {
    for path in &[
        "src/runtime/agent/markdown_loader.rs",
        "src/runtime/agent/registry.rs",
        "src/runtime/agent/registry_loader.rs",
        "src/runtime/agent/async_task_store.rs",
        "src/runtime/agent/task_notification.rs",
        "src/runtime/agent/output_writer.rs",
        "src/runtime/agent/tool_whitelist.rs",
    ] {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("missing {}", path));
        assert!(
            !src.contains("use tauri::") && !src.contains("tauri::Manager"),
            "{} must not import tauri::* (runtime layer purity)",
            path
        );
    }
}

struct NoopLauncher;
#[async_trait]
impl SpawnSubagentLauncher for NoopLauncher {
    async fn launch_sync(&self, _: SpawnSubagentRequest, _: SpawnSubagentContext) -> anyhow::Result<String> { unreachable!() }
    async fn launch_async(&self, _: SpawnSubagentRequest, _: SpawnSubagentContext) -> anyhow::Result<SpawnAsyncOutcome> { unreachable!() }
}

#[test]
fn spawn_subagent_tool_is_concurrency_safe() {
    let tool = SpawnSubagentRuntimeTool::new(
        Arc::new(NoopLauncher),
        Arc::new(AgentRegistry::with_builtins()),
    );
    assert!(tool.is_concurrency_safe(&serde_json::Value::Null));
}

#[test]
fn async_agent_default_disallows_ask_user_question() {
    use app_lib::runtime::agent::tool_whitelist::resolve_agent_tools;
    let allowed = resolve_agent_tools(
        &[],  // def_allowed = empty (= all)
        &[],  // def_disallowed
        &["ask_user_question".to_string(), "read_file".to_string()],  // available
        true,  // is_async
        false, // allow_recursive_spawn
    );
    assert!(
        !allowed.contains(&"ask_user_question".to_string()),
        "async agents must never get ask_user_question"
    );
}
