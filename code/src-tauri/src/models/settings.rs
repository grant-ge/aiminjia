#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    DeepseekV3,
    Volcano,
    Openai,
    Claude,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataMaskingLevel {
    Strict,
    Standard,
    Relaxed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub primary_model: String,
    pub primary_api_key: String,
    pub auto_model_routing: bool,
    pub workspace_path: String,
    pub analysis_threshold: f64,
    pub data_masking_level: String,
    pub auto_cleanup_enabled: bool,
    pub temp_file_retention_days: u32,
    pub keep_old_versions: u32,
    #[serde(default)]
    pub tavily_api_key: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        let default_workspace = dirs::home_dir()
            .map(|h| h.join(".renlijia").to_string_lossy().to_string())
            .unwrap_or_default();

        Self {
            primary_model: "deepseek-v3".to_string(),
            primary_api_key: String::new(),
            auto_model_routing: true,
            workspace_path: default_workspace,
            analysis_threshold: 1.65,
            data_masking_level: "strict".to_string(),
            auto_cleanup_enabled: true,
            temp_file_retention_days: 7,
            keep_old_versions: 1,
            tavily_api_key: String::new(),
        }
    }
}

impl AppSettings {
    /// Build AppSettings from a HashMap<String, String> as stored in the DB.
    ///
    /// The DB stores all values as strings, so we must parse booleans, numbers,
    /// and Option types manually. Falls back to default values for any field
    /// that is missing or fails to parse.
    pub fn from_string_map(map: &HashMap<String, String>) -> Self {
        let defaults = Self::default();

        let get_str = |key: &str, default: &str| -> String {
            map.get(key).cloned().unwrap_or_else(|| default.to_string())
        };
        let get_bool = |key: &str, default: bool| -> bool {
            map.get(key).map(|v| v == "true").unwrap_or(default)
        };
        let get_f64 = |key: &str, default: f64| -> f64 {
            map.get(key).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default)
        };
        let get_u32 = |key: &str, default: u32| -> u32 {
            map.get(key).and_then(|v| v.parse::<u32>().ok()).unwrap_or(default)
        };

        Self {
            primary_model: get_str("primaryModel", &defaults.primary_model),
            primary_api_key: get_str("primaryApiKey", &defaults.primary_api_key),
            auto_model_routing: get_bool("autoModelRouting", defaults.auto_model_routing),
            workspace_path: get_str("workspacePath", &defaults.workspace_path),
            analysis_threshold: get_f64("analysisThreshold", defaults.analysis_threshold),
            data_masking_level: get_str("dataMaskingLevel", &defaults.data_masking_level),
            auto_cleanup_enabled: get_bool("autoCleanupEnabled", defaults.auto_cleanup_enabled),
            temp_file_retention_days: get_u32("tempFileRetentionDays", defaults.temp_file_retention_days),
            keep_old_versions: get_u32("keepOldVersions", defaults.keep_old_versions),
            tavily_api_key: get_str("tavilyApiKey", &defaults.tavily_api_key),
        }
    }
}
