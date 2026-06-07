use std::path::Path;

use crate::storage::GlobalConfigStore;

pub const CONFIG_KEY: &str = "log_level";

/// Validate and normalise a level string. Returns the lowercase form if valid.
pub fn validate(s: &str) -> Option<String> {
    let lower = s.to_lowercase();
    match lower.as_str() {
        "error" | "warn" | "info" | "debug" | "trace" => Some(lower),
        _ => None,
    }
}

/// Convert a validated level string to a `log::LevelFilter`.
pub fn to_log_filter(s: &str) -> log::LevelFilter {
    match s {
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    }
}

/// Read the persisted log level from global config.
/// Returns `"info"` if nothing is stored or the stored value is unrecognised.
pub fn read_persisted(global_dir: &Path) -> String {
    GlobalConfigStore::new(global_dir.to_path_buf())
        .get_setting(CONFIG_KEY)
        .ok()
        .flatten()
        .and_then(|s| validate(&s))
        .unwrap_or_else(|| "info".to_string())
}
