use std::ffi::OsString;

use app_lib::runtime::dependencies::{
    ManagedRuntimePreference, ManagedRuntimeProcessEnv, WorkspaceDependencies,
};
use tempfile::TempDir;

fn workspace_dependencies(root: &std::path::Path) -> WorkspaceDependencies {
    WorkspaceDependencies {
        python: root.join("python").join("bin").join("python3"),
        node: root.join("node").join("bin").join("node"),
        npm: root.join("node").join("bin").join("npm"),
        npx: root.join("node").join("bin").join("npx"),
        uv: root.join("uv").join("bin").join("uv"),
        uvx: root.join("uv").join("bin").join("uvx"),
        node_modules: root.join("node").join("lib").join("node_modules"),
        python_site_packages: root
            .join("python")
            .join("lib")
            .join("python3.12")
            .join("site-packages"),
    }
}

#[test]
fn managed_runtime_process_env_builds_single_patch_for_all_local_runtimes() {
    let tmp = TempDir::new().unwrap();
    let runtime_root = tmp.path().join("runtime");
    let deps = workspace_dependencies(&runtime_root);
    let system_bin = tmp.path().join("system-bin");
    let existing_path =
        std::env::join_paths([deps.node.parent().unwrap(), system_bin.as_path()]).unwrap();

    let env =
        ManagedRuntimeProcessEnv::from_dependencies_with_existing_path(&deps, Some(existing_path));

    let path_value = env.get("PATH").expect("PATH env is present");
    let path_entries: Vec<_> = std::env::split_paths(path_value).collect();
    assert_eq!(path_entries[0], deps.node.parent().unwrap());
    assert_eq!(path_entries[1], deps.python.parent().unwrap());
    assert_eq!(path_entries[2], deps.uv.parent().unwrap());
    assert_eq!(path_entries[3], system_bin);
    assert_eq!(
        path_entries
            .iter()
            .filter(|entry| *entry == deps.node.parent().unwrap())
            .count(),
        1
    );
    assert_eq!(env.get("NODE_PATH"), Some(deps.node_modules.as_os_str()));
    assert_eq!(
        env.get("npm_config_prefix"),
        Some(runtime_root.join("node").as_os_str())
    );
    assert_eq!(
        env.get("npm_config_cache"),
        Some(runtime_root.join("node").join(".npm-cache").as_os_str())
    );
}

#[test]
fn managed_runtime_process_env_uses_windows_node_prefix_shape() {
    let tmp = TempDir::new().unwrap();
    let runtime_root = tmp.path().join("runtime");
    let mut deps = workspace_dependencies(&runtime_root);
    deps.node_modules = runtime_root.join("node").join("node_modules");

    let env = ManagedRuntimeProcessEnv::from_dependencies_with_existing_path(
        &deps,
        Option::<OsString>::None,
    );

    assert_eq!(
        env.get("npm_config_prefix"),
        Some(runtime_root.join("node").as_os_str())
    );
}

#[test]
fn managed_runtime_preference_defaults_on_and_can_be_toggled() {
    let preference = ManagedRuntimePreference::default();
    assert!(preference.is_enabled());

    preference.set_enabled(false);
    assert!(!preference.is_enabled());

    preference.set_enabled(true);
    assert!(preference.is_enabled());
}
