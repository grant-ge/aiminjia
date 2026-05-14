//! Verifies BundledRuntimeResolver reads from a resource_dir-style layout
//! and yields valid WorkspaceDependencies for the current platform.

use std::fs;
use tempfile::TempDir;

use app_lib::runtime::dependencies::{
    BundledRuntimeResolver, RuntimePlatform, RuntimeResolver,
};

#[test]
fn bundled_resolver_finds_runtime_for_current_platform() {
    let tmp = TempDir::new().unwrap();
    let resource_dir = tmp.path();
    let platform = RuntimePlatform::current().expect("platform detection");
    let plat_key = platform.manifest_key();
    let runtime_dir = resource_dir.join("runtime").join(plat_key);

    // Lay out per the spec
    let layout = app_lib::runtime::dependencies::RuntimeLayout::for_platform(platform);
    let deps = layout.workspace_dependencies(&runtime_dir);
    for path in [&deps.python, &deps.node, &deps.npm, &deps.npx, &deps.uv, &deps.uvx] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }
    for dir in [&deps.node_modules, &deps.python_site_packages] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        runtime_dir.join("bundled-version.json"),
        br#"{"bundleVersion":"test-1","platform":"placeholder"}"#,
    )
    .unwrap();

    let resolver = BundledRuntimeResolver::new(resource_dir.to_path_buf());
    let resolved = resolver.workspace_dependencies().expect("resolves");
    assert_eq!(resolved.node, deps.node);
    assert_eq!(resolved.python, deps.python);
}

#[test]
fn bundled_resolver_errors_when_runtime_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let resolver = BundledRuntimeResolver::new(tmp.path().to_path_buf());
    let err = resolver.workspace_dependencies().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("bundled runtime") || msg.contains("not found"),
        "unexpected error: {msg}"
    );
}
