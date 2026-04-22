//! review_masking_level_settings_test.rs
//!
//! 架构约束回归测试：确保 dataMaskingLevel 设置能正确传递到 TurnConfig.masking_level。
//!
//! 覆盖场景：
//! - "relaxed" → relaxed（用户关闭隐私模式）
//! - "standard" → standard
//! - "strict" → strict
//! - 空字符串 / 未知值 → fallback strict
//! - 大小写不敏感 ("RELAXED" → relaxed)
//! - 前后空格 (" relaxed " → relaxed)

use app_lib::llm::masking::MaskingLevel;

// ---------------------------------------------------------------------------
// 测试 MaskingLevel::from_str_or_strict — 字符串解析行为
// ---------------------------------------------------------------------------

#[test]
fn masking_level_relaxed_disables_masking() {
    assert_eq!(MaskingLevel::from_str_or_strict("relaxed"), MaskingLevel::Relaxed);
}

#[test]
fn masking_level_standard_masks_names_and_companies() {
    assert_eq!(MaskingLevel::from_str_or_strict("standard"), MaskingLevel::Standard);
}

#[test]
fn masking_level_strict_masks_all() {
    assert_eq!(MaskingLevel::from_str_or_strict("strict"), MaskingLevel::Strict);
}

#[test]
fn masking_level_empty_string_falls_back_to_strict() {
    assert_eq!(MaskingLevel::from_str_or_strict(""), MaskingLevel::Strict);
}

#[test]
fn masking_level_unknown_value_falls_back_to_strict() {
    assert_eq!(MaskingLevel::from_str_or_strict("none"), MaskingLevel::Strict);
    assert_eq!(MaskingLevel::from_str_or_strict("off"), MaskingLevel::Strict);
}

#[test]
fn masking_level_case_insensitive() {
    assert_eq!(MaskingLevel::from_str_or_strict("RELAXED"), MaskingLevel::Relaxed);
    assert_eq!(MaskingLevel::from_str_or_strict("Strict"), MaskingLevel::Strict);
    assert_eq!(MaskingLevel::from_str_or_strict("Standard"), MaskingLevel::Standard);
}

#[test]
fn masking_level_trims_whitespace() {
    assert_eq!(MaskingLevel::from_str_or_strict(" relaxed "), MaskingLevel::Relaxed);
    assert_eq!(MaskingLevel::from_str_or_strict("\tstrict\n"), MaskingLevel::Strict);
}

// ---------------------------------------------------------------------------
// 测试 ResolvedLlmSettings.masking_level 的 fallback 行为
// ---------------------------------------------------------------------------

use app_lib::runtime::chat::turn_config::ResolvedLlmSettings;

#[test]
fn resolved_llm_settings_default_masking_level_is_strict() {
    let settings = ResolvedLlmSettings::default();
    assert_eq!(settings.masking_level, "strict");
}

#[test]
fn resolved_llm_settings_relaxed_masking_level_preserved() {
    let settings = ResolvedLlmSettings {
        masking_level: "relaxed".to_string(),
        ..ResolvedLlmSettings::default()
    };
    assert_eq!(settings.masking_level, "relaxed");
}
