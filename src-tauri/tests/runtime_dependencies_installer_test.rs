use std::fs;
use std::path::Path;

use app_lib::runtime::dependencies::{
    InstalledRuntimeResolver, RuntimeDependencyError, RuntimeInstallError, RuntimeInstallPlan,
    RuntimeInstaller, RuntimeLayout, RuntimePaths, RuntimeResolver,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

fn read_json(path: &Path) -> Value {
    let content = fs::read_to_string(path).expect("read json file");
    serde_json::from_str(&content).expect("valid json")
}

#[test]
fn skips_when_current_pointer_and_install_manifest_match_bundle_version() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");

    let version_dir = paths.version_dir("2026.04.25").expect("version dir");
    let installer = RuntimeInstaller::new(paths.clone());
    installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.25"))
        .expect("first install should create complete payload");

    let result = installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.25"))
        .expect("installer should skip already-local runtime");

    assert!(result.skipped);
    assert_eq!(result.bundle_version, "2026.04.25");
    assert_eq!(result.install_dir, version_dir);
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("read current pointer"),
        "versions/2026.04.25"
    );
}

#[test]
fn installs_into_versions_and_updates_current_pointer_without_current_directory_payload() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");

    let installer = RuntimeInstaller::new(paths.clone());
    let result = installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.26"))
        .expect("installer should scaffold install");

    assert!(!result.skipped);
    assert_eq!(result.bundle_version, "2026.04.26");
    assert_eq!(
        result.install_dir,
        paths.version_dir("2026.04.26").expect("version dir")
    );

    let install_manifest_path = result.install_dir.join("install.json");
    assert!(install_manifest_path.is_file());
    assert_eq!(
        read_json(&install_manifest_path)
            .get("bundleVersion")
            .and_then(Value::as_str),
        Some("2026.04.26")
    );

    let current_pointer = paths.current_dir();
    assert!(
        current_pointer.is_file(),
        "current should be a pointer file"
    );
    assert_eq!(
        fs::read_to_string(&current_pointer).expect("read current pointer"),
        "versions/2026.04.26"
    );
    assert!(
        !current_pointer.join("install.json").exists(),
        "current/install.json must never be created"
    );

    let bundle_root = paths
        .bundle_root()
        .canonicalize()
        .expect("bundle root canonical");
    let install_dir = result
        .install_dir
        .canonicalize()
        .expect("install dir canonical");
    let downloads_dir = paths
        .downloads_dir()
        .canonicalize()
        .expect("downloads dir canonical");
    let staging_dir = paths
        .staging_dir()
        .canonicalize()
        .expect("staging dir canonical");
    assert!(install_dir.starts_with(&bundle_root));
    assert!(downloads_dir.starts_with(&bundle_root));
    assert!(staging_dir.starts_with(&bundle_root));
}

#[test]
fn rejects_unsafe_bundle_version_before_writing_anything() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");
    let installer = RuntimeInstaller::new(paths.clone());

    let error = installer
        .ensure(RuntimeInstallPlan::already_local("../escape"))
        .expect_err("unsafe version should be rejected");

    match error {
        RuntimeInstallError::InvalidPath(value) => {
            assert!(value.contains("../escape"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    assert!(
        !paths.bundle_root().exists(),
        "bundle root should not be created"
    );
}

#[test]
fn rejects_pointer_to_existing_version_directory_when_payload_is_missing() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");

    fs::create_dir_all(paths.bundle_root()).expect("create bundle root");
    fs::write(paths.current_dir(), "versions/2026.04.27").expect("write current pointer");
    let version_dir = paths.version_dir("2026.04.27").expect("version dir");
    fs::create_dir_all(&version_dir).expect("create version dir");

    let installer = RuntimeInstaller::new(paths.clone());
    let error = installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.27"))
        .expect_err("installer should reject incomplete existing payload");

    assert!(matches!(error, RuntimeInstallError::MissingPayload(_)));
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("read current pointer"),
        "versions/2026.04.27",
        "failed repair should not silently rewrite current"
    );
    assert!(
        !paths.current_dir().join("install.json").exists(),
        "current/install.json must never be created"
    );
}

#[test]
fn rejects_existing_version_directory_when_payload_is_missing() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");

    let version_dir = paths.version_dir("2026.04.28").expect("version dir");
    fs::create_dir_all(&version_dir).expect("create version dir");
    let sentinel = version_dir.join("runtime-binary");
    fs::write(&sentinel, "existing runtime payload").expect("write sentinel");

    let installer = RuntimeInstaller::new(paths.clone());
    let error = installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.28"))
        .expect_err("installer should reject incomplete existing payload");

    assert!(matches!(error, RuntimeInstallError::MissingPayload(_)));
    assert_eq!(
        fs::read_to_string(&sentinel).expect("read sentinel"),
        "existing runtime payload"
    );
    assert!(
        !paths.current_dir().exists(),
        "incomplete payload must not become current"
    );
}

#[test]
fn overwrites_existing_current_pointer_when_installing_new_version() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");

    fs::create_dir_all(paths.bundle_root()).expect("create bundle root");
    fs::write(paths.current_dir(), "versions/2026.04.28").expect("write old pointer");

    let installer = RuntimeInstaller::new(paths.clone());
    let result = installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.29"))
        .expect("installer should replace existing current pointer");

    assert!(!result.skipped);
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("read current pointer"),
        "versions/2026.04.29"
    );
    assert!(!paths.bundle_root().join("current.tmp").exists());
}

#[test]
fn cleanup_old_versions_removes_non_current_versions_and_keeps_current() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let installer = RuntimeInstaller::new(paths.clone());
    installer
        .ensure(RuntimeInstallPlan::already_local("2026.05.19"))
        .expect("install old");
    installer
        .ensure(RuntimeInstallPlan::already_local("2026.05.20"))
        .expect("install current");

    let result = installer
        .cleanup_old_versions(1)
        .expect("cleanup old versions");

    assert_eq!(result.removed_versions, vec!["2026.05.19"]);
    assert!(result.kept_versions.contains(&"2026.05.20".to_string()));
    assert!(!paths.version_dir("2026.05.19").unwrap().exists());
    assert!(paths.version_dir("2026.05.20").unwrap().exists());
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.20"
    );
}

#[test]
fn cleanup_old_versions_never_deletes_current_even_when_current_is_oldest() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let installer = RuntimeInstaller::new(paths.clone());
    installer
        .ensure(RuntimeInstallPlan::already_local("2026.05.20"))
        .expect("install newer");
    installer
        .ensure(RuntimeInstallPlan::already_local("2026.05.19"))
        .expect("install older current");

    let result = installer
        .cleanup_old_versions(0)
        .expect("cleanup old versions");

    assert!(!result.removed_versions.contains(&"2026.05.19".to_string()));
    assert!(paths.version_dir("2026.05.19").unwrap().exists());
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.19"
    );
}

#[test]
fn install_manifest_contains_relative_runtime_paths_and_metadata() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");

    let result = RuntimeInstaller::new(paths.clone())
        .ensure(RuntimeInstallPlan::already_local("2026.05.19"))
        .expect("install runtime");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(result.install_dir.join("install.json"))
            .expect("read install manifest"),
    )
    .expect("parse install manifest");

    assert_eq!(manifest["bundleVersion"], "2026.05.19");
    assert_eq!(
        manifest["platform"],
        app_lib::runtime::dependencies::RuntimePlatform::current()
            .unwrap()
            .manifest_key()
    );
    assert_eq!(manifest["paths"]["node"], "node/bin/node");
    assert_eq!(manifest["paths"]["npm"], "node/bin/npm");
    assert_eq!(manifest["paths"]["npx"], "node/bin/npx");
    assert_eq!(manifest["paths"]["python"], "python/bin/python3");
    assert_eq!(manifest["paths"]["uv"], "uv/bin/uv");
    assert_eq!(manifest["paths"]["uvx"], "uv/bin/uvx");
    let layout = RuntimeLayout::current().expect("current layout");
    assert_eq!(manifest["paths"]["nodeModules"], layout.node_modules());
    assert_eq!(
        manifest["paths"]["pythonSitePackages"],
        layout.python_site_packages()
    );
    assert_eq!(manifest["runtimes"]["node"]["path"], "node");
    assert_eq!(manifest["runtimes"]["python"]["path"], "python");
    assert_eq!(manifest["runtimes"]["uv"]["path"], "uv");
}

#[test]
fn installed_payload_contains_required_executables_and_resolves_successfully() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");

    let installer = RuntimeInstaller::new(paths.clone());
    installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.30"))
        .expect("installer should create payload");

    let deps = InstalledRuntimeResolver::new(paths.bundle_root())
        .workspace_dependencies()
        .expect("installed resolver should validate payload files");

    for executable in [
        &deps.python,
        &deps.node,
        &deps.npm,
        &deps.npx,
        &deps.uv,
        &deps.uvx,
    ] {
        assert!(
            executable.is_file(),
            "{} should exist",
            executable.display()
        );
    }
}

#[test]
fn reinstall_replaces_existing_version_payload_even_when_current_matches() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");
    let version_dir = paths.version_dir("2026.05.01").expect("version dir");
    fs::create_dir_all(version_dir.join("python/bin")).expect("create partial payload");
    fs::write(
        version_dir.join("install.json"),
        serde_json::json!({
            "bundleVersion": "2026.05.01"
        })
        .to_string(),
    )
    .expect("write manifest");
    fs::create_dir_all(paths.bundle_root()).expect("create bundle root");
    fs::write(paths.current_dir(), "versions/2026.05.01").expect("write current pointer");

    let before = InstalledRuntimeResolver::new(paths.bundle_root())
        .workspace_dependencies()
        .expect_err("partial payload should not resolve before reinstall");
    assert!(matches!(
        before,
        RuntimeDependencyError::MissingExecutable { .. }
    ));

    RuntimeInstaller::new(paths.clone())
        .reinstall(RuntimeInstallPlan::already_local("2026.05.01"))
        .expect("reinstall should rebuild payload");

    InstalledRuntimeResolver::new(paths.bundle_root())
        .workspace_dependencies()
        .expect("reinstall should restore all executables");
}

fn write_runtime_zip(path: &Path) {
    let file = fs::File::create(path).expect("create runtime zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().unix_permissions(0o755);
    for entry in [
        "python/bin/python3",
        "node/bin/node",
        "node/bin/npm",
        "node/bin/npx",
        "uv/bin/uv",
        "uv/bin/uvx",
    ] {
        zip.start_file(entry, options).expect("start zip file");
        std::io::Write::write_all(
            &mut zip,
            format!("#!/usr/bin/env sh\necho {entry} real-artifact\n").as_bytes(),
        )
        .expect("write zip executable");
    }
    zip.add_directory("node/node_modules/", SimpleFileOptions::default())
        .expect("add node_modules dir");
    zip.add_directory("python/lib/site-packages/", SimpleFileOptions::default())
        .expect("add site-packages dir");
    zip.finish().expect("finish zip");
}

fn write_runtime_tar_gz(path: &Path) {
    let file = fs::File::create(path).expect("create runtime tar.gz");
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for entry in [
        "python/bin/python3",
        "node/bin/node",
        "node/bin/npm",
        "node/bin/npx",
        "uv/bin/uv",
        "uv/bin/uvx",
    ] {
        let script = format!("#!/usr/bin/env sh\necho {entry} tar-artifact\n");
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, entry, script.as_bytes())
            .expect("append executable");
    }
    for dir in ["node/node_modules", "python/lib/site-packages"] {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, dir, std::io::empty())
            .expect("append dir");
    }
    tar.finish().expect("finish tar");
}

fn write_runtime_tar_gz_without_package_dirs(path: &Path) {
    let file = fs::File::create(path).expect("create runtime tar.gz");
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for entry in [
        "python/bin/python3",
        "node/bin/node",
        "node/bin/npm",
        "node/bin/npx",
        "uv/bin/uv",
        "uv/bin/uvx",
    ] {
        let script = format!("#!/usr/bin/env sh\necho {entry} tar-artifact\n");
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, entry, script.as_bytes())
            .expect("append executable");
    }
    tar.finish().expect("finish tar");
}

fn write_runtime_tar_gz_with_symlinked_bins(path: &Path) {
    let file = fs::File::create(path).expect("create runtime tar.gz");
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);

    for entry in [
        "python/bin/python3.12",
        "node/bin/node",
        "node/lib/node_modules/npm/bin/npm-cli.js",
        "node/lib/node_modules/npm/bin/npx-cli.js",
        "uv/bin/uv",
        "uv/bin/uvx",
    ] {
        let script = format!("#!/usr/bin/env sh\necho {entry} tar-artifact\n");
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, entry, script.as_bytes())
            .expect("append executable");
    }

    for (link, target) in [
        ("python/bin/python3", "python3.12"),
        ("node/bin/npm", "../lib/node_modules/npm/bin/npm-cli.js"),
        ("node/bin/npx", "../lib/node_modules/npm/bin/npx-cli.js"),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_link_name(target).expect("set link target");
        header.set_cksum();
        tar.append_data(&mut header, link, std::io::empty())
            .expect("append symlink");
    }

    for dir in ["node/node_modules", "python/lib/site-packages"] {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, dir, std::io::empty())
            .expect("append dir");
    }
    tar.finish().expect("finish tar");
}

#[test]
fn installs_tar_gz_artifact_with_symlinked_runtime_bins() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-symlinked.tar.gz");
    write_runtime_tar_gz_with_symlinked_bins(&artifact);

    let result = RuntimeInstaller::new(paths.clone())
        .install_from_local_archive(RuntimeInstallPlan::already_local("2026.05.19"), &artifact)
        .expect("installer should preserve symlinked runtime bins");

    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.19"
    );
    assert!(result.install_dir.join("python/bin/python3").is_file());
    assert!(result.install_dir.join("node/bin/npm").is_file());
    assert!(result.install_dir.join("node/bin/npx").is_file());
}

#[test]
fn installs_from_tar_gz_artifact_and_updates_current_pointer() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime.tar.gz");
    write_runtime_tar_gz(&artifact);

    let result = RuntimeInstaller::new(paths.clone())
        .install_from_local_archive(RuntimeInstallPlan::already_local("2026.05.17"), &artifact)
        .expect("installer should extract tar.gz artifact");

    assert_eq!(result.bundle_version, "2026.05.17");
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.17"
    );
    assert!(result.install_dir.join("node/bin/node").is_file());
}

#[test]
fn archive_install_creates_runtime_package_directories_when_missing() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-no-package-dirs.tar.gz");
    write_runtime_tar_gz_without_package_dirs(&artifact);

    let result = RuntimeInstaller::new(paths.clone())
        .install_from_local_archive(RuntimeInstallPlan::already_local("2026.05.20"), &artifact)
        .expect("installer should not depend on archive package dirs");

    let layout = RuntimeLayout::current().expect("current layout");
    assert!(result.install_dir.join(layout.node_modules()).is_dir());
    assert!(result
        .install_dir
        .join(layout.python_site_packages())
        .is_dir());
    InstalledRuntimeResolver::new(paths.bundle_root())
        .workspace_dependencies()
        .expect("installed archive should resolve after package dirs are created");
}

#[test]
fn rejects_invalid_tar_gz_artifact_before_switching_current() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("bad-runtime.tar.gz");
    fs::write(&artifact, b"not a valid tar.gz").expect("write invalid tar.gz");

    let error = RuntimeInstaller::new(paths.clone())
        .install_from_local_archive(RuntimeInstallPlan::already_local("2026.05.18"), &artifact)
        .expect_err("invalid tar.gz must fail");

    assert!(error.to_string().contains("runtime install io error"));
    assert!(!paths.current_dir().exists());
}

#[test]
fn installs_from_verified_zip_artifact_and_updates_current_pointer() {
    let tempdir = tempdir().expect("tempdir");
    let cache_root = tempdir.path().join("cache-root");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid paths");
    let artifact = tempdir.path().join("runtime.zip");
    write_runtime_zip(&artifact);

    let installer = RuntimeInstaller::new(paths.clone());
    let result = installer
        .install_from_local_archive(RuntimeInstallPlan::already_local("2026.05.07"), &artifact)
        .expect("installer should extract verified artifact");

    assert!(!result.skipped);
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("read current"),
        "versions/2026.05.07"
    );
    assert!(result.install_dir.join("node/bin/node").is_file());
    let layout = RuntimeLayout::current().expect("current layout");
    assert!(result
        .install_dir
        .join(layout.python_site_packages())
        .is_dir());
}

fn write_runtime_zip_with_failing_tool(path: &Path, failing_entry: &str) {
    let file = fs::File::create(path).expect("create runtime zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().unix_permissions(0o755);
    for entry in [
        "python/bin/python3",
        "node/bin/node",
        "node/bin/npm",
        "node/bin/npx",
        "uv/bin/uv",
        "uv/bin/uvx",
    ] {
        zip.start_file(entry, options).expect("start zip file");
        let script = if entry == failing_entry {
            format!("#!/usr/bin/env sh\necho {entry} broken >&2\nexit 42\n")
        } else {
            format!("#!/usr/bin/env sh\necho {entry} real-artifact\n")
        };
        std::io::Write::write_all(&mut zip, script.as_bytes()).expect("write zip executable");
    }
    zip.add_directory("node/node_modules/", SimpleFileOptions::default())
        .expect("add node_modules dir");
    zip.add_directory("python/lib/site-packages/", SimpleFileOptions::default())
        .expect("add site-packages dir");
    zip.finish().expect("finish zip");
}

#[test]
fn rejects_zip_artifact_when_staging_smoke_test_fails_before_switching_current() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    RuntimeInstaller::new(paths.clone())
        .ensure(RuntimeInstallPlan::already_local("2026.05.10"))
        .expect("install existing runtime");
    let artifact = tempdir.path().join("runtime-smoke-fails.zip");
    write_runtime_zip_with_failing_tool(&artifact, "node/bin/node");

    let error = RuntimeInstaller::new(paths.clone())
        .install_from_local_archive(RuntimeInstallPlan::reinstall("2026.05.11"), &artifact)
        .expect_err("smoke test failure should fail installation");

    assert!(error
        .to_string()
        .contains("runtime install smoke test failed"));
    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.10"
    );
    assert!(
        !paths
            .version_dir("2026.05.11")
            .expect("version dir")
            .exists(),
        "failed staging payload must not be promoted into versions"
    );
}

#[test]
fn rejects_zip_artifact_with_path_traversal_entry() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("bad-runtime.zip");
    let file = fs::File::create(&artifact).expect("create bad zip");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("../escape", SimpleFileOptions::default())
        .expect("start unsafe entry");
    std::io::Write::write_all(&mut zip, b"bad").expect("write unsafe entry");
    zip.finish().expect("finish bad zip");

    let error = RuntimeInstaller::new(paths.clone())
        .install_from_local_archive(RuntimeInstallPlan::already_local("2026.05.08"), &artifact)
        .expect_err("unsafe archive must fail");

    assert!(error.to_string().contains("unsafe archive entry path"));
    assert!(!paths.current_dir().exists());
}

fn write_windows_runtime_zip(path: &Path) {
    let file = fs::File::create(path).expect("create windows runtime zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for entry in [
        "python/python.exe",
        "node/node.exe",
        "node/npm.cmd",
        "node/npx.cmd",
        "uv/uv.exe",
        "uv/uvx.exe",
    ] {
        zip.start_file(entry, options).expect("start zip file");
        std::io::Write::write_all(
            &mut zip,
            format!("@echo off\r\necho {entry} windows-artifact\r\n").as_bytes(),
        )
        .expect("write zip executable");
    }
    zip.add_directory("node/node_modules/", SimpleFileOptions::default())
        .expect("add node_modules dir");
    zip.add_directory("python/Lib/site-packages/", SimpleFileOptions::default())
        .expect("add site-packages dir");
    zip.finish().expect("finish zip");
}

#[test]
fn installs_windows_zip_artifact_with_platform_layout_without_cross_platform_smoke() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-win32-x64.zip");
    write_windows_runtime_zip(&artifact);

    let result = RuntimeInstaller::new_for_platform(
        paths.clone(),
        app_lib::runtime::dependencies::RuntimePlatform::WindowsX64,
    )
    .install_from_local_archive(
        RuntimeInstallPlan::already_local("2026.04.26-runtime.1"),
        &artifact,
    )
    .expect("installer should accept windows layout");

    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.04.26-runtime.1"
    );
    assert!(result.install_dir.join("python/python.exe").is_file());
    assert!(result.install_dir.join("node/npm.cmd").is_file());
    assert!(result.install_dir.join("uv/uvx.exe").is_file());
    assert!(result.install_dir.join("python/Lib/site-packages").is_dir());

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(result.install_dir.join("install.json"))
            .expect("read install manifest"),
    )
    .expect("parse install manifest");
    assert_eq!(manifest["platform"], "win32-x64");
    assert_eq!(manifest["paths"]["node"], "node/node.exe");
    assert_eq!(manifest["paths"]["npm"], "node/npm.cmd");
    assert_eq!(manifest["paths"]["npx"], "node/npx.cmd");
    assert_eq!(manifest["paths"]["python"], "python/python.exe");
    assert_eq!(manifest["paths"]["uv"], "uv/uv.exe");
    assert_eq!(manifest["paths"]["uvx"], "uv/uvx.exe");
    assert_eq!(
        manifest["paths"]["pythonSitePackages"],
        "python/Lib/site-packages"
    );
}

fn sha256_hex(path: &Path) -> String {
    let bytes = fs::read(path).expect("read artifact");
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn installs_from_zip_artifact_only_when_sha256_matches() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-checksum.zip");
    write_runtime_zip(&artifact);
    let expected = sha256_hex(&artifact);

    RuntimeInstaller::new(paths.clone())
        .install_from_verified_archive(
            RuntimeInstallPlan::already_local("2026.05.09"),
            &artifact,
            &expected,
        )
        .expect("matching checksum should install");

    assert_eq!(
        fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.09"
    );
}

#[test]
fn rejects_zip_artifact_when_sha256_mismatches_before_switching_current() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-bad-checksum.zip");
    write_runtime_zip(&artifact);

    let error = RuntimeInstaller::new(paths.clone())
        .install_from_verified_archive(
            RuntimeInstallPlan::already_local("2026.05.10"),
            &artifact,
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .expect_err("checksum mismatch should fail");

    assert!(error.to_string().contains("checksum mismatch"));
    assert!(!paths.current_dir().exists());
}
