#[test]
fn review_hooks_module_no_tauri_dependency() {
    let _ = app_lib::runtime::hooks::config::HookRegistry::new();
    let files = [
        "src/runtime/hooks/mod.rs",
        "src/runtime/hooks/config.rs",
        "src/runtime/hooks/runner.rs",
    ];
    for file in files {
        let content = std::fs::read_to_string(file).expect("read hook module");
        assert!(
            !content.contains("use tauri::"),
            "{file} must not depend on tauri::*"
        );
    }
}

#[test]
fn review_capability_context_not_widened() {
    use app_lib::runtime::tools::capability::CapabilityContext;
    let _ = std::mem::size_of::<CapabilityContext>();
    let content = std::fs::read_to_string("src/runtime/tools/capability.rs")
        .expect("read capability context");
    assert!(
        !content.contains("hook_registry"),
        "CapabilityContext must not gain hook_registry"
    );
}

#[test]
fn review_hook_runner_no_llm_gateway() {
    let _runner = app_lib::runtime::hooks::runner::HookRunner::new();
    let content = std::fs::read_to_string("src/runtime/hooks/runner.rs").expect("read hook runner");
    assert!(
        !content.contains("LlmGateway") && !content.contains("AgentRuntime"),
        "HookRunner must stay independent from LLM/runtime host services"
    );
}
