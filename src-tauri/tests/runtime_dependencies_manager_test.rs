use app_lib::runtime::dependencies::{
    RuntimeManager, RuntimeManifestSource, RuntimePaths, RuntimePlatform, RuntimeResolver,
};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

#[test]
fn manager_ensure_installs_payload_that_shared_resolver_can_read() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let manager = RuntimeManager::new(paths.clone(), "2026.05.02");

    let result = manager.ensure().expect("ensure should install runtime");

    assert!(!result.skipped);
    assert_eq!(result.bundle_version, "2026.05.02");
    assert_eq!(manager.bundle_version(), "2026.05.02");
    let deps = manager
        .resolver()
        .workspace_dependencies()
        .expect("shared resolver should read installed runtime");
    assert_eq!(
        deps.python,
        paths
            .version_dir("2026.05.02")
            .unwrap()
            .join("python/bin/python3")
    );
}

#[test]
fn manager_reinstall_repairs_corrupt_current_payload() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let manager = RuntimeManager::new(paths.clone(), "2026.05.03");
    manager.ensure().expect("initial install");
    std::fs::remove_file(
        paths
            .version_dir("2026.05.03")
            .unwrap()
            .join("node/bin/node"),
    )
    .expect("corrupt node executable");

    assert!(manager.dependencies().is_err());

    manager
        .reinstall()
        .expect("reinstall should rebuild payload");
    manager
        .dependencies()
        .expect("dependencies should resolve after reinstall");
}

#[test]
fn manager_ensure_rejects_corrupt_already_current_payload() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let manager = RuntimeManager::new(paths.clone(), "2026.05.06");
    manager.ensure().expect("initial install");
    std::fs::remove_file(paths.version_dir("2026.05.06").unwrap().join("uv/bin/uv"))
        .expect("corrupt uv executable");

    let error = manager
        .ensure()
        .expect_err("ensure should not skip corrupt current payload");

    assert!(error.to_string().contains("payload is missing"));
}

fn write_manager_runtime_zip(path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("create runtime zip");
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
        std::io::Write::write_all(&mut zip, b"#!/usr/bin/env sh\necho artifact\n")
            .expect("write zip file");
    }
    zip.add_directory("node/node_modules/", SimpleFileOptions::default())
        .expect("add node_modules");
    zip.add_directory("python/lib/site-packages/", SimpleFileOptions::default())
        .expect("add site packages");
    zip.finish().expect("finish zip");
}

fn sha256_hex(path: &std::path::Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).expect("read artifact"))
    )
}


#[test]
fn manager_ensure_uses_configured_file_manifest_source_instead_of_dev_stub() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-ensure-artifact.zip");
    write_manager_runtime_zip(&artifact);
    let sha = sha256_hex(&artifact);
    let manifest = tempdir.path().join("runtime-ensure-manifest.json");
    std::fs::write(
        &manifest,
        format!(
            r#"{{
              "bundleVersion": "2026.05.14",
              "source": "unit-test",
              "runtimes": {{
                "primary": {{
                  "version": "2026.05.14",
                  "platforms": {{
                    "darwin-arm64": {{
                      "url": "file://{}",
                      "sha256": "{}"
                    }}
                  }}
                }}
              }}
            }}"#,
            artifact.display(),
            sha
        ),
    )
    .expect("write manifest");
    let manager = RuntimeManager::new(paths.clone(), "placeholder-version")
        .with_manifest_source(RuntimeManifestSource::File(manifest), "primary", RuntimePlatform::DarwinArm64);

    let result = manager.ensure().expect("ensure should install from manifest");

    assert_eq!(result.bundle_version, "2026.05.14");
    assert_eq!(
        std::fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.14"
    );
    let node = paths.version_dir("2026.05.14").unwrap().join("node/bin/node");
    assert!(node.is_file());
    assert!(!std::fs::read_to_string(node).expect("node script").contains("managed-runtime-stub"));
}

#[tokio::test]
async fn manager_reinstall_uses_configured_file_manifest_source() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-reinstall-artifact.zip");
    write_manager_runtime_zip(&artifact);
    let sha = sha256_hex(&artifact);
    let manifest = tempdir.path().join("runtime-reinstall-manifest.json");
    std::fs::write(
        &manifest,
        format!(
            r#"{{
              "bundleVersion": "2026.05.15",
              "source": "unit-test",
              "runtimes": {{
                "primary": {{
                  "version": "2026.05.15",
                  "platforms": {{
                    "darwin-arm64": {{
                      "url": "file://{}",
                      "sha256": "{}"
                    }}
                  }}
                }}
              }}
            }}"#,
            artifact.display(),
            sha
        ),
    )
    .expect("write manifest");
    let manager = RuntimeManager::new(paths.clone(), "placeholder-version")
        .with_manifest_source(RuntimeManifestSource::File(manifest), "primary", RuntimePlatform::DarwinArm64);

    manager.ensure().expect("initial install from manifest");
    std::fs::remove_file(paths.version_dir("2026.05.15").unwrap().join("node/bin/node"))
        .expect("corrupt installed runtime");
    assert!(manager.dependencies().is_err());

    manager.reinstall().expect("reinstall should repair from manifest artifact");

    manager.dependencies().expect("manifest reinstall should repair dependencies");
}



#[test]
fn manager_runtime_resolver_ensures_from_manifest_before_returning_dependencies() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-resolver-artifact.zip");
    write_manager_runtime_zip(&artifact);
    let sha = sha256_hex(&artifact);
    let manifest = tempdir.path().join("runtime-resolver-manifest.json");
    std::fs::write(
        &manifest,
        format!(
            r#"{{
              "bundleVersion": "2026.05.16",
              "source": "unit-test",
              "runtimes": {{
                "primary": {{
                  "version": "2026.05.16",
                  "platforms": {{
                    "darwin-arm64": {{
                      "url": "file://{}",
                      "sha256": "{}"
                    }}
                  }}
                }}
              }}
            }}"#,
            artifact.display(),
            sha
        ),
    )
    .expect("write manifest");
    let manager = RuntimeManager::new(paths.clone(), "placeholder-version")
        .with_manifest_source(RuntimeManifestSource::File(manifest), "primary", RuntimePlatform::DarwinArm64);

    let deps = RuntimeResolver::workspace_dependencies(&manager)
        .expect("resolver should ensure configured manifest before returning dependencies");

    assert_eq!(deps.node, paths.version_dir("2026.05.16").unwrap().join("node/bin/node"));
    assert_eq!(
        std::fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.16"
    );
}

#[test]
fn manager_installs_verified_archive_for_downloaded_artifact_boundary() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime.zip");
    write_manager_runtime_zip(&artifact);
    let manager = RuntimeManager::new(paths.clone(), "2026.05.11");

    manager
        .install_verified_archive(&artifact, &sha256_hex(&artifact))
        .expect("manager should install verified artifact");

    let deps = manager
        .dependencies()
        .expect("installed archive should resolve through manager");
    assert_eq!(
        deps.node,
        paths
            .version_dir("2026.05.11")
            .unwrap()
            .join("node/bin/node")
    );
}

#[test]
fn manager_installs_runtime_from_file_manifest_source() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-manifest-artifact.zip");
    write_manager_runtime_zip(&artifact);
    let sha = sha256_hex(&artifact);
    let manifest = tempdir.path().join("runtime-manifest.json");
    std::fs::write(
        &manifest,
        format!(
            r#"{{
              "bundleVersion": "2026.05.12",
              "source": "unit-test",
              "runtimes": {{
                "primary": {{
                  "version": "2026.05.12",
                  "platforms": {{
                    "darwin-arm64": {{
                      "url": "file://{}",
                      "sha256": "{}"
                    }}
                  }}
                }}
              }}
            }}"#,
            artifact.display(),
            sha
        ),
    )
    .expect("write manifest");
    let manager = RuntimeManager::new(paths.clone(), "placeholder-version");

    manager
        .install_from_manifest_source(
            RuntimeManifestSource::File(manifest),
            "primary",
            RuntimePlatform::DarwinArm64,
        )
        .expect("manager should fetch and install artifact from manifest");

    assert_eq!(
        std::fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.12"
    );
    manager
        .dependencies()
        .expect("installed runtime should resolve");
}

#[test]
fn manager_manifest_install_checksum_failure_does_not_switch_current() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let artifact = tempdir.path().join("runtime-bad-sha.zip");
    write_manager_runtime_zip(&artifact);
    let manifest = tempdir.path().join("runtime-manifest-bad-sha.json");
    std::fs::write(
        &manifest,
        format!(
            r#"{{
              "bundleVersion": "2026.05.13",
              "source": "unit-test",
              "runtimes": {{
                "primary": {{
                  "version": "2026.05.13",
                  "platforms": {{
                    "darwin-arm64": {{
                      "url": "file://{}",
                      "sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    }}
                  }}
                }}
              }}
            }}"#,
            artifact.display()
        ),
    )
    .expect("write manifest");
    let manager = RuntimeManager::new(paths.clone(), "placeholder-version");

    let error = manager
        .install_from_manifest_source(
            RuntimeManifestSource::File(manifest),
            "primary",
            RuntimePlatform::DarwinArm64,
        )
        .expect_err("checksum mismatch should fail");

    assert!(error.to_string().contains("checksum mismatch"));
    assert!(!paths.current_dir().exists());
}

#[tokio::test]
async fn manager_manifest_url_install_rejects_untrusted_manifest_before_network() {
    let tempdir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");
    let manager = RuntimeManager::new(paths.clone(), "placeholder-version");

    let error = manager
        .install_from_manifest_url(
            "https://localhost/runtime-manifest.json",
            "primary",
            RuntimePlatform::DarwinArm64,
        )
        .await
        .expect_err("localhost manifest should be rejected before network");

    assert!(error.to_string().contains("untrusted runtime artifact url"));
    assert!(!paths.current_dir().exists());
}
