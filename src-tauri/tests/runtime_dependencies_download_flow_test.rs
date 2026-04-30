use std::path::{Path, PathBuf};

use app_lib::runtime::dependencies::{RuntimeDownloadPlan, RuntimeManifestSource};

#[test]
fn builds_download_plan_from_url_manifest_source() {
    let source =
        RuntimeManifestSource::Url("https://example.com/runtime-manifest.json".to_string());
    let plan = RuntimeDownloadPlan::new(source, "darwin-arm64".to_string());

    assert_eq!(
        plan.manifest_source().as_url(),
        Some("https://example.com/runtime-manifest.json")
    );
    assert_eq!(plan.platform(), "darwin-arm64");
    assert!(!plan.uses_shell_script());
}

#[test]
fn builds_download_plan_from_file_manifest_source() {
    let manifest_path = PathBuf::from("/tmp/runtime-manifest.json");
    let source = RuntimeManifestSource::File(manifest_path.clone());
    let plan = RuntimeDownloadPlan::new(source, "linux-x64".to_string());

    assert_eq!(plan.manifest_source().as_url(), None);
    assert_eq!(
        plan.manifest_source().as_file(),
        Some(Path::new(&manifest_path))
    );
    assert_eq!(plan.platform(), "linux-x64");
    assert!(!plan.uses_shell_script());
}

#[test]
fn file_manifest_plan_selects_artifact_and_copies_to_downloads() {
    use app_lib::runtime::dependencies::{RuntimeArtifactFetcher, RuntimePlatform};
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    let tempdir = tempdir().expect("tempdir");
    let artifact = tempdir.path().join("runtime.zip");
    std::fs::write(&artifact, b"artifact bytes").expect("write artifact");
    let sha = format!("{:x}", Sha256::digest(b"artifact bytes"));
    let manifest = tempdir.path().join("manifest.json");
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
    let downloads = tempdir.path().join("downloads");

    let fetched = RuntimeArtifactFetcher::new()
        .fetch_from_manifest_source(
            RuntimeManifestSource::File(manifest),
            "primary",
            RuntimePlatform::DarwinArm64,
            &downloads,
        )
        .expect("file artifact should be copied");

    assert_eq!(fetched.bundle_version, "2026.05.12");
    assert_eq!(fetched.sha256, sha);
    assert!(fetched.archive_path.starts_with(&downloads));
    assert_eq!(
        std::fs::read(&fetched.archive_path).expect("read downloaded artifact"),
        b"artifact bytes"
    );
}

#[tokio::test]
async fn manifest_url_fetch_rejects_untrusted_hosts_before_network() {
    use app_lib::runtime::dependencies::{
        RuntimeArtifactFetchError, RuntimeArtifactFetcher, RuntimePlatform,
    };
    use tempfile::tempdir;

    let tempdir = tempdir().expect("tempdir");
    let error = RuntimeArtifactFetcher::new()
        .fetch_from_manifest_url(
            "https://localhost/runtime-manifest.json",
            "primary",
            RuntimePlatform::DarwinArm64,
            &tempdir.path().join("downloads"),
        )
        .await
        .expect_err("localhost manifest url must be rejected before network");

    assert_eq!(
        error,
        RuntimeArtifactFetchError::UntrustedUrl(
            "https://localhost/runtime-manifest.json".to_string()
        )
    );
}
