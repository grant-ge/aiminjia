use crate::storage::GlobalConfigStore;

pub const APP_LOG_LEVEL_KEY: &str = "appLogLevel";
pub const DEFAULT_APP_LOG_LEVEL: &str = "info";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl AppLogLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    pub fn to_level_filter(self) -> log::LevelFilter {
        match self {
            Self::Error => log::LevelFilter::Error,
            Self::Warn => log::LevelFilter::Warn,
            Self::Info => log::LevelFilter::Info,
            Self::Debug => log::LevelFilter::Debug,
        }
    }
}

impl Default for AppLogLevel {
    fn default() -> Self {
        Self::Info
    }
}

pub fn normalize_app_log_level(value: &str) -> Result<&'static str, String> {
    AppLogLevel::parse(value)
        .map(AppLogLevel::as_str)
        .ok_or_else(|| format!("Invalid app log level: {value}"))
}

pub fn read_app_log_level(global_store: &GlobalConfigStore) -> AppLogLevel {
    global_store
        .get_setting(APP_LOG_LEVEL_KEY)
        .ok()
        .flatten()
        .as_deref()
        .and_then(AppLogLevel::parse)
        .unwrap_or_default()
}

pub fn read_app_log_level_string(global_store: &GlobalConfigStore) -> String {
    read_app_log_level(global_store).as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn app_log_level_defaults_to_info() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());

        assert_eq!(read_app_log_level(&store), AppLogLevel::Info);
        assert_eq!(read_app_log_level_string(&store), DEFAULT_APP_LOG_LEVEL);
    }

    #[test]
    fn app_log_level_reads_valid_global_setting() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting(APP_LOG_LEVEL_KEY, "debug").unwrap();

        assert_eq!(read_app_log_level(&store), AppLogLevel::Debug);
        assert_eq!(
            read_app_log_level(&store).to_level_filter(),
            log::LevelFilter::Debug
        );
    }

    #[test]
    fn app_log_level_rejects_invalid_values_for_writes() {
        assert_eq!(normalize_app_log_level("WARN").unwrap(), "warn");
        assert!(normalize_app_log_level("trace").is_err());
        assert!(normalize_app_log_level("verbose").is_err());
    }

    #[test]
    fn app_log_level_falls_back_for_invalid_persisted_values() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting(APP_LOG_LEVEL_KEY, "trace").unwrap();

        assert_eq!(read_app_log_level(&store), AppLogLevel::Info);
    }
}
