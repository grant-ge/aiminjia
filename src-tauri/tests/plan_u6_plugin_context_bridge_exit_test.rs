use std::path::{Path, PathBuf};

fn walk_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn recurse(path: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    recurse(&p, out);
                } else if p.extension().map(|ext| ext == "rs").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    recurse(dir, &mut out);
    out
}

#[test]
fn u6_registry_dispatcher_requires_request_scoped_deps_instead_of_plugin_context() {
    let source = std::fs::read_to_string("src/plugin/registry.rs")
        .expect("read src/plugin/registry.rs");
    assert!(
        source.contains("RequestScopedRuntimeDeps"),
        "registry must introduce an explicit request-scoped deps model"
    );
    assert!(
        !source.contains("pub async fn to_runtime_dispatcher(&self, plugin_ctx: PluginContext)"),
        "to_runtime_dispatcher must not accept PluginContext directly anymore"
    );
}

#[test]
fn u6_sub_agent_extracts_request_scoped_deps_before_building_dispatcher() {
    let source = std::fs::read_to_string("src/llm/sub_agent.rs")
        .expect("read src/llm/sub_agent.rs");
    assert!(
        source.contains("RequestScopedRuntimeDeps"),
        "sub_agent must build explicit request-scoped deps for runtime dispatcher"
    );
    assert!(
        !source.contains("to_runtime_dispatcher(sub_plugin_ctx.clone())"),
        "sub_agent must not pass the full PluginContext into to_runtime_dispatcher"
    );
}

#[test]
fn u6_runtime_and_transport_main_path_do_not_import_plugin_context() {
    let mut files = walk_rust_files(Path::new("src/runtime/"));
    files.extend(walk_rust_files(Path::new("src/transport/tauri_commands/")));
    files.push(PathBuf::from("src/llm/sub_agent.rs"));

    let allowlist = [
        Path::new("src/transport/tauri_commands/chat.rs"),
        Path::new("src/runtime/tools/legacy_adapter.rs"),
    ];

    for file in files {
        if allowlist.iter().any(|allowed| allowed == &file.as_path()) {
            continue;
        }
        let content = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            !content.contains("use crate::plugin::context::PluginContext;"),
            "{} must not import PluginContext on the runtime-first main path",
            file.display()
        );
    }
}
