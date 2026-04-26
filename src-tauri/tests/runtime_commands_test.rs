use app_lib::runtime::dependencies::{
    RuntimeInstallPlan, RuntimeInstaller, RuntimePaths, WorkspaceDependencies,
};
use app_lib::transport::tauri_commands::runtime::{RuntimeHealthPayload, RuntimeToolHealthPayload};

#[test]
fn runtime_health_payload_serializes_with_camel_case_fields() {
    let payload = RuntimeHealthPayload {
        bundle_version: "2026.04.25".to_string(),
        node: Some(RuntimeToolHealthPayload {
            version: "v22.19.0".to_string(),
            path: "/tmp/renlijia/node/bin/node".to_string(),
        }),
        npm: None,
        npx: None,
        python: None,
        uv: None,
        uvx: None,
    };

    let json = serde_json::to_value(payload).expect("serialize payload");

    assert_eq!(json["bundleVersion"], "2026.04.25");
    assert_eq!(json["node"]["version"], "v22.19.0");
    assert_eq!(json["node"]["path"], "/tmp/renlijia/node/bin/node");
}

#[test]
fn runtime_health_test_helper_uses_real_bundle_version_and_paths() {
    let deps =
        WorkspaceDependencies::from_install_dir(std::path::Path::new("/tmp/renlijia-runtime"))
            .expect("absolute install dir should build deps");

    let payload =
        app_lib::transport::tauri_commands::runtime::runtime_health_payload_from_dependencies(
            "2026.05.04",
            deps,
        );

    assert_eq!(payload.bundle_version, "2026.05.04");
    assert_eq!(
        payload.python.expect("python payload").path,
        "/tmp/renlijia-runtime/python/bin/python3"
    );
}

#[test]
fn runtime_command_dependencies_can_be_installed_before_health_reads_current() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let paths = RuntimePaths::new(
        tempdir.path().join("cache-root"),
        "renlijia-primary-runtime",
    )
    .expect("valid paths");

    let result = RuntimeInstaller::new(paths.clone())
        .ensure(RuntimeInstallPlan::already_local("2026.05.05"))
        .expect("ensure should install command runtime deps");

    assert!(!result.skipped);
    assert_eq!(
        std::fs::read_to_string(paths.current_dir()).expect("current pointer"),
        "versions/2026.05.05"
    );
    assert!(
        paths
            .version_dir("2026.05.05")
            .expect("version dir")
            .join("node/bin/node")
            .is_file(),
        "ensure must create executable payload for health/resolver"
    );
}


#[test]
fn runtime_cleanup_payload_serializes_removed_and_kept_versions() {
    let payload = app_lib::transport::tauri_commands::runtime::RuntimeCleanupPayload {
        removed_versions: vec!["2026.05.19".to_string()],
        kept_versions: vec!["2026.05.20".to_string()],
    };

    let json = serde_json::to_value(payload).expect("serialize cleanup payload");

    assert_eq!(json["removedVersions"][0], "2026.05.19");
    assert_eq!(json["keptVersions"][0], "2026.05.20");
}

#[test]
fn runtime_operation_progress_payload_serializes_camel_case() {
    let payload = app_lib::transport::tauri_commands::runtime::RuntimeOperationProgressPayload {
        operation_id: "op-1".to_string(),
        kind: "ensure".to_string(),
        phase: "download".to_string(),
        downloaded_bytes: Some(10),
        total_bytes: Some(20),
        percent: Some(50.0),
        attempt: 1,
        max_attempts: 3,
        resumed: false,
        status: "progress".to_string(),
        message: None,
        error: None,
    };

    let json = serde_json::to_value(payload).expect("serialize progress payload");

    assert_eq!(json["operationId"], "op-1");
    assert_eq!(json["downloadedBytes"], 10);
    assert_eq!(json["totalBytes"], 20);
    assert_eq!(json["maxAttempts"], 3);
}
