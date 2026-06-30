#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_thinking_budget_tokens() -> u32 {
    8000
}

fn default_chat_width_mode() -> String {
    "full".to_string()
}

fn default_permission_mode() -> String {
    "default".to_string()
}

fn default_managed_runtime_enabled() -> bool {
    true
}

fn normalize_default_permission_mode(value: &str) -> String {
    match value {
        "default" | "fullAccess" => value.to_string(),
        _ => default_permission_mode(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    DeepseekV3,
    Volcano,
    Openai,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloudGatewayMode {
    Legacy,
    V2,
}

impl Default for CloudGatewayMode {
    fn default() -> Self {
        CloudGatewayMode::V2
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub primary_model: String,
    pub primary_api_key: String,
    pub auto_model_routing: bool,
    pub workspace_path: String,
    pub analysis_threshold: f64,
    pub auto_cleanup_enabled: bool,
    pub temp_file_retention_days: u32,
    pub keep_old_versions: u32,
    #[serde(default)]
    pub custom_model_endpoint: String,
    #[serde(default)]
    pub custom_model_name: String,
    /// Cloud mode: selected logical model name from AIjia Gateway V2.
    #[serde(default)]
    pub cloud_model: String,
    /// Cloud mode: model type ("chat" or "reasoner") for the selected cloud model.
    #[serde(default)]
    pub cloud_model_type: String,
    /// Cloud gateway mode. Kept for settings-file compatibility; desktop always uses V2.
    #[serde(default)]
    pub cloud_gateway_mode: CloudGatewayMode,
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
    /// Chat content width mode: centered | full.
    #[serde(default = "default_chat_width_mode")]
    pub chat_width_mode: String,
    /// Default tool permission mode for new turns: default | fullAccess.
    #[serde(default = "default_permission_mode")]
    pub default_permission_mode: String,
    /// Whether child command processes should prefer AIjia's bundled Node/Python/uv runtime.
    #[serde(default = "default_managed_runtime_enabled")]
    pub managed_runtime_enabled: bool,
    /// JSON-stringified `AuthorizedWorkspaceRef` ({id, rootPath, displayName}) — 首页 task composer
    /// 当前选中的 workspace。空字符串视为未选中。
    #[serde(default)]
    pub ui_home_selected_workspace: String,
    /// JSON-stringified `AuthorizedWorkspaceRef[]` — 首页切换器最近 workspace 列表。
    /// 空字符串或 "[]" 视为空列表。**前端限定最多 10 条**：超出时 LRU 截断（新加入的在前，超过 10 截断）。
    #[serde(default)]
    pub ui_home_recent_workspaces: String,
    /// JSON-stringified map of sidebar project id -> collapsed state.
    /// User-scoped UI preference; empty string means no collapsed projects.
    #[serde(default)]
    pub ui_sidebar_collapsed_projects: String,
    /// JSON-stringified map of conversation id -> cached sidebar pending status.
    /// UI hint only; live runtime state remains the source of truth.
    #[serde(default)]
    pub ui_sidebar_conversation_statuses: String,
    /// Manual context window override (in tokens). When set, takes priority
    /// over model-name-based context window resolution.
    #[serde(default)]
    pub context_window: Option<usize>,
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
            auto_cleanup_enabled: true,
            temp_file_retention_days: 7,
            keep_old_versions: 1,
            custom_model_endpoint: String::new(),
            custom_model_name: String::new(),
            cloud_model: String::new(),
            cloud_model_type: String::new(),
            cloud_gateway_mode: CloudGatewayMode::V2,
            persona_onboarding_done: false,
            thinking_type: "disabled".to_string(),
            thinking_budget_tokens: default_thinking_budget_tokens(),
            font_scale: default_font_scale(),
            accent_color: String::new(),
            chat_width_mode: default_chat_width_mode(),
            default_permission_mode: default_permission_mode(),
            managed_runtime_enabled: default_managed_runtime_enabled(),
            ui_home_selected_workspace: String::new(),
            ui_home_recent_workspaces: String::new(),
            ui_sidebar_collapsed_projects: String::new(),
            ui_sidebar_conversation_statuses: String::new(),
            context_window: None,
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
        let get_usize_option = |key: &str| -> Option<usize> {
            map.get(key)
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|v| *v > 0)
        };

        Self {
            primary_model: get_str("primaryModel", &defaults.primary_model),
            primary_api_key: get_str("primaryApiKey", &defaults.primary_api_key),
            auto_model_routing: get_bool("autoModelRouting", defaults.auto_model_routing),
            workspace_path: get_str("workspacePath", &defaults.workspace_path),
            analysis_threshold: get_f64("analysisThreshold", defaults.analysis_threshold),
            auto_cleanup_enabled: get_bool("autoCleanupEnabled", defaults.auto_cleanup_enabled),
            temp_file_retention_days: get_u32(
                "tempFileRetentionDays",
                defaults.temp_file_retention_days,
            ),
            keep_old_versions: get_u32("keepOldVersions", defaults.keep_old_versions),
            custom_model_endpoint: get_str("customModelEndpoint", &defaults.custom_model_endpoint),
            custom_model_name: get_str("customModelName", &defaults.custom_model_name),
            cloud_model: get_str("cloudModel", &defaults.cloud_model),
            cloud_model_type: get_str("cloudModelType", &defaults.cloud_model_type),
            cloud_gateway_mode: CloudGatewayMode::V2,
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
            chat_width_mode: get_str("chatWidthMode", &defaults.chat_width_mode),
            default_permission_mode: normalize_default_permission_mode(&get_str(
                "defaultPermissionMode",
                &defaults.default_permission_mode,
            )),
            managed_runtime_enabled: get_bool(
                "managedRuntimeEnabled",
                defaults.managed_runtime_enabled,
            ),
            ui_home_selected_workspace: get_str(
                "uiHomeSelectedWorkspace",
                &defaults.ui_home_selected_workspace,
            ),
            ui_home_recent_workspaces: get_str(
                "uiHomeRecentWorkspaces",
                &defaults.ui_home_recent_workspaces,
            ),
            ui_sidebar_collapsed_projects: get_str(
                "uiSidebarCollapsedProjects",
                &defaults.ui_sidebar_collapsed_projects,
            ),
            ui_sidebar_conversation_statuses: get_str(
                "uiSidebarConversationStatuses",
                &defaults.ui_sidebar_conversation_statuses,
            ),
            context_window: get_usize_option("contextWindow"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, CloudGatewayMode};
    use std::collections::HashMap;

    #[test]
    fn cloud_gateway_mode_defaults_to_v2() {
        assert_eq!(CloudGatewayMode::default(), CloudGatewayMode::V2);
        assert_eq!(
            AppSettings::default().cloud_gateway_mode,
            CloudGatewayMode::V2
        );
    }

    #[test]
    fn cloud_gateway_mode_from_string_map_ignores_legacy_value() {
        let mut map = HashMap::new();
        map.insert("cloudGatewayMode".to_string(), "legacy".to_string());

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(settings.cloud_gateway_mode, CloudGatewayMode::V2);
    }

    #[test]
    fn defaults_chat_width_mode_to_full() {
        assert_eq!(AppSettings::default().chat_width_mode, "full");
    }

    #[test]
    fn defaults_font_scale_to_medium() {
        assert_eq!(AppSettings::default().font_scale, "medium");
    }

    #[test]
    fn defaults_permission_mode_to_default() {
        assert_eq!(AppSettings::default().default_permission_mode, "default");
    }

    #[test]
    fn defaults_managed_runtime_enabled() {
        assert!(AppSettings::default().managed_runtime_enabled);
    }

    #[test]
    fn reads_managed_runtime_enabled_from_string_map() {
        let mut map = HashMap::new();
        map.insert("managedRuntimeEnabled".to_string(), "false".to_string());

        let settings = AppSettings::from_string_map(&map);

        assert!(!settings.managed_runtime_enabled);
    }

    #[test]
    fn reads_default_permission_mode_from_string_map() {
        let mut map = HashMap::new();
        map.insert(
            "defaultPermissionMode".to_string(),
            "fullAccess".to_string(),
        );

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(settings.default_permission_mode, "fullAccess");
    }

    #[test]
    fn invalid_default_permission_mode_falls_back() {
        let mut map = HashMap::new();
        map.insert(
            "defaultPermissionMode".to_string(),
            "sudoForever".to_string(),
        );

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(settings.default_permission_mode, "default");
    }

    #[test]
    fn default_settings_do_not_serialize_legacy_data_masking_level() {
        let value = serde_json::to_value(AppSettings::default()).unwrap();

        assert!(value.get("dataMaskingLevel").is_none());
    }

    #[test]
    fn reads_chat_width_mode_from_string_map() {
        let mut map = HashMap::new();
        map.insert("chatWidthMode".to_string(), "full".to_string());

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(settings.chat_width_mode, "full");
    }

    #[test]
    fn home_workspace_fields_default_to_empty_string() {
        let s = AppSettings::default();
        assert_eq!(s.ui_home_selected_workspace, "");
        assert_eq!(s.ui_home_recent_workspaces, "");
        assert_eq!(s.ui_sidebar_collapsed_projects, "");
    }

    #[test]
    fn ui_preference_fields_round_trip_through_json() {
        let s = AppSettings {
            ui_home_selected_workspace: r#"{"id":"ws-1","rootPath":"/x","displayName":"x"}"#
                .to_string(),
            ui_home_recent_workspaces: r#"[{"id":"ws-1","rootPath":"/x","displayName":"x"}]"#
                .to_string(),
            ui_sidebar_collapsed_projects: r#"{"default":true}"#.to_string(),
            ui_sidebar_conversation_statuses:
                r#"{"conv-a":{"kind":"permission-review","updatedAt":1}}"#.to_string(),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.ui_home_selected_workspace,
            s.ui_home_selected_workspace
        );
        assert_eq!(
            parsed.ui_home_recent_workspaces,
            s.ui_home_recent_workspaces
        );
        assert_eq!(
            parsed.ui_sidebar_collapsed_projects,
            s.ui_sidebar_collapsed_projects
        );
        assert_eq!(
            parsed.ui_sidebar_conversation_statuses,
            s.ui_sidebar_conversation_statuses
        );
    }

    #[test]
    fn reads_sidebar_collapsed_projects_from_string_map() {
        let mut map = HashMap::new();
        map.insert(
            "uiSidebarCollapsedProjects".to_string(),
            r#"{"project-a":true}"#.to_string(),
        );

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(
            settings.ui_sidebar_collapsed_projects,
            r#"{"project-a":true}"#
        );
    }

    #[test]
    fn reads_sidebar_conversation_statuses_from_string_map() {
        let mut map = HashMap::new();
        map.insert(
            "uiSidebarConversationStatuses".to_string(),
            r#"{"conv-a":{"kind":"permission-review","updatedAt":1}}"#.to_string(),
        );

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(
            settings.ui_sidebar_conversation_statuses,
            r#"{"conv-a":{"kind":"permission-review","updatedAt":1}}"#
        );
    }

    #[test]
    fn reads_context_window_from_string_map() {
        let mut map = HashMap::new();
        map.insert("contextWindow".to_string(), "32000".to_string());

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(settings.context_window, Some(32_000));
    }

    #[test]
    fn ignores_invalid_context_window_from_string_map() {
        let mut map = HashMap::new();
        map.insert("contextWindow".to_string(), "0".to_string());

        let settings = AppSettings::from_string_map(&map);

        assert_eq!(settings.context_window, None);
    }
}
