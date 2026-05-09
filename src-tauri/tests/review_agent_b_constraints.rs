use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::RuntimeTool;

/// Legacy modules that still depend on `tauri::*` and are tracked under separate
/// architectural cleanup tickets. New code MUST NOT add to this list — extend
/// `RuntimeHost` / event sink traits and inject from the transport layer instead.
const LEGACY_TAURI_ALLOWED: &[&str] = &[
    // worker_runtime currently holds a tauri::AppHandle and emits stream deltas
    // via tauri::Emitter directly. Pending P-runtime-host-trait refactor.
    "runtime/agent/worker_runtime.rs",
];

fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn rel_path_str(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.replace('\\', "/")
}

#[test]
fn runtime_modules_do_not_use_tauri_directly() {
    let root = Path::new("src/runtime");
    assert!(
        root.is_dir(),
        "expected to run from src-tauri/ working dir; src/runtime missing"
    );

    let mut files = Vec::new();
    collect_rs_files(root, &mut files);
    assert!(
        !files.is_empty(),
        "no .rs files found under src/runtime — test setup broken"
    );

    let mut violations = Vec::new();
    for path in &files {
        let rel = rel_path_str(path);
        // `src/runtime/agent/worker_runtime.rs` → strip `src/` prefix to compare
        let allow_key = rel.strip_prefix("src/").unwrap_or(&rel);
        if LEGACY_TAURI_ALLOWED.iter().any(|a| allow_key == *a) {
            continue;
        }
        let src = std::fs::read_to_string(path).expect("readable .rs file");
        // Match any direct tauri:: usage. We exclude `tauri_build` /
        // `tauri_plugin_*` / `tauri_specta` etc. by anchoring to `tauri::`.
        let banned_patterns: &[&str] = &[
            "use tauri::",
            "tauri::Manager",
            "tauri::Emitter",
            "tauri::AppHandle",
            "tauri::State",
            "tauri::Window",
            "tauri::Wry",
        ];
        for pat in banned_patterns {
            if src.contains(pat) {
                violations.push(format!("{rel} contains forbidden `{pat}`"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "runtime/ layer must not depend on tauri::* directly. \
         Add the file to LEGACY_TAURI_ALLOWED with a TODO ticket if intentional. \
         Violations:\n{}",
        violations.join("\n")
    );
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
        &["ask_user_question".to_string(), "Read".to_string()],  // available
        true,  // is_async
        false, // allow_recursive_spawn
    );
    assert!(
        !allowed.contains(&"ask_user_question".to_string()),
        "async agents must never get ask_user_question"
    );
}
