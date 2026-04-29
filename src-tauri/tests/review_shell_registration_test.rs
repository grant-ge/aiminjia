//! Verifies that exactly one shell tool is registered for the current OS.
//! On Windows: `powershell`. Elsewhere: `bash`. Catches accidental cfg-gate
//! removal that would either leave Windows with no shell or leave Unix with
//! a phantom `powershell` registration.

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::ToolRegistry;

#[tokio::test]
async fn shell_tool_registered_matches_current_os() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let schemas = registry.get_all_schemas().await;
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    let has_bash = names.contains(&"bash");
    let has_powershell = names.contains(&"powershell");

    if cfg!(windows) {
        assert!(
            has_powershell,
            "Windows must register powershell tool. registered: {names:?}"
        );
        assert!(
            !has_bash,
            "Windows must not register bash tool (no /bin/sh). registered: {names:?}"
        );
    } else {
        assert!(
            has_bash,
            "Unix must register bash tool. registered: {names:?}"
        );
        assert!(
            !has_powershell,
            "Unix must not register powershell tool. registered: {names:?}"
        );
    }
}
