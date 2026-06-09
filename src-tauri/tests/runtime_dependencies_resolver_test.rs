use std::path::PathBuf;

use app_lib::runtime::dependencies::{
    InstalledRuntimeResolver, RuntimeLayout, RuntimePlatform, RuntimeResolver,
    StaticRuntimeResolver,
};

#[test]
fn static_runtime_resolver_returns_absolute_workspace_dependencies() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().join("renlijia");
    let python = root.join("python/bin/python3");
    let node = root.join("node/bin/node");
    let npm = root.join("node/bin/npm");
    let npx = root.join("node/bin/npx");
    let uv = root.join("uv");
    let uvx = root.join("uvx");
    let node_modules = root.join("node/node_modules");
    let python_site_packages = root.join("python/lib/python3.12/site-packages");
    let resolver = StaticRuntimeResolver::new(
        python.clone(),
        node.clone(),
        npm.clone(),
        npx.clone(),
        uv.clone(),
        uvx.clone(),
        node_modules.clone(),
        python_site_packages.clone(),
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
    assert_eq!(dependencies.python, python);
    assert_eq!(dependencies.node, node);
    assert_eq!(dependencies.npm, npm);
    assert_eq!(dependencies.npx, npx);
    assert_eq!(dependencies.uv, uv);
    assert_eq!(dependencies.uvx, uvx);
    assert!(dependencies.node_modules.is_absolute());
    assert!(dependencies.python_site_packages.is_absolute());
    assert_eq!(dependencies.node_modules, node_modules);
    assert_eq!(dependencies.python_site_packages, python_site_packages);
}

#[test]
fn static_runtime_resolver_rejects_relative_workspace_dependency_paths() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().join("renlijia");
    let resolver = StaticRuntimeResolver::new(
        PathBuf::from("python/bin/python3"),
        root.join("node/bin/node"),
        root.join("node/bin/npm"),
        root.join("node/bin/npx"),
        root.join("uv"),
        root.join("uvx"),
        root.join("node/node_modules"),
        root.join("python/lib/python3.12/site-packages"),
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
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().join("renlijia");
    let resolver = StaticRuntimeResolver::new(
        root.join("python/bin/python3"),
        root.join("node/bin/node"),
        root.join("node/bin/npm"),
        root.join("node/bin/npx"),
        root.join("uv"),
        root.join("uvx"),
        PathBuf::from("node/node_modules"),
        root.join("python/lib/python3.12/site-packages"),
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
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().join("renlijia");
    let resolver = StaticRuntimeResolver::new(
        root.join("python/bin/python3"),
        root.join("node/bin/node"),
        root.join("node/bin/npm"),
        root.join("node/bin/npx"),
        root.join("uv"),
        root.join("uvx"),
        root.join("node/node_modules"),
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
    let tempdir = tempfile::tempdir().expect("tempdir");
    let install_dir = tempdir
        .path()
        .join("renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1");

    let dependencies =
        app_lib::runtime::dependencies::WorkspaceDependencies::from_install_dir_for_platform(
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
    assert_eq!(
        dependencies.node_modules,
        install_dir.join("node/node_modules")
    );
    assert_eq!(
        dependencies.python_site_packages,
        install_dir.join("python/Lib/site-packages")
    );
}

#[test]
fn workspace_dependencies_use_real_unix_package_directories() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let install_dir = tempdir
        .path()
        .join("renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1");

    let dependencies =
        app_lib::runtime::dependencies::WorkspaceDependencies::from_install_dir_for_platform(
            &install_dir,
            app_lib::runtime::dependencies::RuntimePlatform::DarwinArm64,
        )
        .expect("darwin dependencies should resolve");

    assert_eq!(
        dependencies.node_modules,
        install_dir.join("node/lib/node_modules")
    );
    assert_eq!(
        dependencies.python_site_packages,
        install_dir.join("python/lib/python3.12/site-packages")
    );
}

#[test]
fn installed_resolver_accepts_cache_without_package_directories() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let bundle_root = tempdir.path().join("renlijia-primary-runtime");
    let version = "2026.04.26-runtime.1";
    let install_dir = bundle_root.join("versions").join(version);

    std::fs::create_dir_all(&bundle_root).expect("bundle root");
    std::fs::write(bundle_root.join("current"), format!("versions/{version}"))
        .expect("current pointer");

    let platform = RuntimePlatform::current().expect("platform");
    let layout = RuntimeLayout::for_platform(platform);
    for relative in layout.executable_paths() {
        let path = install_dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("executable parent");
        }
        std::fs::write(path, b"").expect("executable");
    }
    std::fs::write(install_dir.join("install.json"), b"{}").expect("install manifest");

    let deps = InstalledRuntimeResolver::new(&bundle_root)
        .workspace_dependencies()
        .expect("package directories are not required for runtime availability");

    assert_eq!(deps.node, install_dir.join(layout.node()));
    assert_eq!(deps.python, install_dir.join(layout.python()));
}
