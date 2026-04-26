use app_lib::runtime::dependencies::{
    configured_runtime_manifest_url, DEFAULT_RUNTIME_MANIFEST_URL,
};

#[test]
fn uses_built_in_oss_manifest_url_when_env_override_is_missing() {
    std::env::remove_var("RENLIJIA_RUNTIME_MANIFEST_URL");

    assert_eq!(
        configured_runtime_manifest_url(),
        DEFAULT_RUNTIME_MANIFEST_URL.to_string()
    );
    assert_eq!(
        DEFAULT_RUNTIME_MANIFEST_URL,
        "https://datamind-pzc.oss-cn-hangzhou.aliyuncs.com/runtimes/runtime-manifest.json"
    );
}

#[test]
fn env_manifest_url_overrides_built_in_default_for_enterprise_or_dev() {
    std::env::set_var(
        "RENLIJIA_RUNTIME_MANIFEST_URL",
        "https://mirror.example.com/runtime-manifest.json",
    );

    assert_eq!(
        configured_runtime_manifest_url(),
        "https://mirror.example.com/runtime-manifest.json"
    );

    std::env::remove_var("RENLIJIA_RUNTIME_MANIFEST_URL");
}
