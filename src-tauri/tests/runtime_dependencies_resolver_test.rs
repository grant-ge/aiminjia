use std::path::PathBuf;

use app_lib::runtime::dependencies::{RuntimeResolver, StaticRuntimeResolver};

#[test]
fn static_runtime_resolver_returns_absolute_workspace_dependencies() {
    let resolver = StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/lib/python3.12/site-packages"),
    );

    let dependencies = resolver
        .workspace_dependencies()
        .expect("dependencies should resolve");

    assert!(dependencies.python.is_absolute());
    assert!(dependencies.node.is_absolute());
    assert!(dependencies.npm.is_absolute());
    assert!(dependencies.npx.is_absolute());
    assert!(dependencies.uv.is_absolute());
    assert!(dependencies.uvx.is_absolute());
    assert_eq!(
        dependencies.python,
        PathBuf::from("/tmp/renlijia/python/bin/python3")
    );
    assert_eq!(
        dependencies.node,
        PathBuf::from("/tmp/renlijia/node/bin/node")
    );
    assert_eq!(
        dependencies.npm,
        PathBuf::from("/tmp/renlijia/node/bin/npm")
    );
    assert_eq!(
        dependencies.npx,
        PathBuf::from("/tmp/renlijia/node/bin/npx")
    );
    assert_eq!(dependencies.uv, PathBuf::from("/tmp/renlijia/uv"));
    assert_eq!(dependencies.uvx, PathBuf::from("/tmp/renlijia/uvx"));
    assert!(dependencies.node_modules.is_absolute());
    assert!(dependencies.python_site_packages.is_absolute());
    assert_eq!(
        dependencies.node_modules,
        PathBuf::from("/tmp/renlijia/node/node_modules")
    );
    assert_eq!(
        dependencies.python_site_packages,
        PathBuf::from("/tmp/renlijia/python/lib/python3.12/site-packages")
    );
}

#[test]
fn static_runtime_resolver_rejects_relative_workspace_dependency_paths() {
    let resolver = StaticRuntimeResolver::new(
        PathBuf::from("python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/lib/python3.12/site-packages"),
    );

    let error = resolver.workspace_dependencies().unwrap_err();

    match error {
        app_lib::runtime::dependencies::RuntimeDependencyError::NonAbsolutePath { field, path } => {
            assert_eq!(field, "python");
            assert_eq!(path, PathBuf::from("python/bin/python3"));
        }
        other => panic!("expected NonAbsolutePath for python, got {other:?}"),
    }
}

#[test]
fn static_runtime_resolver_rejects_relative_node_modules_path() {
    let resolver = StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/lib/python3.12/site-packages"),
    );

    let error = resolver.workspace_dependencies().unwrap_err();

    match error {
        app_lib::runtime::dependencies::RuntimeDependencyError::NonAbsolutePath { field, path } => {
            assert_eq!(field, "node_modules");
            assert_eq!(path, PathBuf::from("node/node_modules"));
        }
        other => panic!("expected NonAbsolutePath for node_modules, got {other:?}"),
    }
}

#[test]
fn static_runtime_resolver_rejects_relative_python_site_packages_path() {
    let resolver = StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("python/lib/python3.12/site-packages"),
    );

    let error = resolver.workspace_dependencies().unwrap_err();

    match error {
        app_lib::runtime::dependencies::RuntimeDependencyError::NonAbsolutePath { field, path } => {
            assert_eq!(field, "python_site_packages");
            assert_eq!(path, PathBuf::from("python/lib/python3.12/site-packages"));
        }
        other => panic!("expected NonAbsolutePath for python_site_packages, got {other:?}"),
    }
}

#[test]
fn workspace_dependencies_support_windows_runtime_layout() {
    let install_dir = PathBuf::from("/tmp/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1");

    let dependencies = app_lib::runtime::dependencies::WorkspaceDependencies::from_install_dir_for_platform(
        &install_dir,
        app_lib::runtime::dependencies::RuntimePlatform::WindowsX64,
    )
    .expect("windows dependencies should resolve");

    assert_eq!(dependencies.python, install_dir.join("python/python.exe"));
    assert_eq!(dependencies.node, install_dir.join("node/node.exe"));
    assert_eq!(dependencies.npm, install_dir.join("node/npm.cmd"));
    assert_eq!(dependencies.npx, install_dir.join("node/npx.cmd"));
    assert_eq!(dependencies.uv, install_dir.join("uv/uv.exe"));
    assert_eq!(dependencies.uvx, install_dir.join("uv/uvx.exe"));
    assert_eq!(dependencies.node_modules, install_dir.join("node/node_modules"));
    assert_eq!(
        dependencies.python_site_packages,
        install_dir.join("python/Lib/site-packages")
    );
}
