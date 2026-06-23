//! Verify SubAgentConfig.model_override propagates to AppSettings used by gateway.
//!
//! We cannot easily mock LlmGateway::stream_message, so this test verifies the
//! per-call settings transformation logic in isolation: given a base AppSettings
//! and a SubAgentConfig with model_override set, the worker produces an effective
//! AppSettings whose primary_model matches the override.

use app_lib::models::settings::AppSettings;
use app_lib::runtime::agent::worker_runtime::effective_settings_for_subagent;

#[test]
fn model_override_some_replaces_primary_model() {
    let base = AppSettings {
        primary_model: "deepseek-v3".to_string(),
        ..AppSettings::default()
    };
    let effective = effective_settings_for_subagent(&base, Some("haiku"));
    assert_eq!(effective.primary_model, "haiku");
    // base unchanged
    assert_eq!(base.primary_model, "deepseek-v3");
}

#[test]
fn model_override_none_inherits_parent_model() {
    let base = AppSettings {
        primary_model: "custom".to_string(),
        ..AppSettings::default()
    };
    let effective = effective_settings_for_subagent(&base, None);
    assert_eq!(effective.primary_model, "custom");
}

#[test]
fn model_override_empty_string_treated_as_inherit() {
    // Defensive: SubAgentConfig.model_override is Option<String>; empty string should
    // be treated as "no override" to avoid breaking gateway routing on bad input.
    let base = AppSettings {
        primary_model: "custom".to_string(),
        ..AppSettings::default()
    };
    let effective = effective_settings_for_subagent(&base, Some(""));
    assert_eq!(effective.primary_model, "custom");
}
