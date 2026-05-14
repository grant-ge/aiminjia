//! Verifies BundledRuntimeResolver reads from a resource_dir-style layout
//! and yields valid WorkspaceDependencies for the current platform.

use std::fs;
use std::sync::Arc;
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

#[test]
fn chain_falls_back_to_installed_when_bundled_missing() {
    use app_lib::runtime::dependencies::{ChainResolver, InstalledRuntimeResolver};

    let tmp = TempDir::new().unwrap();
    // bundled: nothing under tmp/resources, so it'll fail
    let bundled = BundledRuntimeResolver::new(tmp.path().join("resources"));
    // installed: build a fake bundle_root with current pointer + version dir
    let bundle_root = tmp.path().join("renlijia-primary-runtime");
    let version = "2026.05.13-runtime.1";
    let install_dir = bundle_root.join("versions").join(version);
    fs::create_dir_all(&bundle_root).unwrap();
    fs::write(bundle_root.join("current"), format!("versions/{version}")).unwrap();

    let platform = RuntimePlatform::current().unwrap();
    let layout = app_lib::runtime::dependencies::RuntimeLayout::for_platform(platform);
    let deps = layout.workspace_dependencies(&install_dir);
    for p in [&deps.python, &deps.node, &deps.npm, &deps.npx, &deps.uv, &deps.uvx] {
        if let Some(parent) = p.parent() { fs::create_dir_all(parent).unwrap(); }
        fs::write(p, b"").unwrap();
    }
    fs::create_dir_all(&deps.node_modules).unwrap();
    fs::create_dir_all(&deps.python_site_packages).unwrap();
    fs::write(install_dir.join("install.json"), b"{}").unwrap();

    let installed = InstalledRuntimeResolver::new(&bundle_root);
    let chain = ChainResolver::new(vec![Arc::new(bundled), Arc::new(installed)]);

    let resolved = chain.workspace_dependencies().expect("chain resolves via installed");
    assert_eq!(resolved.node, deps.node);
}
