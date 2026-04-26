pub const DEFAULT_RUNTIME_MANIFEST_URL: &str =
    "https://datamind-pzc.oss-cn-hangzhou.aliyuncs.com/runtimes/runtime-manifest.json";

pub fn configured_runtime_manifest_url() -> String {
    std::env::var("RENLIJIA_RUNTIME_MANIFEST_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_RUNTIME_MANIFEST_URL.to_string())
}
