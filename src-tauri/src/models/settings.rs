#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_thinking_budget_tokens() -> u32 {
    8000
}

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
    #[serde(default)]
    pub bocha_api_key: String,
    #[serde(default)]
    pub custom_model_endpoint: String,
    #[serde(default)]
    pub custom_model_name: String,
    #[serde(default)]
    pub enable_taor_tracking: bool,
    #[serde(default)]
    pub use_cloud: bool,
    /// Cloud mode: selected model name from /v1/models (used when logged in).
    #[serde(default)]
    pub cloud_model: String,
    /// Cloud mode: model type ("chat" or "reasoner") for the selected cloud model.
    #[serde(default)]
    pub cloud_model_type: String,
    /// Whether persona onboarding has been completed.
    #[serde(default)]
    pub persona_onboarding_done: bool,
    /// Provider-aware extended thinking mode: disabled | enabled | adaptive.
    #[serde(default)]
    pub thinking_type: String,
    /// Budget tokens used when thinking_type == enabled.
    #[serde(default = "default_thinking_budget_tokens")]
    pub thinking_budget_tokens: u32,
    /// UI font scale: small | medium | large.
    #[serde(default = "default_font_scale")]
    pub font_scale: String,
    /// UI accent color hex string, e.g. "#DBAA22". Empty = use default.
    #[serde(default)]
    pub accent_color: String,
}

fn default_font_scale() -> String {
    "medium".to_string()
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
            data_masking_level: "relaxed".to_string(),
            auto_cleanup_enabled: true,
            temp_file_retention_days: 7,
            keep_old_versions: 1,
            tavily_api_key: String::new(),
            bocha_api_key: String::new(),
            custom_model_endpoint: String::new(),
            custom_model_name: String::new(),
            enable_taor_tracking: true,
            use_cloud: true,
            cloud_model: String::new(),
            cloud_model_type: String::new(),
            persona_onboarding_done: false,
            thinking_type: "disabled".to_string(),
            thinking_budget_tokens: default_thinking_budget_tokens(),
            font_scale: default_font_scale(),
            accent_color: String::new(),
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
            map.get(key)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(default)
        };
        let get_u32 = |key: &str, default: u32| -> u32 {
            map.get(key)
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(default)
        };

        Self {
            primary_model: get_str("primaryModel", &defaults.primary_model),
            primary_api_key: get_str("primaryApiKey", &defaults.primary_api_key),
            auto_model_routing: get_bool("autoModelRouting", defaults.auto_model_routing),
            workspace_path: get_str("workspacePath", &defaults.workspace_path),
            analysis_threshold: get_f64("analysisThreshold", defaults.analysis_threshold),
            data_masking_level: get_str("dataMaskingLevel", &defaults.data_masking_level),
            auto_cleanup_enabled: get_bool("autoCleanupEnabled", defaults.auto_cleanup_enabled),
            temp_file_retention_days: get_u32(
                "tempFileRetentionDays",
                defaults.temp_file_retention_days,
            ),
            keep_old_versions: get_u32("keepOldVersions", defaults.keep_old_versions),
            tavily_api_key: get_str("tavilyApiKey", &defaults.tavily_api_key),
            bocha_api_key: get_str("bochaApiKey", &defaults.bocha_api_key),
            custom_model_endpoint: get_str("customModelEndpoint", &defaults.custom_model_endpoint),
            custom_model_name: get_str("customModelName", &defaults.custom_model_name),
            enable_taor_tracking: get_bool("enableTaorTracking", defaults.enable_taor_tracking),
            use_cloud: get_bool("useCloud", defaults.use_cloud),
            cloud_model: get_str("cloudModel", &defaults.cloud_model),
            cloud_model_type: get_str("cloudModelType", &defaults.cloud_model_type),
            persona_onboarding_done: get_bool(
                "personaOnboardingDone",
                defaults.persona_onboarding_done,
            ),
            thinking_type: get_str("thinkingType", &defaults.thinking_type),
            thinking_budget_tokens: get_u32(
                "thinkingBudgetTokens",
                defaults.thinking_budget_tokens,
            ),
            font_scale: get_str("fontScale", &defaults.font_scale),
            accent_color: get_str("accentColor", &defaults.accent_color),
        }
    }
}
